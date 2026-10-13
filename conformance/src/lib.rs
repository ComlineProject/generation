//! The codegen conformance corpus.
//!
//! Each [`Fixture`] is a whole *package* — one or more schemas, as
//! `Vec<FrozenUnit>` built here rather than parsed, so the corpus pins exactly
//! what IR shape every generator must handle, independent of the parser.
//! `tests/conformance.rs` runs every generator over every fixture and compares
//! to the checked-in golden output; run it with `CONFORMANCE_BLESS=1` to
//! (re)generate the goldens.
//!
//! **Why a package and not a schema.** Until this was a package, every fixture
//! had exactly one namespace, so the corpus could not express a reference that
//! crosses schemas — and that is precisely the shape that broke twice, in the
//! Rust generator and then independently in the TypeScript one, both times
//! under a green suite. A corpus that cannot state the bug cannot guard
//! against it. See Proposals → How generators are built.

use comline_codegen::Mode;
use comline_core::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};
use comline_core::schema::ir::frozen::unit::{FrozenArgument, FrozenUnit};

/// One package: every schema in it, and the request knobs it exists to
/// exercise.
pub struct Fixture {
    pub name: &'static str,
    /// `(namespace, units)` per schema, in the order a generator receives
    /// them.
    pub schemas: Vec<(String, Vec<FrozenUnit>)>,
    /// `Code` for most fixtures; `Lib` for the ones whose point is the
    /// package shape a generator builds (module trees, barrels, manifests).
    pub mode: Mode,
    /// Whether `std` is referenced from its own package rather than inlined.
    pub external_std: bool,
}

impl Fixture {
    /// A single-schema `Code`-mode fixture — the common case.
    fn one(name: &'static str, namespace: &str, units: Vec<FrozenUnit>) -> Self {
        Self {
            name,
            schemas: vec![(namespace.to_string(), units)],
            mode: Mode::Code,
            external_std: false,
        }
    }

    /// A multi-schema fixture; `Code` mode unless [`Self::in_lib_mode`] is
    /// used.
    fn package(name: &'static str, schemas: Vec<(String, Vec<FrozenUnit>)>) -> Self {
        Self {
            name,
            schemas,
            mode: Mode::Code,
            external_std: false,
        }
    }

    fn in_lib_mode(mut self) -> Self {
        self.mode = Mode::Lib;
        self
    }

    fn with_external_std(mut self) -> Self {
        self.external_std = true;
        self
    }
}

/// Every fixture, in a stable order.
pub fn corpus() -> Vec<Fixture> {
    vec![
        primitives(),
        optional_and_arrays(),
        enum_basic(),
        type_refs(),
        protocol(),
        constant(),
        // Shapes a single-schema fixture could not express at all.
        cross_schema(),
        import_alias(),
        import_glob(),
        nested_namespace(),
        external_std(),
    ]
}

// ── builders ────────────────────────────────────────────────────────────────

fn field(name: &str, ty: &str, optional: bool) -> FrozenUnit {
    FrozenUnit::Field {
        docstring: None,
        parameters: vec![],
        optional,
        name: name.to_string(),
        kind_value: KindValue::Namespaced(ty.to_string(), None),
        span: (0, 0),
    }
}

fn structure(name: &str, fields: Vec<FrozenUnit>) -> FrozenUnit {
    FrozenUnit::Struct {
        docstring: None,
        parameters: vec![],
        name: name.to_string(),
        fields,
        span: (0, 0),
    }
}

fn variant(name: &str) -> FrozenUnit {
    FrozenUnit::EnumVariant(KindValue::EnumVariant(name.to_string(), None), (0, 0))
}

fn arg(name: &str, prim: Primitive) -> FrozenArgument {
    FrozenArgument {
        name: name.to_string(),
        kind: KindValue::Primitive(prim),
        span: (0, 0),
    }
}

fn function(
    name: &str,
    args: Vec<FrozenArgument>,
    ret: Option<&str>,
    throws: Vec<u16>,
) -> FrozenUnit {
    FrozenUnit::Function {
        docstring: String::new(),
        parameters: vec![],
        name: name.to_string(),
        arguments: args,
        _return: ret.map(|t| KindValue::Namespaced(t.to_string(), None)),
        throws,
        span: (0, 0),
    }
}

/// A local `error`, ordinal assigned by the fixture itself (mirroring what
/// `plan_error_space` would freeze from a real schema — see
/// `core::schema::ir::compiler::interpreter::incremental`).
fn error(ordinal: u16, name: &str) -> FrozenUnit {
    FrozenUnit::Error {
        docstring: None,
        parameters: vec![],
        ordinal,
        imported_from: None,
        name: name.to_string(),
        message: format!("{name} occurred"),
        fields: vec![],
        span: Some((0, 0)),
    }
}

/// A `use` declaration as core freezes it: one leaf entry per bound name,
/// its path `::`-joined. `alias` is `Some` for `use path as Alias`; a glob is
/// the path with a trailing `::*`, which binds no name on its own.
fn import(path: &str, alias: Option<&str>) -> FrozenUnit {
    FrozenUnit::Import(path.to_string(), alias.map(str::to_string), (0, 0))
}

// ── fixtures ────────────────────────────────────────────────────────────────

fn primitives() -> Fixture {
    Fixture::one(
        "primitives",
        "conformance",
        vec![structure(
            "Scalars",
            vec![
                field("flag", "bool", false),
                field("small", "u8", false),
                field("medium", "u32", false),
                field("large", "u64", false),
                field("huge", "u128", false),
                field("signed_small", "s8", false),
                field("signed_large", "s64", false),
                field("name", "string", false),
            ],
        )],
    )
}

fn optional_and_arrays() -> Fixture {
    Fixture::one(
        "optional_and_arrays",
        "conformance",
        vec![structure(
            "Bag",
            vec![
                field("required", "string", false),
                field("maybe", "u32", true),
                field("many", "string[]", false),
                field("maybe_many", "u32[]", true),
            ],
        )],
    )
}

fn enum_basic() -> Fixture {
    Fixture::one(
        "enum_basic",
        "conformance",
        vec![FrozenUnit::Enum {
            docstring: None,
            name: "State".to_string(),
            variants: vec![variant("Pending"), variant("Active"), variant("Closed")],
            span: (0, 0),
        }],
    )
}

fn type_refs() -> Fixture {
    Fixture::one(
        "type_refs",
        "conformance",
        vec![
            structure("Inner", vec![field("value", "s32", false)]),
            structure(
                "Outer",
                vec![
                    field("one", "Inner", false),
                    field("some", "Inner", true),
                    field("list", "Inner[]", false),
                ],
            ),
        ],
    )
}

fn protocol() -> Fixture {
    Fixture::one(
        "protocol",
        "conformance",
        vec![
            error(0, "NotFound"),
            FrozenUnit::Protocol {
                docstring: "Conformance protocol".to_string(),
                parameters: vec![],
                name: "Service".to_string(),
                functions: vec![
                    function(
                        "lookup",
                        vec![arg("id", Primitive::S32(None))],
                        Some("Record"),
                        vec![0], // ordinal of `NotFound`, above
                    ),
                    function("count", vec![], Some("u64"), vec![]),
                    function(
                        "notify",
                        vec![arg("code", Primitive::U16(None))],
                        None,
                        vec![],
                    ),
                ],
                span: (0, 0),
            },
        ],
    )
}

/// A `const` declaration.
///
/// **Both goldens are empty**, and that is a finding rather than a pass:
/// neither generator handles `FrozenUnit::Constant`, so a schema declaring a
/// constant produces a file with nothing in it. The fixture has been here
/// since before the corpus could express much else, and an empty golden is
/// indistinguishable from a correct one at a glance — worth saying out loud,
/// because a golden pins missing behaviour exactly as silently as it pins
/// present behaviour.
///
/// The Rust golden is not quite empty: it carries
/// `use serde::{Deserialize, Serialize};` with nothing to derive on, which is
/// an unused import. The same shape in `comline-rust`'s generated
/// `comline_std` is what makes clippy fail there.
fn constant() -> Fixture {
    Fixture::one(
        "constant",
        "conformance",
        vec![FrozenUnit::Constant {
            docstring: None,
            name: "MAX_RETRIES".to_string(),
            kind_value: KindValue::Primitive(Primitive::U8(Some(5))),
            span: (0, 0),
        }],
    )
}

// ── multi-schema fixtures ───────────────────────────────────────────────────
//
// Everything below needs more than one namespace, which is why none of it
// existed before. These are the shapes that broke in both generators.

/// A type named across schemas. The defect this pins: the reference emitted as
/// a bare name with nothing importing or qualifying it — valid syntax,
/// uncompilable output, shipped twice.
fn cross_schema() -> Fixture {
    Fixture::package(
        "cross_schema",
        vec![
            (
                "shared".to_string(),
                vec![structure("Shared", vec![field("value", "s32", false)])],
            ),
            (
                "api".to_string(),
                vec![
                    import("shared::Shared", None),
                    structure(
                        "Holder",
                        vec![
                            field("one", "Shared", false),
                            field("maybe", "Shared", true),
                            field("list", "Shared[]", false),
                        ],
                    ),
                ],
            ),
        ],
    )
}

/// `use shared::Shared as Aliased`. The name the *other* schema exports is
/// `Shared`; `Aliased` is only this schema's name for it, so a generator that
/// forgets the difference emits a path to something that doesn't exist.
fn import_alias() -> Fixture {
    Fixture::package(
        "import_alias",
        vec![
            (
                "shared".to_string(),
                vec![structure("Shared", vec![field("value", "s32", false)])],
            ),
            (
                "api".to_string(),
                vec![
                    import("shared::Shared", Some("Aliased")),
                    structure("Holder", vec![field("one", "Aliased", false)]),
                ],
            ),
        ],
    )
}

/// `use shared::*`, in the shape core actually freezes it:
/// `resolve_use_declaration` emits the `::*` marker **and** one leaf entry per
/// name the target schema declares. The marker binds nothing by itself, so a
/// generator that only saw it would have no name to resolve — this pins that
/// the leaves are what carry the binding, and that an unreferenced one
/// (`Unused`) produces no import.
fn import_glob() -> Fixture {
    Fixture::package(
        "import_glob",
        vec![
            (
                "shared".to_string(),
                vec![
                    structure("Shared", vec![field("value", "s32", false)]),
                    structure("Unused", vec![field("ignored", "bool", false)]),
                ],
            ),
            (
                "api".to_string(),
                vec![
                    import("shared::*", None),
                    import("shared::Shared", None),
                    import("shared::Unused", None),
                    structure("Holder", vec![field("one", "Shared", false)]),
                ],
            ),
        ],
    )
}

/// Nested namespaces in `lib` mode — the module tree each generator builds
/// (`pub mod` declarations for Rust, `index.ts` barrels for TypeScript), plus
/// a reference that crosses a directory boundary so the relative path between
/// two schemas is pinned too.
fn nested_namespace() -> Fixture {
    Fixture::package(
        "nested_namespace",
        vec![
            (
                "top".to_string(),
                vec![structure("Top", vec![field("value", "s32", false)])],
            ),
            (
                "group/first".to_string(),
                vec![
                    import("top::Top", None),
                    structure("First", vec![field("top", "Top", false)]),
                ],
            ),
            (
                "group/second".to_string(),
                vec![
                    import("group::first::First", None),
                    structure("Second", vec![field("first", "First", false)]),
                ],
            ),
        ],
    )
    .in_lib_mode()
}

/// `std` referenced from its own package instead of inlined: no `std/*` file
/// is emitted, the manifest declares the dependency, and the reference points
/// outside the package. The `std` schemas are built here like any other
/// fixture rather than pulled from the real standard library, so the corpus
/// stays independent of what std happens to contain.
fn external_std() -> Fixture {
    Fixture::package(
        "external_std",
        vec![
            (
                "std/http".to_string(),
                vec![structure("Request", vec![field("uri", "string", false)])],
            ),
            (
                "api".to_string(),
                vec![
                    import("std::http::Request", None),
                    structure("Call", vec![field("request", "Request", false)]),
                ],
            ),
        ],
    )
    .in_lib_mode()
    .with_external_std()
}
