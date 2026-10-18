//! What a bare name in a schema actually refers to.
//!
//! This is the layer whose absence caused every cross-schema generation bug
//! so far: a reference emitted as a bare, unqualified name because nothing
//! knew which schema declared it. The Rust generator shipped it
//! (comline-rust#28), then the TypeScript generator shipped it independently
//! (comline-typescript#32), then an aliased import shipped a path to a name
//! that didn't exist (comline-rust#39). Three defects, one missing table.
//!
//! Core does the hard part already. `resolve_use_declaration` expands every
//! `use` form — plain, aliased, item-list, glob — into flat
//! [`FrozenUnit::Import`] leaves in the importing schema's own units, so
//! resolving a reference never requires looking at another schema's source.
//! What was missing was somewhere to put the result.
//!
//! Rendering stays with the target. Rust writes a qualified path inline
//! (`crate::shared::Shared`) and no import block; TypeScript writes the bare
//! name and an `import type` header. Those are genuinely different languages,
//! so this module resolves and records, and each generator writes.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};

use comline_core::schema::ir::frozen::unit::FrozenUnit;

/// Where a name a schema uses actually comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// The declaring schema's namespace, `/`-joined (`"std/http"`).
    pub namespace: String,
    /// The name **that schema** declares. Differs from the name the importing
    /// schema binds exactly when there is an alias, and getting the two
    /// confused is comline-rust#39: `use shared::Shared as Aliased` must emit
    /// a path ending in `Shared`, because `Aliased` is a local nickname that
    /// the other schema has never heard of.
    pub declared: String,
}

/// Bare name as one schema binds it → where it comes from.
///
/// Only names that resolve to a schema **in this request** are included. A
/// reference to something the request doesn't carry is left out deliberately,
/// so a generator emits it bare rather than pointing at a file that was never
/// written — see [`SymbolTable::with_external`] for the one case where a
/// namespace is importable despite being absent.
#[derive(Debug, Default, Clone)]
pub struct SymbolTable {
    bound: HashMap<String, Origin>,
}

impl SymbolTable {
    /// Resolve `units`' imports against every schema in the request.
    pub fn new(units: &[FrozenUnit], schemas: &[(String, Vec<FrozenUnit>)]) -> Self {
        Self::with_external(units, schemas, |_| false)
    }

    /// As [`new`](Self::new), but `external` names namespaces that are
    /// importable even though the request will not emit a file for them.
    ///
    /// This exists for `external_std`: std's schemas are filtered out of the
    /// generated output and referenced from their own package instead, so
    /// they are present in the request, absent from the output, and still
    /// nameable. Resolving against the *narrowed* list instead is how the
    /// bare-name bug comes back — a schema that isn't emitted is still a
    /// schema whose types can be named.
    pub fn with_external(
        units: &[FrozenUnit],
        schemas: &[(String, Vec<FrozenUnit>)],
        external: impl Fn(&str) -> bool,
    ) -> Self {
        let known =
            |namespace: &str| schemas.iter().any(|(n, _)| n == namespace) || external(namespace);

        let mut bound = HashMap::new();
        for unit in units {
            let FrozenUnit::Import(path, alias, _) = unit else {
                continue;
            };
            // The glob marker (`ns::*`) binds no name by itself. Core emits it
            // alongside one ordinary leaf per name the target declares, and
            // those leaves are what carry the bindings.
            if path.ends_with("::*") {
                continue;
            }
            let Some((namespace, declared)) = path.rsplit_once("::") else {
                continue; // a single-segment path binds no name here
            };
            let namespace = namespace.replace("::", "/");
            if !known(&namespace) {
                continue;
            }
            let name = alias.clone().unwrap_or_else(|| declared.to_string());
            bound.insert(
                name,
                Origin {
                    namespace,
                    declared: declared.to_string(),
                },
            );
        }
        Self { bound }
    }

    /// Where `name` comes from, or `None` if this schema declares it itself
    /// (or it is a primitive, or unresolvable).
    pub fn origin(&self, name: &str) -> Option<&Origin> {
        self.bound.get(name)
    }

    pub fn is_empty(&self) -> bool {
        self.bound.is_empty()
    }
}

/// A [`SymbolTable`] that remembers which names were actually reached for.
///
/// The point of recording is that an import block can then be *derived* from
/// use rather than maintained alongside it: naming a type is what registers
/// it. It also means an unreferenced `use` produces no import, which matters
/// because real schemas carry them.
///
/// Interior mutability so that resolving reads as a query — every caller in
/// a generator is partway through building a string and has no business
/// holding a mutable borrow.
#[derive(Debug, Default)]
pub struct UseTracker {
    table: SymbolTable,
    reached: RefCell<BTreeSet<String>>,
}

impl UseTracker {
    pub fn new(table: SymbolTable) -> Self {
        Self {
            table,
            reached: RefCell::new(BTreeSet::new()),
        }
    }

    /// Where `name` comes from, recording that it was used.
    pub fn resolve(&self, name: &str) -> Option<&Origin> {
        let origin = self.table.origin(name)?;
        self.reached.borrow_mut().insert(name.to_string());
        Some(origin)
    }

    /// Every name reached for, with its origin — sorted by bound name, so a
    /// generated import block is stable enough to diff.
    ///
    /// `from` is excluded: a schema never imports from itself, and a `use`
    /// that resolved back to it would otherwise produce a circular
    /// self-import.
    pub fn reached(&self, from: &str) -> Vec<(String, Origin)> {
        self.reached
            .borrow()
            .iter()
            .filter_map(|name| {
                let origin = self.table.origin(name)?;
                (origin.namespace != from).then(|| (name.clone(), origin.clone()))
            })
            .collect()
    }
}

/// Error ordinal → the name of the `error` type that carries it, within one
/// schema's own error space.
///
/// Reads this schema's units only, which is what both generators already do,
/// moved here unchanged rather than quietly improved. That is enough more
/// often than it looks: core gives a `throws` that names another schema's
/// error a **re-export slot** in this schema — a `FrozenUnit::Error` with
/// `imported_from: Some(namespace)` and a fresh local ordinal — so the name
/// resolves here even when the declaration lives elsewhere.
///
/// What no generator does yet is *notice* that slot. Both emit a full type
/// declaration for it, so an error shared between two schemas is declared in
/// both of their outputs instead of imported in one. Worth its own change,
/// with a conformance fixture to pin it; this helper is not the place to
/// decide it.
pub fn error_names(units: &[FrozenUnit]) -> HashMap<u16, String> {
    units
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::Error { ordinal, name, .. } => Some((*ordinal, name.clone())),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn import(path: &str, alias: Option<&str>) -> FrozenUnit {
        FrozenUnit::Import(path.to_string(), alias.map(str::to_string), (0, 0))
    }

    fn schema(namespace: &str) -> (String, Vec<FrozenUnit>) {
        (namespace.to_string(), vec![])
    }

    #[test]
    fn a_plain_import_resolves_to_its_declaring_schema() {
        let units = vec![import("shared::http::Request", None)];
        let table = SymbolTable::new(&units, &[schema("shared/http")]);
        assert_eq!(
            table.origin("Request"),
            Some(&Origin {
                namespace: "shared/http".to_string(),
                declared: "Request".to_string(),
            })
        );
    }

    #[test]
    fn an_alias_keeps_the_name_the_other_schema_declares() {
        // comline-rust#39: emitting a path ending in the alias names nothing.
        let units = vec![import("shared::Shared", Some("Aliased"))];
        let table = SymbolTable::new(&units, &[schema("shared")]);
        let origin = table.origin("Aliased").expect("bound under the alias");
        assert_eq!(origin.declared, "Shared", "the exported name");
        assert_eq!(table.origin("Shared"), None, "the alias is what is bound");
    }

    #[test]
    fn the_glob_marker_binds_nothing_on_its_own() {
        let units = vec![import("shared::*", None)];
        let table = SymbolTable::new(&units, &[schema("shared")]);
        assert!(table.is_empty());
    }

    #[test]
    fn a_glob_resolves_through_the_leaves_core_expands_it_into() {
        // Core emits the marker *and* one leaf per declared name.
        let units = vec![import("shared::*", None), import("shared::Shared", None)];
        let table = SymbolTable::new(&units, &[schema("shared")]);
        assert_eq!(
            table.origin("Shared").map(|o| o.declared.as_str()),
            Some("Shared")
        );
    }

    #[test]
    fn a_namespace_the_request_does_not_carry_is_left_unresolved() {
        // Otherwise the generator would import from a file it never wrote.
        let units = vec![import("elsewhere::Thing", None)];
        let table = SymbolTable::new(&units, &[schema("shared")]);
        assert_eq!(table.origin("Thing"), None);
    }

    #[test]
    fn an_external_namespace_resolves_despite_emitting_no_file() {
        // `external_std`: present in the request, absent from the output,
        // still nameable. Resolving against the narrowed list is how the
        // bare-name bug returns.
        let units = vec![import("std::http::Request", None)];
        let table = SymbolTable::with_external(&units, &[], |ns| ns.starts_with("std"));
        assert_eq!(
            table.origin("Request").map(|o| o.namespace.as_str()),
            Some("std/http")
        );
    }

    #[test]
    fn a_single_segment_path_binds_no_name() {
        let units = vec![import("shared", None)];
        let table = SymbolTable::new(&units, &[schema("shared")]);
        assert!(table.is_empty());
    }

    #[test]
    fn the_tracker_records_only_what_was_reached_for() {
        let units = vec![
            import("shared::Used", None),
            import("shared::Ignored", None),
        ];
        let tracker = UseTracker::new(SymbolTable::new(&units, &[schema("shared")]));

        assert!(tracker.resolve("Used").is_some());
        assert!(tracker.resolve("NotAnImport").is_none());

        let reached = tracker.reached("consumer");
        assert_eq!(
            reached.len(),
            1,
            "an unused `use` registers nothing: {reached:?}"
        );
        assert_eq!(reached[0].0, "Used");
    }

    #[test]
    fn the_tracker_never_reports_an_import_from_the_schema_itself() {
        let units = vec![import("consumer::Own", None)];
        let tracker = UseTracker::new(SymbolTable::new(&units, &[schema("consumer")]));
        assert!(tracker.resolve("Own").is_some());
        assert!(
            tracker.reached("consumer").is_empty(),
            "a self-import would be circular"
        );
    }

    #[test]
    fn reached_is_sorted_for_stable_output() {
        let units = vec![import("shared::Beta", None), import("shared::Alpha", None)];
        let tracker = UseTracker::new(SymbolTable::new(&units, &[schema("shared")]));
        tracker.resolve("Beta");
        tracker.resolve("Alpha");
        let names: Vec<String> = tracker
            .reached("consumer")
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(names, vec!["Alpha", "Beta"]);
    }

    #[test]
    fn error_names_maps_ordinals_for_this_schema() {
        let units = vec![FrozenUnit::Error {
            docstring: None,
            parameters: vec![],
            ordinal: 7,
            imported_from: None,
            name: "NotFound".to_string(),
            message: String::new(),
            fields: vec![],
            span: Some((0, 0)),
            message_span: None,
            uninterpolated: vec![],
        }];
        assert_eq!(
            error_names(&units).get(&7).map(String::as_str),
            Some("NotFound")
        );
    }

    #[test]
    fn a_re_exported_error_still_resolves_by_ordinal() {
        // Core gives a `throws` naming another schema's error a slot here,
        // with `imported_from` set. The name is findable; that it is a
        // re-export is information no generator acts on yet.
        let units = vec![FrozenUnit::Error {
            docstring: None,
            parameters: vec![],
            ordinal: 3,
            imported_from: Some("shared".to_string()),
            name: "Upstream".to_string(),
            message: String::new(),
            fields: vec![],
            span: None,
            message_span: None,
            uninterpolated: vec![],
        }];
        assert_eq!(
            error_names(&units).get(&3).map(String::as_str),
            Some("Upstream")
        );
    }
}
