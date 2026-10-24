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

use comline_codegen::validation::ValidateOnDecode;
use comline_codegen::Mode;
use comline_core::schema::ir::frozen::types::{Literal, PrimitiveType, TypeRef};
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenArgument, FrozenUnit, Guarantee};

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
    /// Whether decoding runs the `@validators` checks.
    pub validate_on_decode: ValidateOnDecode,
    /// Whether connection protocols get async handlers and senders.
    pub async_api: bool,
}

impl Fixture {
    /// A single-schema `Code`-mode fixture — the common case.
    fn one(name: &'static str, namespace: &str, units: Vec<FrozenUnit>) -> Self {
        Self {
            name,
            schemas: vec![(namespace.to_string(), units)],
            mode: Mode::Code,
            external_std: false,
            validate_on_decode: ValidateOnDecode::Off,
            async_api: false,
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
            validate_on_decode: ValidateOnDecode::Off,
            async_api: false,
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

    fn with_validate_on_decode(mut self, on_decode: ValidateOnDecode) -> Self {
        self.validate_on_decode = on_decode;
        self
    }

    fn with_async(mut self) -> Self {
        self.async_api = true;
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
        error_shapes(),
        constant(),
        shapes(),
        // Shapes a single-schema fixture could not express at all.
        cross_schema(),
        import_alias(),
        import_glob(),
        nested_namespace(),
        external_std(),
        validators(),
        validators_on_decode(),
        // One per feature the rest don't reach - each generator's claim about
        // it is proven here (see `comline_codegen::features`).
        unions(),
        large_fixed_arrays_nested(),
        framing_jsonrpc(),
        handshake_none(),
        connections(),
        connections_async(),
        connections_jsonrpc(),
    ]
}

// ── builders ────────────────────────────────────────────────────────────────

/// The type a schema spells `written`: a primitive name, a named type, or
/// either followed by `[]` / `[N]`.
fn type_of(written: &str) -> TypeRef {
    if let Some(rest) = written.strip_suffix(']') {
        let (element, length) = rest.rsplit_once('[').expect("an array type");
        return TypeRef::Array {
            element: Box::new(type_of(element)),
            length: (!length.is_empty()).then(|| length.parse().expect("an array length")),
        };
    }
    match PrimitiveType::from_name(written) {
        Some(p) => TypeRef::Primitive(p),
        None => TypeRef::Named(written.to_string()),
    }
}

fn field(name: &str, ty: &str, optional: bool) -> FrozenUnit {
    FrozenUnit::Field {
        docstring: None,
        parameters: vec![],
        optional,
        name: name.to_string(),
        ty: type_of(ty),
        default: None,
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
    FrozenUnit::EnumVariant(name.to_string(), (0, 0))
}

fn arg(name: &str, prim: PrimitiveType) -> FrozenArgument {
    FrozenArgument {
        name: name.to_string(),
        ty: TypeRef::Primitive(prim),
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
        _return: ret.map(type_of),
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
        message_span: None,
        uninterpolated: vec![],
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
            // Declared, not just named: a generator must render a reference
            // to it, and a TypeScript `Record` must shadow the global one.
            structure(
                "Record",
                vec![field("id", "s32", false), field("name", "string", false)],
            ),
            error(0, "NotFound"),
            FrozenUnit::Protocol {
                docstring: "Conformance protocol".to_string(),
                parameters: vec![],
                name: "Service".to_string(),
                functions: vec![
                    function(
                        "lookup",
                        vec![arg("id", PrimitiveType::S32)],
                        Some("Record"),
                        vec![0], // ordinal of `NotFound`, above
                    ),
                    function("count", vec![], Some("u64"), vec![]),
                    function(
                        "notify",
                        vec![arg("code", PrimitiveType::U16)],
                        None,
                        vec![],
                    ),
                ],
                span: (0, 0),
            },
        ],
    )
}

/// The error-handling shapes `protocol()` cannot express: a function that
/// throws **several** errors, a protocol where **nothing** throws, and an
/// error whose `message` **interpolates a field**.
///
/// Each one pins a decision from the fault-channel work:
///
/// - `! Denied, TooLong` must produce one enum variant per declared error, in
///   *declaration* order rather than ordinal order — hence `Denied` is ordinal
///   2 and listed first.
/// - A protocol with no `!` anywhere makes even the `<Proto>Error` union
///   uninhabited, so it too becomes an `Infallible` alias rather than
///   `pub enum … {}`.
/// - `message = f"{self.limit} is the limit"` is what the generated `Display`
///   renders. `protocol()`'s errors have no fields, so without this the
///   interpolating branch of the message renderer is never exercised by the
///   corpus.
fn error_shapes() -> Fixture {
    let with_message =
        |ordinal: u16, name: &str, message: &str, fields: Vec<FrozenUnit>| FrozenUnit::Error {
            docstring: None,
            parameters: vec![],
            ordinal,
            imported_from: None,
            name: name.to_string(),
            message: message.to_string(),
            fields,
            span: Some((0, 0)),
            message_span: None,
            uninterpolated: vec![],
        };

    Fixture::one(
        "error_shapes",
        "conformance",
        vec![
            with_message(
                1,
                "TooLong",
                "{self.limit} is the limit",
                vec![field("limit", "u32", false)],
            ),
            with_message(2, "Denied", "denied", vec![]),
            FrozenUnit::Protocol {
                docstring: "Throws more than one error".to_string(),
                parameters: vec![],
                name: "Multi".to_string(),
                functions: vec![function(
                    "send",
                    vec![arg("body", PrimitiveType::String)],
                    Some("u64"),
                    vec![2, 1], // `! Denied, TooLong` — declaration order
                )],
                span: (0, 0),
            },
            FrozenUnit::Protocol {
                docstring: "Nothing here can fail".to_string(),
                parameters: vec![],
                name: "Quiet".to_string(),
                functions: vec![
                    function("ping", vec![], Some("u64"), vec![]),
                    function("hush", vec![], None, vec![]),
                ],
                span: (0, 0),
            },
        ],
    )
}

/// A `const` declaration.
///
/// Both goldens were **empty**, and saying so in this comment is what
/// eventually got it fixed: neither generator handled
/// `FrozenUnit::Constant`, so a schema declaring a constant produced a file
/// with nothing in it. An empty golden is indistinguishable from a correct
/// one at a glance, and a golden pins missing behaviour exactly as silently
/// as it pins present behaviour.
///
/// Closed by comline-rust#42 and comline-typescript#35, which needed
/// core#92 first: the declared type was being inferred from the literal, so
/// this fixture's `U8(Some(5))` could not be told from a `u64` in a real
/// build, and emitting it would have written `u64`.
///
/// The Rust golden also lost `use serde::{Deserialize, Serialize};` - it had
/// nothing to derive on, which was an unused import in generated code. The
/// same shape in `comline-rust`'s generated `comline_std` is what made
/// clippy fail there, and no longer does.
fn constant() -> Fixture {
    Fixture::one(
        "constant",
        "conformance",
        vec![FrozenUnit::Constant {
            docstring: None,
            name: "MAX_RETRIES".to_string(),
            ty: TypeRef::Primitive(PrimitiveType::U8),
            value: Some(Literal::Integer(5)),
            span: (0, 0),
        }],
    )
}

/// Exact float widths and fixed arrays - both reached generators as text
/// that had lost them (`"float"`, `"T[]"`) until core froze types
/// structurally. `u8[64]` is past serde's derive limit, so Rust routes it
/// through `serde_big_array`; `u16[3][]` nests a small fixed array in a
/// dynamic one.
fn shapes() -> Fixture {
    Fixture::one(
        "shapes",
        "conformance",
        vec![
            structure(
                "Sample",
                vec![
                    field("ratio", "f32", false),
                    field("precise", "f64", false),
                    field("rgba", "u8[4]", false),
                    field("digest", "u8[64]", false),
                    field("rows", "u16[3][]", false),
                    field("maybe_rgba", "u8[4]", true),
                ],
            ),
            FrozenUnit::Constant {
                docstring: None,
                name: "SCALE".to_string(),
                ty: TypeRef::Primitive(PrimitiveType::F32),
                value: Some(Literal::Integer(2)),
                span: (0, 0),
            },
        ],
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

// ── validators ──────────────────────────────────────────────────────────────

/// Every way a check reaches a value: a field's own `@validators` (std's
/// `StringBounds`, bound through a constant, and a local validator over a
/// number), a `type` alias carrying checks into a field, an optional field,
/// an array of the alias, a struct nested in another, and a function
/// argument checked through the alias. Written as IDL rather than built by
/// hand: the condition tree and the alias annotations are core's to freeze.
const VALIDATED: &str = r#"
use std::validators::StringBounds

const MIN_HANDLE: u32 = 2

validator Range for u32 {
    min: u32 = 0
    max: u32 = 100

    validate {
        assert(value >= params.min and value <= params.max,
               f"{value.name} must be between {params.min} and {params.max}")
    }
}

@validators = [StringBounds(min_chars = MIN_HANDLE, max_chars = 8)]
type Handle = string

struct Profile {
    handle: Handle
    @validators = [Range(max = 120)]
    age: u32
    optional nickname: Handle
    friends: Handle[]
}

struct Post {
    author: Profile
    @validators = [StringBounds(min_chars = 1)]
    body: string
}

protocol Social {
    function rename(to: Handle) -> Handle;
}
"#;

fn validated_schemas() -> Vec<(String, Vec<FrozenUnit>)> {
    use comline_core::package::stdlib;
    use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
    use comline_core::schema::ir::compiler::Compile;
    let (_, std_validators) = stdlib::schemas()
        .find(|(namespace, _)| namespace.join("/") == "std/validators")
        .expect("std ships validators");
    vec![
        (
            "std/validators".to_string(),
            IncrementalInterpreter::from_source(std_validators),
        ),
        (
            "conformance".to_string(),
            IncrementalInterpreter::from_source(VALIDATED),
        ),
    ]
}

fn validators() -> Fixture {
    Fixture::package("validators", validated_schemas())
}

/// The same package, with decoding running the checks.
fn validators_on_decode() -> Fixture {
    Fixture::package("validators_on_decode", validated_schemas())
        .with_validate_on_decode(ValidateOnDecode::Collect)
}

/// A union, as a field and through an alias. A generator either renders it
/// or refuses the schema - never something else.
fn unions() -> Fixture {
    let either = TypeRef::Union(vec![type_of("string"), type_of("u32")]);
    Fixture::one(
        "unions",
        "conformance",
        vec![
            FrozenUnit::TypeAlias {
                docstring: None,
                parameters: vec![],
                name: "Id".to_string(),
                target: either.clone(),
                span: (0, 0),
            },
            structure(
                "Lookup",
                vec![FrozenUnit::Field {
                    docstring: None,
                    parameters: vec![],
                    optional: false,
                    name: "key".to_string(),
                    ty: either,
                    default: None,
                    span: (0, 0),
                }],
            ),
        ],
    )
}

/// A fixed array over 32 elements where it isn't a field's own,
/// non-optional type: optional, and inside another array.
fn large_fixed_arrays_nested() -> Fixture {
    Fixture::one(
        "large_fixed_arrays_nested",
        "conformance",
        vec![structure(
            "Blocks",
            vec![field("maybe", "u8[64]", true), field("many", "u8[64][]", false)],
        )],
    )
}

/// A protocol that picks JSON-RPC framing with `@framing`.
fn framing_jsonrpc() -> Fixture {
    Fixture::one(
        "framing_jsonrpc",
        "conformance",
        vec![FrozenUnit::Protocol {
            docstring: "Speaks JSON-RPC".to_string(),
            parameters: vec![FrozenUnit::Property {
                name: "framing".to_string(),
                value: ConfigValue::Str("jsonrpc".to_string()),
            }],
            name: "Rpc".to_string(),
            functions: vec![function("ping", vec![arg("n", PrimitiveType::U32)], Some("u32"), vec![])],
            span: (0, 0),
        }],
    )
}

/// A protocol that runs no handshake, marked `@unverified` with the set core
/// freezes for it.
fn handshake_none() -> Fixture {
    Fixture::one(
        "handshake_none",
        "conformance",
        vec![FrozenUnit::Protocol {
            docstring: "Talks to a peer with no handshake".to_string(),
            parameters: vec![
                FrozenUnit::Property {
                    name: "handshake".to_string(),
                    value: ConfigValue::Str("none".to_string()),
                },
                FrozenUnit::Property {
                    name: "unverified".to_string(),
                    value: ConfigValue::Str("the gateway predates Comline".to_string()),
                },
                FrozenUnit::Unverified(vec![
                    Guarantee::SchemaAgreement,
                    Guarantee::EncodingAgreement,
                ]),
            ],
            name: "Gateway".to_string(),
            functions: vec![function("ping", vec![arg("n", PrimitiveType::U32)], Some("u32"), vec![])],
            span: (0, 0),
        }],
    )
}

/// Two protocols on one connection: `Chat`, which the accepting side
/// provides over `datagram@2`, and `Events`, which the dialling side does.
/// `connections`, with `async`: the same protocols, generated for an async
/// driver where a generator has one.
fn connections_async() -> Fixture {
    Fixture { name: "connections_async", ..connections() }.with_async()
}

/// [`connections`] over JSON-RPC: `Chat` names only its framing and joins
/// the connection `Events` is on; `Events` qualifies its method names.
fn connections_jsonrpc() -> Fixture {
    let jsonrpc = FrozenUnit::Property { name: "framing".to_string(), value: ConfigValue::Str("jsonrpc".to_string()) };
    let qualified = FrozenUnit::Property {
        name: "framing".to_string(),
        value: ConfigValue::Dict(vec![
            ("name".to_string(), ConfigValue::Str("jsonrpc".to_string())),
            ("method_names".to_string(), ConfigValue::Name("qualified".to_string())),
        ]),
    };
    let mut fixture = Fixture { name: "connections_jsonrpc", ..connections() };
    for (_, units) in &mut fixture.schemas {
        for unit in units {
            if let FrozenUnit::Protocol { name, parameters, .. } = unit {
                let framing = if name == "Chat" { jsonrpc.clone() } else { qualified.clone() };
                for p in parameters.iter_mut() {
                    if matches!(p, FrozenUnit::Property { name, .. } if name == "framing") {
                        *p = framing.clone();
                    }
                }
            }
        }
    }
    fixture
}

fn connections() -> Fixture {
    let datagram2 = || FrozenUnit::Property {
        name: "framing".to_string(),
        value: ConfigValue::Str("datagram@2".to_string()),
    };
    Fixture::one(
        "connections",
        "conformance",
        vec![
            FrozenUnit::Protocol {
                docstring: "What the server provides".to_string(),
                parameters: vec![datagram2()],
                name: "Chat".to_string(),
                functions: vec![function("send", vec![arg("n", PrimitiveType::U32)], Some("u32"), vec![])],
                span: (0, 0),
            },
            FrozenUnit::Protocol {
                docstring: "What the client provides".to_string(),
                parameters: vec![
                    FrozenUnit::Property {
                        name: "provider".to_string(),
                        value: ConfigValue::Name("Client".to_string()),
                    },
                    datagram2(),
                ],
                name: "Events".to_string(),
                functions: vec![
                    function("shown", vec![arg("n", PrimitiveType::U32)], Some("bool"), vec![]),
                    function("ping", vec![], None, vec![]),
                ],
                span: (0, 0),
            },
        ],
    )
}
