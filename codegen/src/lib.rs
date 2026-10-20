//! `comline-codegen` — the language-neutral contract every code generator is
//! built against, plus the [`Registry`] the CLI composes them into.
//!
//! Per-language generators live in their own crates (`comline-codegen-rust`,
//! `comline-codegen-typescript`, …), each contributing through its own
//! `register(&mut Registry)`. See `design/generation.md`.

pub mod builder;
pub mod collisions;
pub mod connections;
pub mod features;
pub mod modules;
pub mod naming;
pub mod stdlib;
pub mod symbols;
pub mod utils;
pub mod validation;
pub mod versions;

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use comline_core::schema::ir::frozen::unit::FrozenUnit;

use eyre::{eyre, Result};

use versions::{LangVersion, Versions};

/// One file a generator wants written, relative to the target's output root.
#[derive(Debug, Clone)]
pub struct GeneratedFile {
    pub path: PathBuf,
    pub contents: String,
}

/// How far generation goes: `Code` = bare source files; `Lib` = a buildable
/// package (manifest + module tree).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Code,
    Lib,
}

/// Package identity, for the manifest a `Lib` build emits. Unused by `Code`.
#[derive(Debug, Clone)]
pub struct PackageMeta {
    pub name: String,
    pub version: String,
}

/// Everything a generator needs for one (target, version): every schema in the
/// package (namespace + IR), the mode, and the package identity.
pub struct GenRequest<'a> {
    pub mode: Mode,
    pub schemas: &'a [(String, Vec<FrozenUnit>)],
    pub package: PackageMeta,
    /// Consumer-set fallback wire framing (`comline.toml`'s
    /// `[generate] default_framing`), applied by a generator to any protocol
    /// that does not pick one itself with `@framing`. `None` ⇒ the generator's
    /// built-in default. The value is a framing name the generator recognises
    /// (`"jsonrpc"`, `"datagram"`, …); an unknown one falls back to the default.
    pub default_framing: Option<String>,
    /// The target's language/toolchain version - the `#`-suffix of the
    /// language the congregation declared (`rust#1.70.0` -> `"1.70.0"`,
    /// `typescript#5.0` -> `"5.0"`), which is also what selected this
    /// generator. A generator records it in whatever its own ecosystem's
    /// manifest calls that (`rust-version` in a `Cargo.toml`), and ignores
    /// it otherwise - `Mode::Code` has no manifest to put it in.
    pub lang_version: String,
    /// `comline.toml`'s `[generate] external_std` - `true` means `std`
    /// should be referenced as its own crate instead of inlined into this
    /// package's own generated output. Only a generator that knows how to
    /// do that (today, Rust's) acts on it; others ignore it, same as any
    /// other field they have no use for. Default `false` (inlined,
    /// unchanged) everywhere this isn't explicitly set.
    pub external_std: bool,
    /// `comline.toml`'s `[generate] validate_on_decode` - whether decoding a
    /// value runs its `@validators` checks. A generator that emits no checks
    /// ignores it.
    pub validate_on_decode: validation::ValidateOnDecode,
}

impl GenRequest<'_> {
    /// [`lang_version`](Self::lang_version), parsed.
    pub fn version(&self) -> Result<LangVersion> {
        LangVersion::parse(&self.lang_version)
            .ok_or_else(|| eyre!("`{}` isn't a version: expected dotted numbers, like `1.70.0`", self.lang_version))
    }

    /// Whether the target's version is at least `boundary`, one of the
    /// generator's own [`Versions`] boundaries: how a generator picks output
    /// that needs a newer toolchain. An unparsable version counts as the
    /// floor; [`features::refuse_unsupported`] has already refused it.
    pub fn at_least(&self, boundary: &str) -> bool {
        match (LangVersion::parse(&self.lang_version), LangVersion::parse(boundary)) {
            (Some(version), Some(boundary)) => version >= boundary,
            _ => false,
        }
    }
}

/// A code generator: frozen IR in, generated files out.
pub type GeneratorFn = fn(&GenRequest) -> Result<Vec<GeneratedFile>>;

/// One generator, as it registers: its language, what it generates with,
/// what it supports, and which versions of the language it handles.
#[derive(Debug, Clone, Copy)]
pub struct Generator {
    /// `rust`, `typescript`: the name a congregation declares.
    pub language: &'static str,
    /// The source file extension: `rs`, `ts`.
    pub ext: &'static str,
    pub generate: GeneratorFn,
    pub support: features::SupportTable,
    pub versions: Versions,
}

/// Maps a declared `(language, version)` to a generator. The CLI builds one
/// at startup from the generator crates it was compiled with.
///
/// A generator registers at its floor, and handles every version from there
/// up: a declared version gets the one with the newest floor at or below it.
#[derive(Default)]
pub struct Registry {
    /// Per language, by floor.
    langs: BTreeMap<&'static str, Vec<Generator>>,
}

/// A declared target, matched to its generator.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub generator: Generator,
    /// The declared version, which the generator receives.
    pub version: LangVersion,
}

impl Resolved {
    /// The boundary this target generates as.
    pub fn boundary(&self) -> &'static str {
        self.generator.versions.boundary_for(&self.version).expect("resolved at or above the floor")
    }

    /// Whether generating for this version is verified (see
    /// [`Versions::is_verified`]).
    pub fn is_verified(&self) -> bool {
        self.generator.versions.is_verified(&self.version)
    }

    /// `feature`'s support at this version.
    pub fn support(&self, feature: features::Feature) -> features::Support {
        features::support_at(self.generator.support, feature, &self.version)
    }
}

/// Why a declared target has no generator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unresolved {
    /// This build has no generator for the language at all.
    UnknownLanguage { language: String },
    /// The version isn't dotted numbers.
    BadVersion { language: String, version: String },
    /// The version is older than every generator's floor.
    BelowFloor { language: String, version: String, floor: &'static str },
}

impl fmt::Display for Unresolved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unresolved::UnknownLanguage { language } => write!(f, "this build of comline has no {language} generator"),
            Unresolved::BadVersion { language, version } => {
                write!(f, "`{language}#{version}`: `{version}` isn't a version - expected dotted numbers, like `1.70.0`")
            }
            Unresolved::BelowFloor { language, version, floor } => write!(
                f,
                "`{language}#{version}` is older than the {language} generator handles: it needs `{language}#{floor}` or later"
            ),
        }
    }
}

impl std::error::Error for Unresolved {}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `generator` at its floor. One at the same floor is replaced.
    pub fn register(&mut self, generator: Generator) {
        let floor = LangVersion::parse(generator.versions.floor);
        let same = self.langs.entry(generator.language).or_default();
        same.retain(|g| LangVersion::parse(g.versions.floor) != floor);
        same.push(generator);
        same.sort_by_key(|g| LangVersion::parse(g.versions.floor));
    }

    /// The generator for a declared `language#version`.
    pub fn resolve(&self, language: &str, version: &str) -> std::result::Result<Resolved, Unresolved> {
        let generators = self
            .langs
            .get(language)
            .ok_or_else(|| Unresolved::UnknownLanguage { language: language.to_string() })?;
        let declared = LangVersion::parse(version)
            .ok_or_else(|| Unresolved::BadVersion { language: language.to_string(), version: version.to_string() })?;
        generators
            .iter()
            .rev()
            .find(|g| LangVersion::parse(g.versions.floor).is_some_and(|floor| floor <= declared))
            .map(|g| Resolved { generator: *g, version: declared })
            .ok_or_else(|| Unresolved::BelowFloor {
                language: language.to_string(),
                version: version.to_string(),
                floor: generators[0].versions.floor,
            })
    }

    /// The generator for `(name, version)` and its file extension, if one
    /// handles that version.
    pub fn find(&self, name: &str, version: &str) -> Option<(GeneratorFn, &'static str)> {
        self.resolve(name, version).ok().map(|r| (r.generator.generate, r.generator.ext))
    }

    /// Every generator this registry has, sorted by language and floor.
    pub fn targets(&self) -> Vec<Target> {
        self.langs
            .values()
            .flatten()
            .map(|g| Target { name: g.language, ext: g.ext, version: g.versions.floor, versions: g.versions })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop(_req: &GenRequest) -> Result<Vec<GeneratedFile>> {
        Ok(vec![])
    }

    fn generator(language: &'static str, ext: &'static str, floor: &'static str) -> Generator {
        Generator {
            language,
            ext,
            generate: noop,
            support: &[],
            versions: Versions { floor, changes: &[], verified_up_to: floor, forward_compatible: true },
        }
    }

    fn registry() -> Registry {
        let mut registry = Registry::new();
        registry.register(generator("typescript", "ts", "5.0"));
        registry.register(generator("rust", "rs", "1.70.0"));
        registry.register(generator("rust", "rs", "1.60.0"));
        registry
    }

    #[test]
    fn targets_is_flattened_and_sorted() {
        let targets: Vec<(&str, &str)> = registry().targets().iter().map(|t| (t.name, t.version)).collect();
        assert_eq!(targets, [("rust", "1.60.0"), ("rust", "1.70.0"), ("typescript", "5.0")]);
    }

    #[test]
    fn targets_is_empty_for_an_empty_registry() {
        assert_eq!(Registry::new().targets(), vec![]);
    }

    #[test]
    fn a_declared_version_gets_the_newest_floor_at_or_below_it() {
        let registry = registry();
        let floor = |v| registry.resolve("rust", v).map(|r| r.generator.versions.floor);
        assert_eq!(floor("1.70.0"), Ok("1.70.0"));
        assert_eq!(floor("1.70"), Ok("1.70.0"));
        assert_eq!(floor("1.82.1"), Ok("1.70.0"));
        assert_eq!(floor("1.65"), Ok("1.60.0"));
        assert_eq!(registry.resolve("rust", "1.82.1").unwrap().version.to_string(), "1.82.1");
        assert_eq!(registry.resolve("typescript", "5.4").unwrap().generator.ext, "ts");
    }

    #[test]
    fn a_miss_says_why() {
        let registry = registry();
        assert_eq!(
            registry.resolve("rust", "1.50").unwrap_err().to_string(),
            "`rust#1.50` is older than the rust generator handles: it needs `rust#1.60.0` or later"
        );
        assert_eq!(
            registry.resolve("rust", "latest").unwrap_err().to_string(),
            "`rust#latest`: `latest` isn't a version - expected dotted numbers, like `1.70.0`"
        );
        assert_eq!(registry.resolve("go", "1.21").unwrap_err().to_string(), "this build of comline has no go generator");
    }

    #[test]
    fn at_least_compares_the_declared_version() {
        let req = |v: &str| GenRequest {
            mode: Mode::Code,
            schemas: &[],
            package: PackageMeta { name: "p".into(), version: "0.0.0".into() },
            default_framing: None,
            lang_version: v.into(),
            external_std: false,
            validate_on_decode: Default::default(),
        };
        assert!(req("1.81").at_least("1.81.0"));
        assert!(req("1.90.0").at_least("1.81.0"));
        assert!(!req("1.80.9").at_least("1.81.0"));
        assert!(req("1.70.0").version().is_ok());
        assert!(req("nightly").version().is_err());
    }
}

/// One compiled-in generator a [`Registry`] has: `name#version` (its floor)
/// is exactly the token `.idp`'s `ItemVersionMeta` grammar rule expects, and
/// every later version of the language reaches it too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub name: &'static str,
    pub ext: &'static str,
    /// The floor.
    pub version: &'static str,
    pub versions: Versions,
}
