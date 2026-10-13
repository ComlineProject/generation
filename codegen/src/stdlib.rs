//! Comline's standard library, from a generator's point of view.
//!
//! `std` is implicit in every package — a project that declares no
//! dependencies at all still receives `std/http` and `std/validators` in its
//! request — so every generator has to decide whether to copy std's code into
//! its output or reference std's own pre-generated package. Both generators
//! answered that identically, in their own words, including the same
//! "declare the dependency only if std was actually there" subtlety. This is
//! that answer, once.
//!
//! What the dependency *is* stays with the target: a git-pinned
//! `comline_std` crate for Rust, a semver-ranged `@comline/std` npm package
//! for TypeScript.

use std::borrow::Cow;

use comline_core::schema::ir::frozen::unit::FrozenUnit;

/// The namespace Comline's standard library occupies.
pub const NAMESPACE: &str = "std";

/// Whether `namespace` belongs to the standard library — `std` itself, or
/// anything beneath it.
pub fn is_std_namespace(namespace: &str) -> bool {
    namespace == NAMESPACE || namespace.starts_with("std/")
}

/// What a generator should emit, and whether to declare a std dependency.
pub struct StdSplit<'a> {
    /// The schemas to generate files for. Borrowed when nothing was removed,
    /// so the common path allocates nothing.
    pub emitted: Cow<'a, [(String, Vec<FrozenUnit>)]>,
    /// Whether the output should declare a dependency on std's own package.
    ///
    /// False when `external_std` is off, and *also* false when it is on but
    /// the request carried no std schemas — declaring a dependency nothing
    /// references would be a lie, and it is the difference between a
    /// manifest that is merely verbose and one that is wrong.
    pub needs_std_dependency: bool,
}

/// Split std out of what gets emitted, when `external_std` asks for it.
///
/// Note what this does **not** do: narrow the list a *reference* resolves
/// against. Std's schemas stay in the request and stay nameable; only the
/// output is narrowed. Resolving against `emitted` instead is how a std
/// reference loses its import and reverts to a bare, uncompilable name — see
/// [`crate::symbols::SymbolTable::with_external`].
pub fn split<'a>(schemas: &'a [(String, Vec<FrozenUnit>)], external_std: bool) -> StdSplit<'a> {
    if !external_std {
        return StdSplit {
            emitted: Cow::Borrowed(schemas),
            needs_std_dependency: false,
        };
    }
    let had_std = schemas.iter().any(|(ns, _)| is_std_namespace(ns));
    StdSplit {
        emitted: Cow::Owned(
            schemas
                .iter()
                .filter(|(ns, _)| !is_std_namespace(ns))
                .cloned()
                .collect(),
        ),
        needs_std_dependency: had_std,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schemas(names: &[&str]) -> Vec<(String, Vec<FrozenUnit>)> {
        names.iter().map(|n| (n.to_string(), vec![])).collect()
    }

    #[test]
    fn std_itself_and_anything_under_it() {
        assert!(is_std_namespace("std"));
        assert!(is_std_namespace("std/http"));
        assert!(is_std_namespace("std/a/b"));
    }

    #[test]
    fn a_namespace_merely_starting_with_std_is_not_std() {
        assert!(!is_std_namespace("standard"));
        assert!(!is_std_namespace("stdlib"));
        assert!(!is_std_namespace("my/std"));
    }

    #[test]
    fn inlined_std_emits_everything_and_declares_nothing() {
        let all = schemas(&["api", "std/http"]);
        let split = split(&all, false);
        assert_eq!(split.emitted.len(), 2);
        assert!(!split.needs_std_dependency);
        assert!(matches!(split.emitted, Cow::Borrowed(_)), "no clone needed");
    }

    #[test]
    fn external_std_drops_std_and_declares_the_dependency() {
        let all = schemas(&["api", "std/http", "std/validators"]);
        let split = split(&all, true);
        let left: Vec<&str> = split.emitted.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(left, vec!["api"]);
        assert!(split.needs_std_dependency);
    }

    #[test]
    fn external_std_declares_nothing_when_there_was_no_std() {
        // The flag is on but nothing it applies to was present.
        let all = schemas(&["api"]);
        let split = split(&all, true);
        assert_eq!(split.emitted.len(), 1);
        assert!(!split.needs_std_dependency);
    }
}
