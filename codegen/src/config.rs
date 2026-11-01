//! `comline.toml` — the consumer-owned code-generation config.
//!
//! `config.idp` (the congregation) declares *what* a package can be generated as
//! (`code_generation.languages`); it is frozen into the package and must not
//! carry output paths. `comline.toml` is the other half: it belongs to whoever
//! *consumes* the package and says where generated code lands, in what layout,
//! and in what form. It is plain committed source — never frozen, never in CAS.
//!
//! Field precedence, lowest to highest: built-in defaults → `[generate]` →
//! `[[generate.target]]` → `COMLINE_GENERATE_*` env → CLI flags.
//!
//! The types here are the one definition of the file: the CLI parses and
//! resolves with them, and [`json_schema`] derives the JSON Schema editors
//! validate it with (`schemas/comline-toml.json`, checked against these types
//! by a test). A new key goes here, with a doc comment - that comment is the
//! description an editor shows.
//!
//! Rendering a target's `layout` into a path stays with the CLI: it needs
//! core's template engine, which is part of core's `compiler` feature, and
//! this crate builds without it.

use std::path::{Path, PathBuf};

use crate::validation::ValidateOnDecode;
use crate::versions::LangVersion;
use comline_core::schema::idl::annotations;
use miette::{miette, IntoDiagnostic, Result, WrapErr};
use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::Deserialize;

/// File looked for in the project directory.
pub const FILE_NAME: &str = "comline.toml";

/// Default output root, relative to `comline.toml`.
pub const DEFAULT_OUT: &str = "generated";
/// Default on-disk layout under a target's root.
pub const DEFAULT_LAYOUT: &str = "{{language}}/{{namespace}}.{{ext}}";
/// Default emit form. `"code"` and `"lib"` are implemented; `"dylib"` is not.
pub const DEFAULT_MODE: &str = "code";
/// The emit forms `mode` accepts.
pub const MODES: [&str; 2] = ["code", "lib"];
/// The wall-time limit for each call to an external `generator`, when its
/// target sets no `timeout`.
pub const DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Where the JSON Schema for `comline.toml` is published. Versioned by the
/// file's compatibility line: `0.x` until the 1.0 announcement.
pub const SCHEMA_ID: &str = "https://comlineproject.github.io/docs/schemas/comline-toml/0.x.json";

/// Whether `std` is referenced as its own package rather than inlined, when
/// nothing says either way.
///
/// It follows the mode, because the two modes differ in whether there is
/// anywhere to put the dependency. `lib` writes the package manifest, so the
/// generator adds `comline_std` to it and the crate links one shared std.
/// `code` emits loose source files with no manifest to add anything to - all
/// a generator can do there is leave a comment naming the dependency line to
/// write by hand, so inlining stays the honest default.
fn default_external_std(mode: &str) -> bool {
    mode == "lib"
}

/// Which package versions to generate bindings for.
///
/// TOML: `package_versions = "latest"` (default) | `"all"` | `"0.3.0"` |
/// `["0.3.0", "0.4.0"]`. `latest` is the working tree; anything else reads
/// committed versions from the CAS chain: `all` every release, a list any
/// version named, a build (`0.3.0-dev.2`) included.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum VersionSpec {
    /// The working tree (no CAS read).
    #[default]
    Latest,
    /// Every committed version in the chain.
    All,
    /// Specific committed versions (each a version string or commit hash).
    List(Vec<String>),
}

impl JsonSchema for VersionSpec {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "VersionSpec".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Which package versions to generate bindings for: `latest` (the \
                working tree, the default), `all` (every committed version), or one version \
                or a list of them, each a version string or commit hash.",
            "anyOf": [
                { "type": "string", "examples": ["latest", "all", "0.3.0"] },
                { "type": "array", "items": { "type": "string" } }
            ]
        })
    }
}

impl<'de> Deserialize<'de> for VersionSpec {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            One(String),
            Many(Vec<String>),
        }
        Ok(match Raw::deserialize(d)? {
            Raw::One(s) if s.eq_ignore_ascii_case("latest") => VersionSpec::Latest,
            Raw::One(s) if s.eq_ignore_ascii_case("all") => VersionSpec::All,
            Raw::One(s) => VersionSpec::List(vec![s]),
            Raw::Many(v) => VersionSpec::List(v),
        })
    }
}

/// `comline.toml`: where `comline generate` writes code, and how. Which
/// languages are generated comes from `config.idp`'s
/// `code_generation.languages`; this file says where each one lands.
// An absent file parses as `Self::default`.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComlineToml {
    /// Code-generation settings: defaults shared by every target, and
    /// `[[generate.target]]` blocks configuring one language each.
    #[serde(default)]
    pub generate: Generate,
    /// Documentation settings: where `comline doc` writes, in what format,
    /// with which renderer.
    #[serde(default)]
    pub doc: Doc,
}

/// Default output root for `comline doc`, relative to `comline.toml`.
pub const DEFAULT_DOC_OUT: &str = "docs/api";
/// Default `comline doc` formats.
pub const DEFAULT_DOC_FORMATS: &[&str] = &["markdown"];

/// The `[doc]` table: `comline doc`'s settings. Each key has a flag that
/// overrides it.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Doc {
    /// Output directory, relative to `comline.toml`. Default `docs/api`.
    /// `--out` overrides it.
    #[schemars(extend("default" = DEFAULT_DOC_OUT))]
    pub out: Option<String>,
    /// What to write, each into `<out>/<format>/` in one run: `markdown`
    /// (pages, from the official renderer or `renderer`), `html` (a static
    /// site, from the official HTML renderer or `renderer`), `json` (the
    /// documentation model itself, `model.json`), or another renderer id
    /// `renderer` offers. Default `["markdown"]`. `--format` overrides it.
    #[schemars(extend("default" = DEFAULT_DOC_FORMATS, "examples" = [["html", "markdown", "json"]]))]
    pub formats: Option<Vec<String>>,
    /// Which versions to document. Default `"all"`: every release, every
    /// dev build and the unreleased working state. A table trims the list:
    /// `{ last = 5 }` keeps the newest five, `{ since = "0.3.0" }` keeps that
    /// version and newer, `{ releases_only = true }` keeps releases alone
    /// (no dev builds, no unreleased state); they combine. `--version`
    /// documents one version instead.
    #[schemars(extend("default" = "all"))]
    pub versions: Option<DocVersions>,
    /// A renderer outside this binary: a path to a `.wasm` (run sandboxed
    /// under wasmtime) or to an executable, relative to `comline.toml`'s
    /// directory, run exactly like an external `generator`. Unset, the
    /// built-in renderer for `format` is used.
    pub renderer: Option<String>,
    /// The wall-time limit for each call to `renderer`, in seconds.
    /// Default 60.
    #[schemars(range(min = 1))]
    pub timeout: Option<u64>,
    /// Where each dependency's documentation lives, by the name the package
    /// gives the dependency (or `std`): `links = { std = "https://..." }`. A
    /// type from a dependency listed here links into its docs; one that
    /// isn't is plain text.
    pub links: std::collections::BTreeMap<String, String>,
    /// Add a changelog from this built version (a version or commit hash) to
    /// the documented one. `--changelog-from` overrides it.
    pub changelog_from: Option<String>,
}

/// `[doc] versions`: every version, or a trimmed list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum DocVersions {
    /// `"all"`: every release, dev build and the unreleased state.
    All(AllVersions),
    /// A trimmed list.
    Select(DocVersionSelect),
}

/// The keyword `"all"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
pub enum AllVersions {
    #[serde(rename = "all")]
    All,
}

/// `[doc] versions` as a table: each key trims the list, and they combine
/// (`releases_only` first, then `since`, then `last`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DocVersionSelect {
    /// Keep the newest N (the unreleased state counts as one).
    #[schemars(range(min = 1))]
    pub last: Option<usize>,
    /// Keep this built version (a version string) and every newer one.
    pub since: Option<String>,
    /// Keep releases only: no dev builds and no unreleased state.
    pub releases_only: bool,
}

/// The `[generate]` table.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Generate {
    /// Narrow generation to these languages, out of the ones `config.idp`
    /// declares. Empty (the default) means every declared language.
    ///
    /// The consumer's half of the split: `config.idp` says which languages
    /// the *package* is for, and this says which of them *this checkout*
    /// wants built. A name the congregation doesn't declare is an error
    /// rather than a silent no-op, so a typo here can't quietly stop
    /// generating something.
    pub only: Vec<String>,
    /// Output root, relative to `comline.toml`, shared by every target unless
    /// overridden. Default `generated`.
    #[schemars(extend("default" = DEFAULT_OUT))]
    pub out: Option<String>,
    /// Where each file lands under the output root, as a template over
    /// `{{language}}`, `{{namespace}}`, `{{ext}}`, `{{lang_version}}`,
    /// `{{spec_version}}` and `{{package_version}}`. Shared by every target
    /// unless overridden. Has no effect in `lib` mode.
    #[schemars(extend("default" = DEFAULT_LAYOUT))]
    pub layout: Option<String>,
    /// Emit form shared by every target unless overridden: `code` (loose
    /// source files, the default) or `lib` (a buildable package).
    #[schemars(schema_with = "mode_schema", extend("default" = DEFAULT_MODE))]
    pub mode: Option<String>,
    /// Version selection shared by every target unless overridden.
    pub package_versions: Option<VersionSpec>,
    /// Moved to config.idp's `wire = { framing = "..." }`: the framing is part
    /// of the schema, which both ends must agree on. Read only to say so, and
    /// left out of the JSON Schema, so an editor flags it.
    #[schemars(skip)]
    pub default_framing: Option<String>,
    /// `true`: reference `comline_std` as its own package instead of inlining
    /// it, shared by every target unless overridden. Only a generator that
    /// knows how (today, Rust's) acts on it. Defaults on in `lib` mode, off in
    /// `code` mode.
    pub external_std: Option<bool>,
    /// Whether decoding a value runs its `@validators` checks: `off` (the
    /// default), `collect` (fail with every failure) or `first` (fail with
    /// the first). Shared by every target unless overridden.
    #[schemars(schema_with = "validate_on_decode_schema", extend("default" = "off"))]
    pub validate_on_decode: Option<String>,
    /// `true`: protocols on a connection get async handlers and senders
    /// (Rust's `AsyncConnection`), shared by every target unless overridden.
    /// A generator whose output is async anyway ignores it. Default `false`.
    #[serde(rename = "async")]
    #[schemars(extend("default" = false))]
    pub async_api: Option<bool>,
    /// Which type implements each encoding (format, framing, handshake) in
    /// the generated code, by encoding name:
    /// `encodings = { "com.acme.lines@1" = "acme::LinesFraming" }`, or with
    /// the package it comes from, `{ type = "...", package = "...", version
    /// = "..." }`. Overrides the declaring package's suggested mapping, and
    /// maps an encoding that has none. A target's own `encodings` override
    /// these, name by name.
    pub encodings: EncodingMap,
    /// `[[generate.target]]` blocks.
    #[serde(rename = "target")]
    pub targets: Vec<Target>,
}

/// `encodings = { ... }`, by encoding name as written.
pub type EncodingMap = std::collections::BTreeMap<String, EncodingEntry>;

/// One entry of `encodings`: the type alone, or with its package.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum EncodingEntry {
    /// The type implementing the encoding, e.g. `"acme::LinesFraming"`.
    Type(String),
    /// The type and the package it comes from, e.g.
    /// `{ type = "LinesFraming", package = "acme-encodings", version = "^0.2" }`.
    Full(EncodingTable),
}

/// The table form of an `encodings` entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EncodingTable {
    /// The type implementing the encoding, as the generated code names it.
    #[serde(rename = "type")]
    pub ty: String,
    /// The package the type comes from.
    pub package: Option<String>,
    /// The version requirement on that package.
    pub version: Option<String>,
}

impl EncodingEntry {
    pub fn mapping(&self) -> crate::protocol::EncodingMapping {
        match self {
            EncodingEntry::Type(ty) => crate::protocol::EncodingMapping {
                type_path: ty.clone(),
                package: None,
                version: None,
            },
            EncodingEntry::Full(t) => crate::protocol::EncodingMapping {
                type_path: t.ty.clone(),
                package: t.package.clone(),
                version: t.version.clone(),
            },
        }
    }
}

/// `[generate]`'s `encodings` with a target's over them, by canonical
/// name, so `jsonrpc@1` in one and `json-rpc` in the other are the same
/// entry and the target's wins.
fn merge_encodings(
    section: &EncodingMap,
    target: &EncodingMap,
) -> std::collections::BTreeMap<String, crate::protocol::EncodingMapping> {
    let mut merged = std::collections::BTreeMap::new();
    for (name, entry) in section.iter().chain(target) {
        merged.insert(crate::encodings::canonical(name), entry.mapping());
    }
    merged
}

/// One `[[generate.target]]` block: the settings for one language the
/// package declares. It does not decide which languages are generated, and
/// naming one `config.idp` doesn't declare is an error.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// The language this block configures. Must match a language the
    /// package declares under `code_generation.languages` in `config.idp`.
    /// With `generator`, it also picks among the languages that generator
    /// offers, and may be left out when it offers one.
    // `comline generate` fills it in from the generator before [`resolve`].
    #[serde(default)]
    #[schemars(transform = no_default)]
    pub language: String,
    /// A generator outside this binary: a path to a `.wasm` (run sandboxed
    /// under wasmtime) or to an executable, relative to `comline.toml`'s
    /// directory. Unset, the built-in generator for `language` is used.
    pub generator: Option<String>,
    /// The wall-time limit for each call to `generator`, in seconds.
    /// Default 60.
    #[schemars(range(min = 1))]
    pub timeout: Option<u64>,
    /// Language/toolchain version selecting the generator (e.g. `1.88.0`).
    /// Falls back to the version the congregation declared for this language,
    /// and may raise it but never lower it.
    pub lang_version: Option<String>,
    /// This target's output root, relative to `comline.toml`. Overrides
    /// `[generate] out`.
    pub out: Option<String>,
    /// This target's layout template. Overrides `[generate] layout`.
    pub layout: Option<String>,
    /// This target's emit form: `code` or `lib`. Overrides `[generate] mode`.
    // `default` only so the schema knows the key is optional: an
    // `Option` is already `None` when left out.
    #[serde(default)]
    #[schemars(schema_with = "mode_schema")]
    pub mode: Option<String>,
    /// This target's version selection. Overrides
    /// `[generate] package_versions`.
    pub package_versions: Option<VersionSpec>,
    /// Moved to config.idp; see [`Generate::default_framing`].
    #[schemars(skip)]
    pub default_framing: Option<String>,
    /// Whether this target references `comline_std` as its own package.
    /// Overrides `[generate] external_std`.
    pub external_std: Option<bool>,
    /// Whether this target's decoding runs `@validators` checks: `off`,
    /// `collect` or `first`. Overrides `[generate] validate_on_decode`.
    // `default` only so the schema knows the key is optional: an
    // `Option` is already `None` when left out.
    #[serde(default)]
    #[schemars(schema_with = "validate_on_decode_schema")]
    pub validate_on_decode: Option<String>,
    /// Whether this target's protocols on a connection get async handlers and
    /// senders. Overrides `[generate] async`.
    #[serde(rename = "async")]
    pub async_api: Option<bool>,
    /// Which type implements each encoding, by encoding name, for this
    /// target. Overrides `[generate] encodings`, name by name.
    #[serde(default)]
    pub encodings: EncodingMap,
    /// `lib` mode only: write the crate straight into `out` instead of
    /// `<out>/<language>/`. Target-only (no `[generate]` default, no flag/env)
    /// — a shared `out` across several `lib` targets needs the per-language
    /// split to avoid two crates colliding in one directory, so flattening
    /// only makes sense once a target already has `out` to itself.
    #[serde(default)]
    pub flatten: bool,
}

/// `mode`: one of [`MODES`].
fn mode_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "string", "enum": MODES })
}

/// `validate_on_decode`: one of [`ValidateOnDecode::NAMES`].
fn validate_on_decode_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "string", "enum": ValidateOnDecode::NAMES })
}

/// An empty `language` is "left out", not a value to suggest.
fn no_default(schema: &mut Schema) {
    schema.remove("default");
}

/// TOML has no null - an optional key is one left out - so the `null` a
/// schema for an `Option` admits is taken back out.
fn drop_null(value: &mut serde_json::Value) {
    use serde_json::Value;
    let is_null = |v: &Value| v.get("type").and_then(Value::as_str) == Some("null");
    match value {
        Value::Object(object) => {
            if let Some(Value::Array(types)) = object.get_mut("type") {
                types.retain(|t| t != "null");
                if let [only] = types.as_slice() {
                    let only = only.clone();
                    object.insert("type".into(), only);
                }
            }
            for key in ["anyOf", "oneOf"] {
                if let Some(Value::Array(branches)) = object.get_mut(key) {
                    branches.retain(|b| !is_null(b));
                    if let [only] = branches.as_slice() {
                        let Value::Object(mut only) = only.clone() else {
                            continue;
                        };
                        object.remove(key);
                        // Draft-07 ignores a `$ref`'s siblings, such as the
                        // field's description: keep it under `allOf`.
                        if let Some(reference) = only.remove("$ref") {
                            let all_of = vec![serde_json::json!({ "$ref": reference })];
                            object.insert("allOf".into(), all_of.into());
                        }
                        object.extend(only);
                    }
                }
            }
            // A default of "unset" is no default at all.
            if object.get("default").is_some_and(Value::is_null) {
                object.remove("default");
            }
            object.values_mut().for_each(drop_null);
        }
        Value::Array(items) => items.iter_mut().for_each(drop_null),
        _ => {}
    }
}

/// The JSON Schema for `comline.toml`, derived from [`ComlineToml`]: draft-07,
/// the newest draft Taplo (Even Better TOML) validates against.
pub fn json_schema() -> serde_json::Value {
    let schema = schemars::generate::SchemaSettings::draft07()
        .into_generator()
        .into_root_schema_for::<ComlineToml>();
    let mut value = schema.to_value();
    drop_null(&mut value);
    let object = value.as_object_mut().expect("a root schema is an object");
    object.insert("$id".into(), SCHEMA_ID.into());
    object.insert("title".into(), "comline.toml".into());
    value
}

/// A non-empty `COMLINE_GENERATE_*` env var, if set.
fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

/// Same as [`env_var`], parsed as a bool (`"true"`/`"1"` ⇒ on,
/// `"false"`/`"0"` ⇒ off; anything else unset, same as absent).
fn env_var_bool(name: &str) -> Option<bool> {
    match env_var(name)?.as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

impl ComlineToml {
    /// Parse the text of a `comline.toml`.
    pub fn parse(text: &str) -> std::result::Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Load `<work_dir>/comline.toml`; a missing file yields defaults.
    pub fn load(work_dir: &Path) -> Result<Self> {
        let path = work_dir.join(FILE_NAME);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text)
                .into_diagnostic()
                .wrap_err_with(|| format!("failed to parse `{}`", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e)
                .into_diagnostic()
                .wrap_err_with(|| format!("failed to read `{}`", path.display())),
        }
    }
}

/// A language the congregation declared, `name#version` already split.
#[derive(Debug, Clone)]
pub struct DeclaredLang {
    pub language: String,
    pub lang_version: String,
}

/// CLI flag overrides for `comline generate`.
#[derive(Debug, Default)]
pub struct Overrides<'a> {
    pub target: Option<&'a str>,
    pub out: Option<&'a str>,
    pub layout: Option<&'a str>,
    pub mode: Option<&'a str>,
    /// `Some(true)` from `--external-std`, `Some(false)` from
    /// `--no-external-std`, `None` when neither was passed. Both spellings
    /// exist because the default is no longer a fixed `false` - it follows
    /// the mode (see [`default_external_std`]), so either direction can be
    /// the one a given run needs to override.
    pub external_std: Option<bool>,
    /// `--validate-on-decode`.
    pub validate_on_decode: Option<&'a str>,
}

/// A fully-resolved target: every field decided, `out` made absolute.
///
/// Its `layout` is rendered into a path by the CLI, which has core's
/// template engine.
#[derive(Debug)]
pub struct ResolvedTarget {
    pub language: String,
    /// See [`Target::generator`], made absolute.
    pub generator: Option<PathBuf>,
    /// See [`Target::timeout`].
    pub timeout: std::time::Duration,
    pub lang_version: String,
    pub out: PathBuf,
    pub layout: String,
    pub mode: String,
    pub versions: VersionSpec,
    /// `lib` mode only — see [`Target::flatten`].
    pub flatten: bool,
    /// See [`Generate::external_std`].
    pub external_std: bool,
    /// See [`Generate::validate_on_decode`].
    pub validate_on_decode: ValidateOnDecode,
    /// See [`Generate::async_api`].
    pub async_api: bool,
    /// [`Generate::encodings`] with the target's over them, by canonical
    /// encoding name.
    pub encodings: std::collections::BTreeMap<String, crate::protocol::EncodingMapping>,
}

/// Merge `comline.toml`, the congregation's declared languages and CLI flags
/// into the concrete set of targets `generate` should emit.
///
/// **The declared languages are the set.** `config.idp`'s
/// `code_generation.languages` says which languages this package is generated
/// for - that is the package's own contract, committed alongside it - and a
/// `[[generate.target]]` block says where and how one of them lands. A block
/// used to *replace* the list outright, so adding one to redirect `rust`'s
/// output silently stopped generating every other declared language; the
/// docs described the block as settings all along ("one block per target you
/// want to pin or override"), and this is the code catching up.
///
/// A language with no block takes the `[generate]` defaults. Several blocks
/// for one language give several targets - two `rust` blocks writing to
/// different `out` is a real thing to want. A block naming a language the
/// congregation doesn't declare is an error, not a silent no-op: that would
/// be the same footgun inverted.
///
/// With no declared languages at all, the blocks are the set - a
/// `comline.toml`-only project with no `code_generation` in its congregation
/// still works.
///
/// `[generate] only` narrows the declared set persistently; `--target`
/// narrows it for one run (case-insensitive). The `--out` / `--layout` /
/// `--mode` flags apply to the single remaining target, or to the
/// `--target`-named one; using them with several targets and no `--target` is an
/// error. `COMLINE_GENERATE_{OUT,LAYOUT,MODE}` sit just below the flags but apply
/// to every target regardless (a deliberate global for CI).
pub fn resolve(
    cfg: &ComlineToml,
    declared: &[DeclaredLang],
    work_dir: &Path,
    ov: &Overrides,
) -> Result<Vec<ResolvedTarget>> {
    let moved = std::iter::once(&cfg.generate.default_framing)
        .chain(cfg.generate.targets.iter().map(|t| &t.default_framing));
    if let Some(written) = moved.flatten().next() {
        return Err(moved_default_framing(written));
    }

    if let Some(t) = cfg.generate.targets.iter().find(|t| t.language.is_empty()) {
        return Err(match &t.generator {
            Some(g) => miette!("`[[generate.target]]` with `generator = \"{g}\"` needs `language`"),
            None => miette!("a `[[generate.target]]` sets no `language`"),
        });
    }

    let declares = |language: &str| {
        declared
            .iter()
            .any(|d| d.language.eq_ignore_ascii_case(language))
    };

    // A block for a language nobody declared is a mistake worth naming -
    // it would otherwise configure a target that is never built.
    if !declared.is_empty() {
        if let Some(stray) = cfg.generate.targets.iter().find(|t| !declares(&t.language)) {
            return Err(miette!(
                "`[[generate.target]]` names `{}`, which config.idp does not declare \
                 - add `{}#<version> = {{}}` to `code_generation.languages`, or drop \
                 the block",
                stray.language,
                stray.language,
            ));
        }
    }
    if let Some(stray) = cfg
        .generate
        .only
        .iter()
        .find(|l| !declared.is_empty() && !declares(l))
    {
        return Err(miette!(
            "`[generate] only` names `{stray}`, which config.idp does not declare",
        ));
    }

    let base: Vec<Target> = if declared.is_empty() {
        // No congregation-declared languages: the blocks are all there is.
        cfg.generate.targets.clone()
    } else {
        declared
            .iter()
            .filter(|d| {
                cfg.generate.only.is_empty()
                    || cfg
                        .generate
                        .only
                        .iter()
                        .any(|l| l.eq_ignore_ascii_case(&d.language))
            })
            .flat_map(|d| {
                let blocks: Vec<Target> = cfg
                    .generate
                    .targets
                    .iter()
                    .filter(|t| t.language.eq_ignore_ascii_case(&d.language))
                    .cloned()
                    .collect();
                if blocks.is_empty() {
                    // Declared but unconfigured: the `[generate]` defaults.
                    vec![Target {
                        language: d.language.clone(),
                        generator: None,
                        timeout: None,
                        lang_version: Some(d.lang_version.clone()),
                        out: None,
                        layout: None,
                        mode: None,
                        package_versions: None,
                        default_framing: None,
                        external_std: None,
                        validate_on_decode: None,
                        async_api: None,
                        encodings: Default::default(),
                        flatten: false,
                    }]
                } else {
                    blocks
                }
            })
            .collect()
    };

    let selected: Vec<&Target> = base
        .iter()
        .filter(|t| match ov.target {
            Some(want) => want.eq_ignore_ascii_case(&t.language),
            None => true,
        })
        .collect();

    if selected.is_empty() {
        return match ov.target {
            Some(want) => Err(miette!("no code-generation target matches `{want}`")),
            None if !cfg.generate.only.is_empty() => {
                Err(miette!("`[generate] only` leaves nothing to generate",))
            }
            None => Err(miette!(
                "nothing to generate: add `[[generate.target]]` to {} \
                 or `code_generation.languages` to config.idp",
                FILE_NAME
            )),
        };
    }

    let field_override = ov.out.is_some()
        || ov.layout.is_some()
        || ov.mode.is_some()
        || ov.external_std.is_some()
        || ov.validate_on_decode.is_some();
    if field_override && selected.len() > 1 && ov.target.is_none() {
        return Err(miette!(
            "`--out` / `--layout` / `--mode` / `--external-std` / `--validate-on-decode` \
             need `--target <lang>` when more than one target is configured ({} here)",
            selected.len()
        ));
    }
    // A flag binds to a target when it was explicitly named, or when it is the
    // only one left. `COMLINE_*` env vars are a deliberate global — they apply to
    // every target regardless.
    let apply_flag = ov.target.is_some() || selected.len() == 1;
    let env_out = env_var("COMLINE_GENERATE_OUT");
    let env_layout = env_var("COMLINE_GENERATE_LAYOUT");
    let env_mode = env_var("COMLINE_GENERATE_MODE");
    let env_external_std = env_var_bool("COMLINE_GENERATE_EXTERNAL_STD");
    let env_validate_on_decode = env_var("COMLINE_GENERATE_VALIDATE_ON_DECODE");

    let mut resolved = Vec::with_capacity(selected.len());
    for t in selected {
        // Precedence, highest first: flag → env → [[target]] → [generate] → default.
        let pick = |flag: Option<&str>,
                    env: &Option<String>,
                    target: &Option<String>,
                    section: &Option<String>,
                    default: &str| {
            flag.filter(|_| apply_flag)
                .map(str::to_owned)
                .or_else(|| env.clone())
                .or_else(|| target.clone())
                .or_else(|| section.clone())
                .unwrap_or_else(|| default.to_owned())
        };

        // Same precedence, for a bool field.
        let pick_bool = |flag: Option<bool>,
                         env: Option<bool>,
                         target: Option<bool>,
                         section: Option<bool>,
                         default: bool| {
            flag.filter(|_| apply_flag)
                .or(env)
                .or(target)
                .or(section)
                .unwrap_or(default)
        };

        let out = pick(ov.out, &env_out, &t.out, &cfg.generate.out, DEFAULT_OUT);
        let layout = pick(
            ov.layout,
            &env_layout,
            &t.layout,
            &cfg.generate.layout,
            DEFAULT_LAYOUT,
        );
        let mode = pick(
            ov.mode,
            &env_mode,
            &t.mode,
            &cfg.generate.mode,
            DEFAULT_MODE,
        );
        let external_std = pick_bool(
            ov.external_std,
            env_external_std,
            t.external_std,
            cfg.generate.external_std,
            default_external_std(&mode),
        );
        let validate_on_decode = pick(
            ov.validate_on_decode,
            &env_validate_on_decode,
            &t.validate_on_decode,
            &cfg.generate.validate_on_decode,
            ValidateOnDecode::Off.name(),
        );
        let validate_on_decode = ValidateOnDecode::parse(&validate_on_decode).ok_or_else(|| {
            miette!(
                "`validate_on_decode = \"{validate_on_decode}\"` for target `{}` - expected one of \
                 {}",
                t.language,
                ValidateOnDecode::NAMES.map(|n| format!("`{n}`")).join(", ")
            )
        })?;

        let declared_version = declared
            .iter()
            .find(|d| d.language.eq_ignore_ascii_case(&t.language))
            .map(|d| d.lang_version.clone());
        // `comline.toml` may raise the version, never lower it: config.idp's
        // is the oldest toolchain the package was proven against. A version
        // that doesn't parse is left for the registry to name.
        if let (Some(raised), Some(declared_version)) = (&t.lang_version, &declared_version) {
            if let (Some(r), Some(d)) = (
                LangVersion::parse(raised),
                LangVersion::parse(declared_version),
            ) {
                if r < d {
                    return Err(miette!(
                        "target `{}` sets `lang_version = \"{raised}\"`, below config.idp's `{}#{declared_version}` \
                         - {} can raise a target's version, never lower it: the package is only proven \
                         from `{declared_version}` on",
                        t.language,
                        t.language,
                        FILE_NAME
                    ));
                }
            }
        }
        let lang_version = t.lang_version.clone().or(declared_version).ok_or_else(|| {
            miette!(
                "target `{}` sets no `lang_version` and config.idp does not \
                     declare that language under `code_generation.languages`",
                t.language
            )
        })?;

        let versions = t
            .package_versions
            .clone()
            .or_else(|| cfg.generate.package_versions.clone())
            .unwrap_or_default();

        if t.timeout == Some(0) {
            return Err(miette!(
                "target `{}`: `timeout = 0` - it's in seconds, and must be at least 1",
                t.language
            ));
        }
        if t.timeout.is_some() && t.generator.is_none() {
            return Err(miette!(
                "target `{}`: `timeout` limits an external `generator`, and this target uses the \
                 built-in one",
                t.language
            ));
        }
        resolved.push(ResolvedTarget {
            language: t.language.clone(),
            generator: t.generator.as_ref().map(|g| work_dir.join(g)),
            timeout: t
                .timeout
                .map(std::time::Duration::from_secs)
                .unwrap_or(DEFAULT_TIMEOUT),
            lang_version,
            out: work_dir.join(out),
            layout,
            mode,
            versions,
            flatten: t.flatten,
            external_std,
            validate_on_decode,
            async_api: t.async_api.or(cfg.generate.async_api).unwrap_or(false),
            encodings: merge_encodings(&cfg.generate.encodings, &t.encodings),
        });
    }

    Ok(resolved)
}

/// `default_framing` left `comline.toml`: the framing is part of the schema,
/// which both ends of a connection must agree on, so the package declares it.
fn moved_default_framing(written: &str) -> miette::Report {
    let framing = annotations::canonical_value("framing", written).unwrap_or(written);
    miette!(
        "`default_framing` moved from {FILE_NAME} to config.idp\n  \
         help: drop it here, and declare the package's framing in config.idp:\n      \
         wire = {{ framing = \"{framing}\" }}\n  \
         Protocols with no `@framing` default to `datagram@2` without it."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml_src: &str) -> ComlineToml {
        toml::from_str(toml_src).unwrap()
    }

    #[test]
    fn version_spec_forms() {
        assert_eq!(
            parse("[generate]\npackage_versions = \"latest\"")
                .generate
                .package_versions,
            Some(VersionSpec::Latest)
        );
        assert_eq!(
            parse("[generate]\npackage_versions = \"ALL\"")
                .generate
                .package_versions,
            Some(VersionSpec::All)
        );
        assert_eq!(
            parse("[generate]\npackage_versions = \"0.3.0\"")
                .generate
                .package_versions,
            Some(VersionSpec::List(vec!["0.3.0".into()]))
        );
        assert_eq!(
            parse("[generate]\npackage_versions = [\"0.3.0\", \"0.4.0\"]")
                .generate
                .package_versions,
            Some(VersionSpec::List(vec!["0.3.0".into(), "0.4.0".into()]))
        );
        assert_eq!(parse("[generate]").generate.package_versions, None);
    }

    #[test]
    fn target_package_versions_wins_over_section() {
        let cfg = parse(
            "[generate]\npackage_versions = \"all\"\n\n\
             [[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.85.0\"\n\
             package_versions = \"latest\"\n",
        );
        let declared = [];
        let resolved = resolve(&cfg, &declared, Path::new("/x"), &Overrides::default()).unwrap();
        assert_eq!(resolved[0].versions, VersionSpec::Latest);
    }

    fn two_langs() -> [DeclaredLang; 2] {
        [
            DeclaredLang {
                language: "rust".into(),
                lang_version: "1.85.0".into(),
            },
            DeclaredLang {
                language: "typescript".into(),
                lang_version: "5.0".into(),
            },
        ]
    }

    /// The bug this rule replaced: one block existed only to redirect
    /// `rust`'s output, and every other declared language silently stopped
    /// being generated.
    #[test]
    fn a_block_configures_its_language_without_dropping_the_others() {
        let cfg = parse("[[generate.target]]\nlanguage = \"rust\"\nout = \"../test-cl-rs/\"\n");
        let resolved = resolve(&cfg, &two_langs(), Path::new("/x"), &Overrides::default()).unwrap();

        let langs: Vec<&str> = resolved.iter().map(|t| t.language.as_str()).collect();
        assert_eq!(langs, vec!["rust", "typescript"]);
        assert_eq!(
            resolved[0].out,
            Path::new("/x/../test-cl-rs/"),
            "the block's out"
        );
        assert_eq!(
            resolved[1].out,
            Path::new("/x/generated"),
            "the [generate] default"
        );
        // A language with no block still gets the version its congregation declared.
        assert_eq!(resolved[1].lang_version, "5.0");
    }

    #[test]
    fn a_block_may_be_matched_case_insensitively() {
        let cfg = parse("[[generate.target]]\nlanguage = \"Rust\"\nout = \"elsewhere\"\n");
        let resolved = resolve(&cfg, &two_langs(), Path::new("/x"), &Overrides::default()).unwrap();
        assert_eq!(resolved[0].out, Path::new("/x/elsewhere"));
    }

    /// Two outputs for one language is a real thing to want, and survives
    /// the declared set becoming the base.
    #[test]
    fn several_blocks_for_one_language_give_several_targets() {
        let cfg = parse(
            "[[generate.target]]\nlanguage = \"rust\"\nout = \"a\"\n\n\
             [[generate.target]]\nlanguage = \"rust\"\nout = \"b\"\n",
        );
        let declared = [DeclaredLang {
            language: "rust".into(),
            lang_version: "1.85.0".into(),
        }];
        let resolved = resolve(&cfg, &declared, Path::new("/x"), &Overrides::default()).unwrap();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].out, Path::new("/x/a"));
        assert_eq!(resolved[1].out, Path::new("/x/b"));
    }

    /// Silently ignoring a block someone wrote would be the same footgun
    /// inverted, so it names the fix instead.
    #[test]
    fn a_block_for_an_undeclared_language_is_an_error() {
        let cfg = parse("[[generate.target]]\nlanguage = \"python\"\nlang_version = \"3.11\"\n");
        let declared = [DeclaredLang {
            language: "rust".into(),
            lang_version: "1.85.0".into(),
        }];
        let err = resolve(&cfg, &declared, Path::new("/x"), &Overrides::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("config.idp does not declare"), "{err}");
        assert!(err.contains("code_generation.languages"), "{err}");
    }

    /// A congregation with no `code_generation` at all: the blocks are all
    /// there is, as they always were.
    #[test]
    fn blocks_stand_alone_when_nothing_is_declared() {
        let cfg = parse("[[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.85.0\"\n");
        let resolved = resolve(&cfg, &[], Path::new("/x"), &Overrides::default()).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].language, "rust");
    }

    #[test]
    fn the_doc_section_parses_and_refuses_unknown_keys() {
        let cfg = parse(
            "[doc]\nout = \"site/api\"\nformats = [\"json\", \"html\"]\nrenderer = \"r.wasm\"\ntimeout = 5\n\
             changelog_from = \"0.1.0\"\nlinks = { std = \"https://x/std\" }\nversions = { last = 3 }\n",
        );
        assert_eq!(cfg.doc.out.as_deref(), Some("site/api"));
        assert_eq!(cfg.doc.formats, Some(vec!["json".to_string(), "html".to_string()]));
        assert_eq!(cfg.doc.versions, Some(DocVersions::Select(DocVersionSelect { last: Some(3), ..Default::default() })));
        assert_eq!(parse("[doc]\nversions = \"all\"\n").doc.versions, Some(DocVersions::All(AllVersions::All)));
        assert_eq!(
            parse("[doc]\nversions = { since = \"0.3.0\", releases_only = true }\n").doc.versions,
            Some(DocVersions::Select(DocVersionSelect {
                since: Some("0.3.0".into()),
                releases_only: true,
                ..Default::default()
            }))
        );
        assert!(toml::from_str::<ComlineToml>("[doc]\nversions = \"some\"\n").is_err());
        assert!(toml::from_str::<ComlineToml>("[doc]\nversions = { first = 2 }\n").is_err());
        assert!(toml::from_str::<ComlineToml>("[doc]\nformat = \"json\"\n").is_err());
        assert_eq!(cfg.doc.renderer.as_deref(), Some("r.wasm"));
        assert_eq!(cfg.doc.timeout, Some(5));
        assert_eq!(cfg.doc.changelog_from.as_deref(), Some("0.1.0"));
        assert_eq!(cfg.doc.links["std"], "https://x/std");
        assert!(parse("").doc.out.is_none());
        assert!(toml::from_str::<ComlineToml>("[doc]\nouts = \"x\"\n").is_err());
    }

    #[test]
    fn only_narrows_the_declared_set() {
        let cfg = parse("[generate]\nonly = [\"rust\"]\n");
        let resolved = resolve(&cfg, &two_langs(), Path::new("/x"), &Overrides::default()).unwrap();
        let langs: Vec<&str> = resolved.iter().map(|t| t.language.as_str()).collect();
        assert_eq!(langs, vec!["rust"]);
    }

    /// A typo in `only` would otherwise quietly stop generating something.
    #[test]
    fn only_naming_an_undeclared_language_is_an_error() {
        let cfg = parse("[generate]\nonly = [\"rustt\"]\n");
        let err = resolve(&cfg, &two_langs(), Path::new("/x"), &Overrides::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("does not declare"), "{err}");
    }

    #[test]
    fn only_still_lets_a_block_configure_what_survives() {
        let cfg = parse(
            "[generate]\nonly = [\"rust\"]\n\n\
             [[generate.target]]\nlanguage = \"rust\"\nout = \"here\"\n\n\
             [[generate.target]]\nlanguage = \"typescript\"\nout = \"there\"\n",
        );
        let resolved = resolve(&cfg, &two_langs(), Path::new("/x"), &Overrides::default()).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].out, Path::new("/x/here"));
    }

    /// `lib` writes the package manifest, so the generator can add
    /// `comline_std` to it; `code` emits loose files with nowhere to put a
    /// dependency, so inlining stays the honest default there.
    #[test]
    fn external_std_follows_the_mode() {
        for (mode, expected) in [("code", false), ("lib", true)] {
            let cfg = parse(&format!(
                "[[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.85.0\"\n\
                 mode = \"{mode}\"\n",
            ));
            let resolved = resolve(&cfg, &[], Path::new("/x"), &Overrides::default()).unwrap();
            assert_eq!(resolved[0].external_std, expected, "mode {mode}");
        }
    }

    /// Following the mode is a *default*, not a rule: a crate that wants a
    /// self-contained copy of std can still say so.
    #[test]
    fn an_explicit_false_still_inlines_std_in_lib_mode() {
        let cfg = parse(
            "[[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.85.0\"\n\
             mode = \"lib\"\nexternal_std = false\n",
        );
        let resolved = resolve(&cfg, &[], Path::new("/x"), &Overrides::default()).unwrap();
        assert!(!resolved[0].external_std);
    }

    #[test]
    fn the_section_default_applies_to_a_code_target_too() {
        let cfg = parse(
            "[generate]\nexternal_std = true\n\n\
             [[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.85.0\"\n",
        );
        let resolved = resolve(&cfg, &[], Path::new("/x"), &Overrides::default()).unwrap();
        assert!(resolved[0].external_std, "`code`, but asked for explicitly");
    }

    /// Both flag spellings exist because the default is no longer a fixed
    /// `false` - either direction can be the one a run needs to override.
    #[test]
    fn either_flag_spelling_overrides_the_mode_default() {
        let cfg = parse(
            "[[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.85.0\"\n\
             mode = \"lib\"\n",
        );
        let off = Overrides {
            external_std: Some(false),
            ..Default::default()
        };
        let resolved = resolve(&cfg, &[], Path::new("/x"), &off).unwrap();
        assert!(!resolved[0].external_std, "--no-external-std");

        let cfg = parse("[[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.85.0\"\n");
        let on = Overrides {
            external_std: Some(true),
            ..Default::default()
        };
        let resolved = resolve(&cfg, &[], Path::new("/x"), &on).unwrap();
        assert!(resolved[0].external_std, "--external-std");
    }

    #[test]
    fn a_target_may_raise_the_declared_version_but_not_lower_it() {
        let declared = [DeclaredLang {
            language: "rust".into(),
            lang_version: "1.88.0".into(),
        }];
        let raised = parse("[[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.90\"\n");
        let resolved = resolve(&raised, &declared, Path::new("/x"), &Overrides::default()).unwrap();
        assert_eq!(resolved[0].lang_version, "1.90");

        let lowered =
            parse("[[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.85.0\"\n");
        let err = resolve(&lowered, &declared, Path::new("/x"), &Overrides::default()).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("below config.idp's `rust#1.88.0`"),
            "{message}"
        );
        assert!(message.contains("never lower it"), "{message}");
    }

    #[test]
    fn flatten_defaults_to_false_and_is_target_only() {
        let cfg = parse(
            "[[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.85.0\"\n\
             mode = \"lib\"\nflatten = true\n",
        );
        let declared = [];
        let resolved = resolve(&cfg, &declared, Path::new("/x"), &Overrides::default()).unwrap();
        assert!(resolved[0].flatten);

        let cfg = parse(
            "[[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.85.0\"\nmode = \"lib\"\n",
        );
        let resolved = resolve(&cfg, &declared, Path::new("/x"), &Overrides::default()).unwrap();
        assert!(!resolved[0].flatten);
    }

    #[test]
    fn async_is_off_unless_asked_for_and_a_target_overrides_the_section() {
        let declared = [DeclaredLang {
            language: "rust".into(),
            lang_version: "1.85.0".into(),
        }];
        let off = resolve(
            &parse("[generate]\n"),
            &declared,
            Path::new("/x"),
            &Overrides::default(),
        )
        .unwrap();
        assert!(!off[0].async_api);

        let on = resolve(
            &parse("[generate]\nasync = true\n"),
            &declared,
            Path::new("/x"),
            &Overrides::default(),
        )
        .unwrap();
        assert!(on[0].async_api);

        let overridden = parse(
            "[generate]\nasync = true\n\n[[generate.target]]\nlanguage = \"rust\"\nasync = false\n",
        );
        let resolved = resolve(
            &overridden,
            &declared,
            Path::new("/x"),
            &Overrides::default(),
        )
        .unwrap();
        assert!(!resolved[0].async_api);
    }

    #[test]
    fn external_std_defaults_to_false() {
        let cfg = parse("[generate]\n");
        let declared = [DeclaredLang {
            language: "rust".into(),
            lang_version: "1.80".into(),
        }];
        let resolved = resolve(&cfg, &declared, Path::new("/x"), &Overrides::default()).unwrap();
        assert!(!resolved[0].external_std);
    }

    #[test]
    fn external_std_section_applies_to_every_target() {
        let cfg = parse("[generate]\nexternal_std = true\n");
        let declared = [DeclaredLang {
            language: "rust".into(),
            lang_version: "1.80".into(),
        }];
        let resolved = resolve(&cfg, &declared, Path::new("/x"), &Overrides::default()).unwrap();
        assert!(resolved[0].external_std);
    }

    #[test]
    fn target_external_std_wins_over_section() {
        let cfg = parse(
            "[generate]\nexternal_std = true\n\n\
             [[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.85.0\"\n\
             external_std = false\n",
        );
        let declared = [];
        let resolved = resolve(&cfg, &declared, Path::new("/x"), &Overrides::default()).unwrap();
        assert!(!resolved[0].external_std);
    }

    #[test]
    fn flag_wins_over_everything_for_external_std() {
        let cfg = parse("[generate]\nexternal_std = false\n");
        let declared = [DeclaredLang {
            language: "rust".into(),
            lang_version: "1.80".into(),
        }];
        let ov = Overrides {
            external_std: Some(true),
            ..Default::default()
        };
        let resolved = resolve(&cfg, &declared, Path::new("/x"), &ov).unwrap();
        assert!(resolved[0].external_std);
    }

    #[test]
    fn validate_on_decode_defaults_to_off() {
        let cfg = parse("[[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.85.0\"\n");
        let resolved = resolve(&cfg, &[], Path::new("/x"), &Overrides::default()).unwrap();
        assert_eq!(resolved[0].validate_on_decode, ValidateOnDecode::Off);
    }

    #[test]
    fn validate_on_decode_target_wins_over_section_and_the_flag_over_both() {
        let cfg = parse(
            "[generate]\nvalidate_on_decode = \"collect\"\n\n\
             [[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.85.0\"\n\
             validate_on_decode = \"first\"\n",
        );
        let resolved = resolve(&cfg, &[], Path::new("/x"), &Overrides::default()).unwrap();
        assert_eq!(resolved[0].validate_on_decode, ValidateOnDecode::First);

        let ov = Overrides {
            validate_on_decode: Some("off"),
            ..Default::default()
        };
        let resolved = resolve(&cfg, &[], Path::new("/x"), &ov).unwrap();
        assert_eq!(resolved[0].validate_on_decode, ValidateOnDecode::Off);
    }

    #[test]
    fn default_framing_says_it_moved_to_config_idp() {
        let cfg = parse("[generate]\ndefault_framing = \"json-rpc\"\n");
        let declared = [DeclaredLang {
            language: "rust".into(),
            lang_version: "1.80".into(),
        }];
        let error = resolve(&cfg, &declared, Path::new("/x"), &Overrides::default()).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("moved from comline.toml to config.idp"),
            "{message}"
        );
        // The hint is ready to paste, in the canonical spelling.
        assert!(
            message.contains("wire = { framing = \"jsonrpc\" }"),
            "{message}"
        );
    }

    #[test]
    fn a_target_default_framing_says_it_moved_too() {
        let cfg = parse(
            "[[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.85.0\"\n\
             default_framing = \"datagram\"\n",
        );
        let error = resolve(&cfg, &[], Path::new("/x"), &Overrides::default()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("wire = { framing = \"datagram\" }"),
            "{error}"
        );
    }

    #[test]
    fn an_unknown_validate_on_decode_names_the_accepted_values() {
        let cfg = parse(
            "[generate]\nvalidate_on_decode = \"always\"\n\n\
                         [[generate.target]]\nlanguage = \"rust\"\nlang_version = \"1.85.0\"\n",
        );
        let error = resolve(&cfg, &[], Path::new("/x"), &Overrides::default()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "`validate_on_decode = \"always\"` for target `rust` - expected one of `off`, `collect`, `first`"
        );
    }

    #[test]
    fn a_targets_encodings_override_the_sections_by_canonical_name() {
        let cfg: ComlineToml = toml::from_str(
            "[generate]\n\
             encodings = { \"jsonrpc@1\" = \"section::Rpc\", \"com.acme.cbor@1\" = \"section::Cbor\" }\n\
             [[generate.target]]\n\
             language = \"rust\"\n\
             encodings = { \"json-rpc\" = { type = \"mine::Rpc\", package = \"mine\", version = \"1\" } }\n",
        )
        .unwrap();
        let merged = merge_encodings(&cfg.generate.encodings, &cfg.generate.targets[0].encodings);
        let keys: Vec<&str> = merged.keys().map(String::as_str).collect();
        assert_eq!(keys, ["com.acme.cbor@1", "jsonrpc"]);
        assert_eq!(merged["jsonrpc"].type_path, "mine::Rpc");
        assert_eq!(merged["jsonrpc"].package.as_deref(), Some("mine"));
        assert_eq!(merged["com.acme.cbor@1"].type_path, "section::Cbor");
        assert_eq!(merged["com.acme.cbor@1"].package, None);
    }
}
