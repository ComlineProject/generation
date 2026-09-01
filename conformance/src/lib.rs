//! The codegen conformance corpus.
//!
//! Each [`Fixture`] is one schema, as a `Vec<FrozenUnit>` built here rather than
//! parsed, so the corpus pins exactly what IR shape every generator must handle
//! — independent of the parser. `tests/conformance.rs` runs every generator over
//! every fixture and compares to the checked-in golden output; run it with
//! `CONFORMANCE_BLESS=1` to (re)generate the goldens.

use comline_core::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};
use comline_core::schema::ir::frozen::unit::{FrozenArgument, FrozenUnit};

/// One schema and its namespace.
pub struct Fixture {
    pub name: &'static str,
    pub namespace: &'static str,
    pub units: Vec<FrozenUnit>,
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
    }
}

// ── fixtures ────────────────────────────────────────────────────────────────

fn primitives() -> Fixture {
    Fixture {
        name: "primitives",
        namespace: "conformance",
        units: vec![structure(
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
    }
}

fn optional_and_arrays() -> Fixture {
    Fixture {
        name: "optional_and_arrays",
        namespace: "conformance",
        units: vec![structure(
            "Bag",
            vec![
                field("required", "string", false),
                field("maybe", "u32", true),
                field("many", "string[]", false),
                field("maybe_many", "u32[]", true),
            ],
        )],
    }
}

fn enum_basic() -> Fixture {
    Fixture {
        name: "enum_basic",
        namespace: "conformance",
        units: vec![FrozenUnit::Enum {
            docstring: None,
            name: "State".to_string(),
            variants: vec![variant("Pending"), variant("Active"), variant("Closed")],
            span: (0, 0),
        }],
    }
}

fn type_refs() -> Fixture {
    Fixture {
        name: "type_refs",
        namespace: "conformance",
        units: vec![
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
    }
}

fn protocol() -> Fixture {
    Fixture {
        name: "protocol",
        namespace: "conformance",
        units: vec![
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
                    function("notify", vec![arg("code", Primitive::U16(None))], None, vec![]),
                ],
                span: (0, 0),
            },
        ],
    }
}

fn constant() -> Fixture {
    Fixture {
        name: "constant",
        namespace: "conformance",
        units: vec![FrozenUnit::Constant {
            docstring: None,
            name: "MAX_RETRIES".to_string(),
            kind_value: KindValue::Primitive(Primitive::U8(Some(5))),
            span: (0, 0),
        }],
    }
}
