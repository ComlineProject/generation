//! `comline-codegen` — the language-neutral contract every code generator is
//! built against, plus the [`Registry`] the CLI composes them into.
//!
//! Per-language generators live in their own crates (`comline-codegen-rust`,
//! `comline-codegen-typescript`, …), each contributing through its own
//! `register(&mut Registry)`. See `design/generation.md`.

pub mod builder;
pub mod utils;

use std::collections::HashMap;
use std::path::PathBuf;

use comline_core::schema::ir::frozen::unit::FrozenUnit;

use eyre::Result;

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
}

/// A code generator: frozen IR in, generated files out.
pub type GeneratorFn = fn(&GenRequest) -> Result<Vec<GeneratedFile>>;

/// Maps `(language, version)` to a generator. The CLI builds one at startup from
/// the generator crates it was compiled with.
#[derive(Default)]
pub struct Registry {
    langs: HashMap<&'static str, Lang>,
}

struct Lang {
    ext: &'static str,
    versions: HashMap<&'static str, GeneratorFn>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `generator` for `name` at `version`. `ext` is the source file
    /// extension (`"rs"`, `"ts"`, …).
    pub fn register(
        &mut self,
        name: &'static str,
        ext: &'static str,
        version: &'static str,
        generator: GeneratorFn,
    ) {
        self.langs
            .entry(name)
            .or_insert_with(|| Lang {
                ext,
                versions: HashMap::new(),
            })
            .versions
            .insert(version, generator);
    }

    /// The generator for `(name, version)` and its file extension, if one is
    /// registered.
    pub fn find(&self, name: &str, version: &str) -> Option<(GeneratorFn, &'static str)> {
        let lang = self.langs.get(name)?;
        let generator = *lang.versions.get(version)?;
        Some((generator, lang.ext))
    }

    /// Every `(language, version)` this registry can generate, sorted by
    /// `(name, version)` — `HashMap` iteration order isn't stable, and this
    /// is meant to be diffed/scripted against (`comline targets`).
    pub fn targets(&self) -> Vec<Target> {
        let mut out: Vec<Target> = self
            .langs
            .iter()
            .flat_map(|(name, lang)| {
                lang.versions.keys().map(move |version| Target {
                    name,
                    ext: lang.ext,
                    version,
                })
            })
            .collect();
        out.sort_unstable_by_key(|t| (t.name, t.version));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop(_req: &GenRequest) -> Result<Vec<GeneratedFile>> {
        Ok(vec![])
    }

    #[test]
    fn targets_is_flattened_and_sorted() {
        let mut registry = Registry::new();
        registry.register("typescript", "ts", "5.0", noop);
        registry.register("rust", "rs", "1.70.0", noop);
        registry.register("rust", "rs", "1.60.0", noop);

        let targets = registry.targets();
        assert_eq!(
            targets,
            vec![
                Target { name: "rust", ext: "rs", version: "1.60.0" },
                Target { name: "rust", ext: "rs", version: "1.70.0" },
                Target { name: "typescript", ext: "ts", version: "5.0" },
            ]
        );
    }

    #[test]
    fn targets_is_empty_for_an_empty_registry() {
        assert_eq!(Registry::new().targets(), vec![]);
    }
}

/// One compiled-in `(language, version)` a [`Registry`] can generate —
/// `name#version` is exactly the token `.idp`'s `ItemVersionMeta` grammar
/// rule expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub name: &'static str,
    pub ext: &'static str,
    pub version: &'static str,
}
