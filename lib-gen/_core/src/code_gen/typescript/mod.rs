// Relative Modules
mod generator;

// Standard Uses
use std::collections::HashMap;

// Crate Uses
use crate::code_gen::VersionGenerators;

// External Uses
use once_cell::sync::Lazy;

pub use generator::generate_typescript;


// One implementation for now; the map keeps the version-keyed shape
// `find_generator` expects. `target` / `module` dialect handling is a later
// concern — see "Language version & dialect" in design/generation.md.
#[allow(unused)]
pub(crate) static GENERATORS: VersionGenerators = Lazy::new(|| {
    HashMap::from([
        ("5.0", generate_typescript as _),
    ])
});
