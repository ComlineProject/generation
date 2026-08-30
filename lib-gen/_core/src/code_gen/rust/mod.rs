// Relative Modules
mod generator;

// Standard Uses
use std::collections::HashMap;

// Crate Uses
use crate::code_gen::VersionGenerators;

// External Uses
use once_cell::sync::Lazy;

pub use generator::generate_rust;


// One implementation serves every Rust version today; the map keeps the
// version-keyed shape `find_generator` expects. Version-specific generators
// (async-trait flavours, edition differences) can be added as more keys.
//
// De-rot G1: the old `generate_frozen_schemas_into_path` (read frozen schemas
// from a version dir, write a `src/` tree) is gone — the CLI is the composition
// root and owns that orchestration now. See design/generation.md.
#[allow(unused)]
pub(crate) static GENERATORS: VersionGenerators = Lazy::new(|| {
    HashMap::from([
        ("1.70.0", generate_rust as _),
    ])
});
