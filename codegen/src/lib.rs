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
}
