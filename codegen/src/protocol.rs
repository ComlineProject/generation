//! The generator protocol: what a generator receives and answers, for the
//! built-ins linked into the CLI and for a generator running as its own
//! process alike.
//!
//! An external generator reads one CBOR-encoded [`Request`] from stdin and
//! writes one CBOR-encoded [`Response`] to stdout. CBOR here is the same
//! encoding `comline-core` stores its IR with (`.comline/`'s CAS), so the
//! [`FrozenUnit`]s in a request are exactly what a build froze. A Rust
//! generator binary is [`serve_stdio`] over its [`Registry`].
//!
//! The same programs can render documentation: a [`RenderRequest`] carries
//! a package's documentation model (`comline_core::doc`, as JSON) and the
//! renderer's options, and the answer is files, like [`GenerateRequest`]'s.
//! [`Description::renderers`] lists the renderers a program offers.
//!
//! **Several versions.** `comline doc` documents every version of a package
//! (generation#101) with one `Render` call per version, then one for the
//! site's index. A per-version call sets [`RenderOptions::version`], the
//! folder it lands in, so its pages can reach the site's shared files at
//! `../`; it carries that one version's model, exactly like a single-version
//! render, so a renderer that ignores the option still works. The index call
//! sets [`RenderOptions::versions`] (every version, each with its model, and
//! the changelog from the one before it) and is only sent to a renderer whose
//! [`RendererInfo::sites`] is true. Both are additive: the protocol version
//! stays.
//!
//! A [`Request`] states the protocol version and the IR format it was built
//! with; a generator refuses either one it doesn't accept, and says so in a
//! [`Response::Error`] naming both sides. Both start their header fields
//! first, and [`handle`] reads them before the body, so a mismatch is
//! reported even when the body has a shape this side doesn't know.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::ExitCode;

use comline_core::package::build::cas::object_store::CURRENT_IR_FORMAT_VERSION;
use comline_core::package::build::cas::storage::{from_cbor, to_cbor};
use comline_core::schema::ir::frozen::unit::FrozenUnit;
use eyre::{eyre, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::features::{Support, SupportTable};
use crate::validation::ValidateOnDecode;
use crate::versions::{LangVersion, Versions};
use crate::{GeneratedFile, Generator, Mode, PackageMeta, Registry, Renderer};

/// This protocol's version. `0` while Comline is in its early days: until
/// 1.0 it may change incompatibly, and the number moves when it does.
///
/// An additive change keeps it: a new field with a default, or a new request
/// a program is only sent once its [`Description`] says it handles it
/// (`Render`, which only goes to a program listing renderers).
pub const PROTOCOL_VERSION: u32 = 0;

/// The IR format this build of `comline-core` freezes and reads: the
/// `.comline/` stamp. A request carries it in [`Request::ir_format`].
pub const IR_FORMAT: u32 = CURRENT_IR_FORMAT_VERSION;

/// What the CLI sends a generator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// The protocol version the sender speaks: [`PROTOCOL_VERSION`].
    pub protocol: u32,
    /// The IR format of every [`FrozenUnit`] in the body: [`IR_FORMAT`].
    pub ir_format: u32,
    pub body: RequestBody,
}

impl Request {
    /// `body`, stamped with this build's protocol and IR format.
    pub fn new(body: RequestBody) -> Self {
        Request { protocol: PROTOCOL_VERSION, ir_format: IR_FORMAT, body }
    }

    pub fn describe() -> Self {
        Self::new(RequestBody::Describe)
    }

    pub fn generate(request: GenerateRequest) -> Self {
        Self::new(RequestBody::Generate(request))
    }

    pub fn render(request: RenderRequest) -> Self {
        Self::new(RequestBody::Render(request))
    }
}

/// What a [`Request`] asks for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RequestBody {
    /// Which languages, versions, protocol versions and IR formats the
    /// generator handles: [`Response::Describe`].
    Describe,
    /// Generate one target: [`Response::Generate`].
    Generate(GenerateRequest),
    /// Render a package's documentation: [`Response::Render`]. Only sent to
    /// a program whose [`Description::renderers`] lists the renderer.
    Render(RenderRequest),
}

/// Render a package's documentation with one renderer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderRequest {
    /// The renderer's id, from [`Description::renderers`]: `markdown`.
    pub renderer: String,
    /// The model's version (`comline_core::doc::DOC_MODEL_VERSION`), readable
    /// without parsing `model`: a renderer refuses one it doesn't list.
    pub model_version: u32,
    /// The documentation model as JSON text, in the shape of its JSON
    /// Schema (`schemas/doc-model.json` in core).
    pub model: String,
    pub options: RenderOptions,
}

/// What a renderer is told besides the model. Every renderer accepts every
/// option and ignores the ones it has no use for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderOptions {
    /// Where each dependency's documentation lives: a dependency's name (as
    /// the package names it, or `std`) to the base URL of its rendered docs.
    /// A type from a dependency listed here links to it; one that isn't is
    /// plain text.
    #[serde(default)]
    pub links: BTreeMap<String, String>,
    /// Set when this render is one version of a multi-version site: the
    /// version's folder name (`0.2.0`, `0.3.0-dev.2`, `unreleased`). The
    /// output lands in `<site>/<version>/`, so the site's shared files are
    /// at `../` (for the HTML renderer, `../versions.js` feeds the version
    /// picker). Unset, the render is a site of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Set for a site's index render: every documented version, oldest
    /// first. The output lands at the site's root, next to the version
    /// folders. The request's own `model` is the newest version's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<SiteVersion>,
    /// Plain-text notes for every page of this version: what its
    /// documentation lacks (a version built before headers were frozen has
    /// none, and built versions have no wire tables yet).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// One version of a multi-version site, for its index render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteVersion {
    /// Its folder name: the full version (`0.2.0`, `0.3.0-dev.2`), or
    /// `unreleased` for the working tree's changes since the last build.
    pub id: String,
    /// The release it belongs to: itself for a release, the release a dev
    /// build leads to (`0.3.0` for `0.3.0-dev.2`), `unreleased` for the
    /// working tree. A picker groups by it.
    pub release: String,
    pub kind: VersionKind,
    /// Its documentation model as JSON, like [`RenderRequest::model`]. Its
    /// `changelog` holds what changed since the version before it in the
    /// list (none for the first), computed by core's rules.
    pub model: String,
}

/// What kind of version a [`SiteVersion`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionKind {
    Release,
    Dev,
    Unreleased,
}

impl VersionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VersionKind::Release => "release",
            VersionKind::Dev => "dev",
            VersionKind::Unreleased => "unreleased",
        }
    }
}

/// The language and version a target declared: `rust#1.85.0` is
/// `{ language: "rust", version: "1.85.0" }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetLanguage {
    pub language: String,
    pub version: String,
}

/// Everything a generator needs for one target: every schema in the package
/// (namespace + IR), the mode, the package identity and the target's
/// options. Built-ins receive this directly, external generators inside a
/// [`Request`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerateRequest {
    pub mode: Mode,
    /// Every schema in the package: its namespace, and its frozen IR.
    pub schemas: Vec<(String, Vec<FrozenUnit>)>,
    pub package: PackageMeta,
    /// The language and version the target declared, which is also what
    /// selected this generator. A generator records the version in whatever
    /// its own ecosystem's manifest calls that (`rust-version` in a
    /// `Cargo.toml`), and ignores it otherwise - `Mode::Code` has no
    /// manifest to put it in.
    pub target: TargetLanguage,
    /// The package's wire format (`config.idp`'s `wire.format`), canonical:
    /// `packed@1` (the default), `json@1` or `msgpack@1`. What the generated
    /// code encodes with, except for JSON-RPC protocols, which carry JSON.
    ///
    /// There's no framing here: core writes the package's `wire.framing`
    /// into each protocol's IR, where [`crate::connections::framing`] reads
    /// it.
    pub wire_format: String,
    /// `comline.toml`'s `[generate] external_std` - `true` means `std`
    /// should be referenced as its own crate instead of inlined into this
    /// package's own generated output. Only a generator that knows how to
    /// do that (today, Rust's) acts on it; others ignore it.
    pub external_std: bool,
    /// `comline.toml`'s `[generate] validate_on_decode` - whether decoding a
    /// value runs its `@validators` checks. A generator that emits no checks
    /// ignores it.
    pub validate_on_decode: ValidateOnDecode,
    /// `comline.toml`'s `[generate] async` - whether protocols on a
    /// connection get async handlers and senders instead of blocking ones. A
    /// generator whose output is async anyway, or that has no connections,
    /// ignores it.
    pub async_api: bool,
    /// The implementation each encoding the package uses maps to in this
    /// target's language, by encoding name (`datagram@2`). Empty until
    /// encodings are declared in `config.idp` (core#120).
    #[serde(default)]
    pub encodings: BTreeMap<String, EncodingMapping>,
}

impl GenerateRequest {
    /// The target's version, parsed.
    pub fn version(&self) -> Result<LangVersion> {
        LangVersion::parse(&self.target.version).ok_or_else(|| {
            eyre!("`{}` isn't a version: expected dotted numbers, like `1.70.0`", self.target.version)
        })
    }

    /// Whether the target's version is at least `boundary`, one of the
    /// generator's own [`Versions`] boundaries: how a generator picks output
    /// that needs a newer toolchain. An unparsable version counts as the
    /// floor; [`crate::features::refuse_unsupported`] has already refused it.
    pub fn at_least(&self, boundary: &str) -> bool {
        match (LangVersion::parse(&self.target.version), LangVersion::parse(boundary)) {
            (Some(version), Some(boundary)) => version >= boundary,
            _ => false,
        }
    }
}

/// Where one encoding's implementation lives in the target's language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncodingMapping {
    /// The type implementing it: `comline_runtime::framing::Datagram2Framing`.
    pub type_path: String,
    /// The package it comes from, for a `lib` build's manifest.
    #[serde(default)]
    pub package: Option<String>,
    /// That package's version requirement.
    #[serde(default)]
    pub version: Option<String>,
}

/// What a generator answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    Describe(Description),
    Generate(Generated),
    /// The rendered documentation, as files.
    Render(Generated),
    /// The request was refused or generation failed.
    Error { message: String },
}

/// A generator's answer to [`RequestBody::Describe`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Description {
    /// One entry per generator: a language with several floors has several.
    pub languages: Vec<LanguageInfo>,
    /// The protocol versions it accepts, `(min, max)` inclusive.
    pub protocols: (u32, u32),
    /// The IR formats it accepts.
    pub ir_formats: Vec<u32>,
    /// The documentation renderers it offers; empty for a program that only
    /// generates code (and for one that predates renderers).
    #[serde(default)]
    pub renderers: Vec<RendererInfo>,
}

/// One documentation renderer, as [`Description`] lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendererInfo {
    /// Its id: `markdown`.
    pub id: String,
    /// The documentation model versions it reads.
    pub model_versions: Vec<u32>,
    /// Its version, part of `comline doc`'s cache key; empty when the
    /// program doesn't say.
    #[serde(default)]
    pub version: String,
    /// Whether it renders a multi-version site's index
    /// ([`RenderOptions::versions`]).
    #[serde(default)]
    pub sites: bool,
}

impl From<&Renderer> for RendererInfo {
    fn from(r: &Renderer) -> Self {
        RendererInfo {
            id: r.id.to_string(),
            model_versions: r.model_versions.to_vec(),
            version: r.version.to_string(),
            sites: r.sites,
        }
    }
}

/// One generator, as [`Description`] lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageInfo {
    /// `rust`, `typescript`: the name a congregation declares.
    pub id: String,
    /// The source file extension: `rs`, `ts`.
    pub ext: String,
    pub versions: VersionsInfo,
    /// Its support, one entry per feature it states.
    pub support: Vec<SupportInfo>,
}

/// [`Versions`], owned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionsInfo {
    pub floor: String,
    pub changes: Vec<String>,
    pub verified_up_to: String,
    pub forward_compatible: bool,
}

impl From<Versions> for VersionsInfo {
    fn from(v: Versions) -> Self {
        VersionsInfo {
            floor: v.floor.to_string(),
            changes: v.changes.iter().map(|c| c.to_string()).collect(),
            verified_up_to: v.verified_up_to.to_string(),
            forward_compatible: v.forward_compatible,
        }
    }
}

/// One [`SupportTable`] entry, owned, by the stable ids
/// ([`crate::features::Feature::id`], [`crate::features::State::id`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupportInfo {
    pub feature: String,
    pub state: String,
    pub note: Option<String>,
    pub tracking: Option<String>,
    /// `(version, why)`: the version support starts at, if not the floor.
    pub since: Option<(String, String)>,
}

impl SupportInfo {
    pub fn table(table: SupportTable) -> Vec<SupportInfo> {
        table.iter().map(|(feature, support)| SupportInfo::new(feature.id(), support)).collect()
    }

    fn new(feature: &str, s: &Support) -> Self {
        SupportInfo {
            feature: feature.to_string(),
            state: s.state.id().to_string(),
            note: s.note.map(str::to_string),
            tracking: s.tracking.map(str::to_string),
            since: s.since.map(|since| (since.version.to_string(), since.why.to_string())),
        }
    }
}

impl From<&Generator> for LanguageInfo {
    fn from(g: &Generator) -> Self {
        LanguageInfo {
            id: g.language.to_string(),
            ext: g.ext.to_string(),
            versions: g.versions.into(),
            support: SupportInfo::table(g.support),
        }
    }
}

/// A generator's answer to [`RequestBody::Generate`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Generated {
    pub files: Vec<FileOut>,
    /// Anything worth telling the user that didn't stop generation.
    pub diagnostics: Vec<String>,
}

/// One generated file, relative to the target's output root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOut {
    /// `/`-separated, relative, no `..`.
    pub path: String,
    pub contents: Vec<u8>,
}

impl From<GeneratedFile> for FileOut {
    fn from(f: GeneratedFile) -> Self {
        FileOut { path: path_string(&f.path), contents: f.contents.into_bytes() }
    }
}

/// `path` with `/` separators on every platform.
fn path_string(path: &std::path::Path) -> String {
    path.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

impl FileOut {
    /// The path, as a relative [`PathBuf`].
    pub fn path_buf(&self) -> PathBuf {
        self.path.split('/').collect()
    }
}

/// `value` as one CBOR document, the CAS's encoding.
pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>> {
    to_cbor(value)
}

/// One CBOR document from `bytes`.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    from_cbor(bytes)
}

/// A request's version fields alone, readable whatever its body is.
#[derive(Deserialize)]
struct Header {
    protocol: u32,
    ir_format: u32,
}

/// What [`Registry`] answers to [`RequestBody::Describe`].
pub fn describe(registry: &Registry) -> Description {
    Description {
        languages: registry.generators().map(LanguageInfo::from).collect(),
        protocols: (PROTOCOL_VERSION, PROTOCOL_VERSION),
        ir_formats: vec![IR_FORMAT],
        renderers: registry.renderers().map(RendererInfo::from).collect(),
    }
}

/// Answers one request with `registry`'s generators. Never panics: a request
/// that doesn't decode, a version mismatch, an unknown target, a failed or
/// panicking generator all come back as [`Response::Error`].
pub fn respond(registry: &Registry, request: &[u8]) -> Response {
    let header: Header = match decode(request) {
        Ok(header) => header,
        Err(e) => return error(format!("the request isn't a generator protocol request: {e}")),
    };
    if header.protocol != PROTOCOL_VERSION {
        return error(format!(
            "the request speaks generator protocol {}, and this generator speaks protocol {PROTOCOL_VERSION}",
            header.protocol
        ));
    }
    if header.ir_format != IR_FORMAT {
        return error(format!(
            "the request carries IR format {}, and this generator reads IR format {IR_FORMAT}",
            header.ir_format
        ));
    }
    let request: Request = match decode(request) {
        Ok(request) => request,
        Err(e) => return error(format!("the request doesn't decode: {e}")),
    };
    match request.body {
        RequestBody::Describe => Response::Describe(describe(registry)),
        RequestBody::Generate(req) => generate(registry, &req),
        RequestBody::Render(req) => render(registry, &req),
    }
}

fn render(registry: &Registry, req: &RenderRequest) -> Response {
    let Some(renderer) = registry.renderer(&req.renderer) else {
        let offered: Vec<&str> = registry.renderers().map(|r| r.id).collect();
        return error(format!(
            "no renderer `{}` here (offered: {})",
            req.renderer,
            if offered.is_empty() { "none".to_string() } else { offered.join(", ") }
        ));
    };
    if !renderer.model_versions.contains(&req.model_version) {
        return error(format!(
            "the request carries documentation model {}, and the `{}` renderer reads {:?}",
            req.model_version, renderer.id, renderer.model_versions
        ));
    }
    match catch_unwind(AssertUnwindSafe(|| (renderer.render)(req))) {
        Ok(Ok(files)) => {
            Response::Render(Generated { files: files.into_iter().map(FileOut::from).collect(), diagnostics: vec![] })
        }
        Ok(Err(e)) => error(format!("{e:#}")),
        Err(panic) => error(format!("the `{}` renderer panicked: {}", renderer.id, panic_message(&*panic))),
    }
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "no message".to_string())
}

fn generate(registry: &Registry, req: &GenerateRequest) -> Response {
    let resolved = match registry.resolve(&req.target.language, &req.target.version) {
        Ok(resolved) => resolved,
        Err(e) => return error(e.to_string()),
    };
    match catch_unwind(AssertUnwindSafe(|| (resolved.generator.generate)(req))) {
        Ok(Ok(files)) => {
            Response::Generate(Generated { files: files.into_iter().map(FileOut::from).collect(), diagnostics: vec![] })
        }
        Ok(Err(e)) => error(format!("{e:#}")),
        Err(panic) => error(format!("the {} generator panicked: {}", req.target.language, panic_message(&*panic))),
    }
}

fn error(message: String) -> Response {
    Response::Error { message }
}

/// [`respond`], encoded: request bytes in, response bytes out.
pub fn handle(registry: &Registry, request: &[u8]) -> Vec<u8> {
    let response = respond(registry, request);
    encode(&response)
        .or_else(|e| encode(&error(format!("the response doesn't encode: {e}"))))
        .unwrap_or_default()
}

/// A generator process's whole `main`: reads one [`Request`] from stdin,
/// answers it with `registry`, and writes the [`Response`] to stdout.
///
/// Exits successfully whenever a response was written, an error response
/// included; failing means stdin or stdout failed.
pub fn serve_stdio(registry: &Registry) -> ExitCode {
    let mut request = Vec::new();
    let response = match std::io::stdin().lock().read_to_end(&mut request) {
        Ok(_) => handle(registry, &request),
        Err(e) => encode(&error(format!("couldn't read the request from stdin: {e}"))).unwrap_or_default(),
    };
    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(&response).and_then(|()| stdout.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("couldn't write the response to stdout: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use comline_core::schema::ir::frozen::unit::ConfigValue;

    fn units() -> Vec<FrozenUnit> {
        vec![
            FrozenUnit::Namespace("demo".into()),
            FrozenUnit::Import("std::Str".into(), Some("S".into()), (3, 9)),
            FrozenUnit::Property { name: "framing".into(), value: ConfigValue::Str("datagram@2".into()) },
        ]
    }

    fn gen_request() -> GenerateRequest {
        GenerateRequest {
            mode: Mode::Lib,
            schemas: vec![("demo".into(), units()), ("other::nested".into(), vec![])],
            package: PackageMeta { name: "demo".into(), version: "1.2.3".into() },
            target: TargetLanguage { language: "toy".into(), version: "2.1".into() },
            wire_format: "packed@1".into(),
            external_std: true,
            validate_on_decode: ValidateOnDecode::Collect,
            async_api: true,
            encodings: BTreeMap::from([(
                "datagram@2".into(),
                EncodingMapping { type_path: "x::Datagram2".into(), package: Some("x".into()), version: None },
            )]),
        }
    }

    /// A generator whose output depends on every request field.
    fn toy(req: &GenerateRequest) -> Result<Vec<GeneratedFile>> {
        if req.package.name == "fail" {
            return Err(eyre!("refused: {}", req.package.name));
        }
        if req.package.name == "panic" {
            panic!("toy panicked");
        }
        let mut files: Vec<GeneratedFile> = req
            .schemas
            .iter()
            .map(|(ns, units)| GeneratedFile {
                path: ns.split("::").collect::<PathBuf>().with_extension("toy"),
                contents: format!("{units:?}"),
            })
            .collect();
        files.push(GeneratedFile {
            path: "manifest".into(),
            contents: format!(
                "{:?} {:?} {:?} {} {} {:?} {} {:?}",
                req.mode,
                req.package,
                req.target,
                req.wire_format,
                req.external_std,
                req.validate_on_decode,
                req.async_api,
                req.encodings
            ),
        });
        Ok(files)
    }

    const TOY_SUPPORT: SupportTable = &[(crate::features::Feature::Structs, Support::supported())];

    fn registry() -> Registry {
        let mut registry = Registry::new();
        registry.register(Generator {
            language: "toy",
            ext: "toy",
            generate: toy,
            support: TOY_SUPPORT,
            versions: Versions { floor: "2.0", changes: &[], verified_up_to: "2.0", forward_compatible: true },
        });
        registry.register_renderer(Renderer { id: "toydoc", model_versions: &[1], version: "1", sites: false, render: toydoc });
        registry
    }

    fn toydoc(req: &RenderRequest) -> Result<Vec<GeneratedFile>> {
        if req.model == "panic" {
            panic!("toydoc panicked");
        }
        Ok(vec![GeneratedFile { path: "index.md".into(), contents: format!("{} {:?}", req.model, req.options.links) }])
    }

    fn render_request(model: &str) -> RenderRequest {
        RenderRequest {
            renderer: "toydoc".into(),
            model_version: 1,
            model: model.into(),
            options: RenderOptions {
                links: BTreeMap::from([("std".into(), "https://x/std".into())]),
                ..Default::default()
            },
        }
    }

    #[test]
    fn render_answers_with_files() {
        round_trip(Request::render(render_request("{}")));
        round_trip(Response::Render(Generated { files: vec![], diagnostics: vec![] }));
        let Response::Render(out) = answer(&Request::render(render_request("{}"))) else { panic!() };
        assert_eq!(out.files, [FileOut { path: "index.md".into(), contents: b"{} {\"std\": \"https://x/std\"}".to_vec() }]);
    }

    #[test]
    fn render_refusals_are_errors() {
        let mut unknown = render_request("{}");
        unknown.renderer = "html".into();
        assert!(message(answer(&Request::render(unknown))).contains("no renderer `html` here (offered: toydoc)"));
        let mut newer = render_request("{}");
        newer.model_version = 9;
        assert!(message(answer(&Request::render(newer))).contains("documentation model 9"));
        assert!(message(answer(&Request::render(render_request("panic")))).contains("toydoc panicked"));
    }

    #[test]
    fn an_older_description_has_no_renderers() {
        #[derive(Serialize)]
        struct Old {
            languages: Vec<LanguageInfo>,
            protocols: (u32, u32),
            ir_formats: Vec<u32>,
        }
        let bytes = encode(&Old { languages: vec![], protocols: (0, 0), ir_formats: vec![3] }).unwrap();
        assert_eq!(decode::<Description>(&bytes).unwrap().renderers, vec![]);
    }

    fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: T) {
        let bytes = encode(&value).unwrap();
        assert_eq!(decode::<T>(&bytes).unwrap(), value);
    }

    #[test]
    fn requests_round_trip() {
        round_trip(Request::describe());
        round_trip(Request::generate(gen_request()));
        let mut bare = gen_request();
        bare.schemas.clear();
        bare.encodings.clear();
        bare.mode = Mode::Code;
        bare.validate_on_decode = ValidateOnDecode::Off;
        round_trip(Request::generate(bare));
    }

    #[test]
    fn responses_round_trip() {
        round_trip(Response::Describe(describe(&registry())));
        round_trip(Response::Describe(Description { languages: vec![], protocols: (0, 3), ir_formats: vec![], renderers: vec![] }));
        round_trip(Response::Generate(Generated {
            files: vec![FileOut { path: "a/b.rs".into(), contents: vec![0, 159, 255] }],
            diagnostics: vec!["careful".into()],
        }));
        round_trip(Response::Generate(Generated { files: vec![], diagnostics: vec![] }));
        round_trip(Response::Error { message: "no".into() });
    }

    #[test]
    fn encodings_default_to_empty() {
        #[derive(Serialize)]
        struct Old {
            mode: Mode,
            schemas: Vec<(String, Vec<FrozenUnit>)>,
            package: PackageMeta,
            target: TargetLanguage,
            wire_format: String,
            external_std: bool,
            validate_on_decode: ValidateOnDecode,
            async_api: bool,
        }
        let r = gen_request();
        let old = Old {
            mode: r.mode,
            schemas: r.schemas.clone(),
            package: r.package.clone(),
            target: r.target.clone(),
            wire_format: r.wire_format.clone(),
            external_std: r.external_std,
            validate_on_decode: r.validate_on_decode,
            async_api: r.async_api,
        };
        let decoded: GenerateRequest = decode(&encode(&old).unwrap()).unwrap();
        assert_eq!(decoded, GenerateRequest { encodings: BTreeMap::new(), ..r });
    }

    fn answer(request: &Request) -> Response {
        decode(&handle(&registry(), &encode(request).unwrap())).unwrap()
    }

    fn message(response: Response) -> String {
        match response {
            Response::Error { message } => message,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn a_protocol_mismatch_names_both_sides() {
        let request = Request { protocol: 7, ..Request::describe() };
        assert_eq!(
            message(answer(&request)),
            "the request speaks generator protocol 7, and this generator speaks protocol 0"
        );
    }

    #[test]
    fn an_ir_format_mismatch_names_both_sides() {
        let request = Request { ir_format: 999, ..Request::generate(gen_request()) };
        assert_eq!(
            message(answer(&request)),
            format!("the request carries IR format 999, and this generator reads IR format {IR_FORMAT}")
        );
    }

    #[test]
    fn a_mismatch_is_reported_before_an_unknown_body() {
        #[derive(Serialize)]
        struct Future {
            protocol: u32,
            ir_format: u32,
            body: (u8, String),
        }
        let bytes = encode(&Future { protocol: 1, ir_format: IR_FORMAT, body: (9, "?".into()) }).unwrap();
        let response: Response = decode(&handle(&registry(), &bytes)).unwrap();
        assert!(message(response).starts_with("the request speaks generator protocol 1,"));
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        for bytes in [&b""[..], b"\xff\x00garbage", b"\xa0"] {
            let response: Response = decode(&handle(&registry(), bytes)).unwrap();
            assert!(matches!(response, Response::Error { .. }), "{bytes:?}: {response:?}");
        }
    }

    #[test]
    fn describe_lists_the_registry() {
        let Response::Describe(d) = answer(&Request::describe()) else { panic!() };
        assert_eq!(d.protocols, (PROTOCOL_VERSION, PROTOCOL_VERSION));
        assert_eq!(d.ir_formats, vec![IR_FORMAT]);
        assert_eq!(d.languages.len(), 1);
        assert_eq!(d.languages[0].id, "toy");
        assert_eq!(d.languages[0].versions.floor, "2.0");
        assert_eq!(d.languages[0].support[0].feature, "structs");
        assert_eq!(d.languages[0].support[0].state, "supported");
        assert_eq!(d.renderers, [RendererInfo { id: "toydoc".into(), model_versions: vec![1], version: "1".into(), sites: false }]);
    }

    #[test]
    fn handle_generates_what_the_generator_does() {
        let req = gen_request();
        let direct: Vec<FileOut> = toy(&req).unwrap().into_iter().map(FileOut::from).collect();
        let Response::Generate(generated) = answer(&Request::generate(req)) else { panic!() };
        assert_eq!(generated.files, direct);
        assert_eq!(generated.files[1].path, "other/nested.toy");
        assert_eq!(generated.files[1].path_buf(), PathBuf::from("other").join("nested.toy"));
    }

    #[test]
    fn generator_failures_are_errors() {
        let mut req = gen_request();
        req.package.name = "fail".into();
        assert_eq!(message(answer(&Request::generate(req.clone()))), "refused: fail");
        req.package.name = "panic".into();
        assert_eq!(message(answer(&Request::generate(req.clone()))), "the toy generator panicked: toy panicked");
        req.target.version = "1.0".into();
        assert!(message(answer(&Request::generate(req.clone()))).contains("older than the toy generator handles"));
        req.target.language = "go".into();
        assert_eq!(message(answer(&Request::generate(req))), "this build of comline has no go generator");
    }
}
