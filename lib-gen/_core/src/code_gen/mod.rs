// Compiled Languages
pub mod rust;
// TODO(de-rot G2): revive against the current core API — both target a
// pre-audit `FrozenUnit` and call `basic_storage` / `package_from_path_without_context`
// which core has since removed. See design/generation.md.
// pub mod rust_c_ffi;
// pub mod rust_abi_stable;

// Dynamic Languages
pub mod typescript;

// Standard Uses
use std::collections::HashMap;
use std::path::PathBuf;

// Crate Uses

// External Uses
use comline_core::schema::ir::frozen::unit::FrozenUnit;

use eyre::Result;
use once_cell::sync::Lazy;


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

#[allow(unused)]
pub type VersionGenerators = Lazy<HashMap<&'static str, GeneratorFn>>;
#[allow(unused)]
pub type GeneratorFn = fn(&GenRequest) -> Result<Vec<GeneratedFile>>;

#[allow(unused)]
static LANG_GENERATORS: Lazy<HashMap<&str, (&VersionGenerators, &str)>> = Lazy::new(|| {
    HashMap::from([
        ("rust", (&rust::GENERATORS, "rs")),
        ("typescript", (&typescript::GENERATORS, "ts")),
        ("ts", (&typescript::GENERATORS, "ts")),

        //("luau", (&luau::GENERATORS, "luau")),
        //("python", (&python::GENERATORS, "py"))
    ])
});

#[allow(unused)]
pub fn find_generator(name: &str, version: &str) -> Option<(&'static GeneratorFn, &'static str)> {
    if let Some((lang_generator, extension)) = LANG_GENERATORS.get(name) {
        if let Some(version_generator) = lang_generator.get(version) {
            return Some((version_generator, extension))
        }
    };

    None
}
