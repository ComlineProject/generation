//! Which language features a schema uses, and which of them a generator
//! supports.
//!
//! Three pieces:
//! - the vocabulary: [`Feature`], one stable id per feature;
//! - the one detector: [`used`], what a package (or a single schema) uses,
//!   and where;
//! - each generator's claims: a `SUPPORT` table of [`Support`] per feature,
//!   handed to [`crate::Registry::register`].
//!
//! The claims are proven, not trusted: `comline-conformance` checks every
//! one against the corpus (a supported feature generates and matches its
//! golden; an ignored one generates exactly what it would without it; an
//! unsupported one is refused). The same detector and tables drive the
//! CLI's pre-generation check, `comline targets --features`, the editor's
//! warnings, and the rendered matrix ([`markdown`], [`json`]).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use comline_core::schema::idl::annotations::{
    DEFAULT_FORMAT, FORMAT_JSON, FORMAT_MSGPACK, FORMAT_PACKED, FRAMING_JSONRPC, HANDSHAKE_NONE,
};
use comline_core::schema::ir::frozen::types::{PrimitiveType, TypeRef};
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};
use eyre::{eyre, Result};

use crate::validation::ValidateOnDecode;
use crate::versions::{LangVersion, Versions};
use crate::{GenerateRequest, Mode};

/// A language feature a generator may or may not implement.
///
/// Its [`id`](Feature::id) is stable: it's what the report, the docs and the
/// editor's diagnostics name it by. A new variant goes at the end of
/// [`Feature::ALL`], and every generator then has to state its support for
/// it (conformance fails until it does).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feature {
    Structs,
    Enums,
    OptionalFields,
    Arrays,
    FixedArrays,
    LargeFixedArraysNested,
    Unions,
    TypeAliases,
    Constants,
    Protocols,
    Errors,
    Throws,
    Validators,
    Imports,
    NestedNamespaces,
    LibMode,
    ExternalStd,
    ValidateOnDecode,
    FramingJsonRpc,
    HandshakeNone,
    FramingDatagram3,
    HandshakeComline2,
    Connections,
    Async,
    FormatPacked,
    FormatJson,
    FormatMsgPack,
    Bounds,
    Chars,
}

impl Feature {
    /// Every feature, in report order.
    pub const ALL: &'static [Feature] = &[
        Feature::Structs,
        Feature::Enums,
        Feature::OptionalFields,
        Feature::Arrays,
        Feature::FixedArrays,
        Feature::LargeFixedArraysNested,
        Feature::Unions,
        Feature::TypeAliases,
        Feature::Constants,
        Feature::Protocols,
        Feature::Errors,
        Feature::Throws,
        Feature::Validators,
        Feature::Imports,
        Feature::NestedNamespaces,
        Feature::LibMode,
        Feature::ExternalStd,
        Feature::ValidateOnDecode,
        Feature::FramingJsonRpc,
        Feature::HandshakeNone,
        Feature::FramingDatagram3,
        Feature::HandshakeComline2,
        Feature::Connections,
        Feature::Async,
        Feature::FormatPacked,
        Feature::FormatJson,
        Feature::FormatMsgPack,
        Feature::Bounds,
        Feature::Chars,
    ];

    /// The stable id: `unions`, `validate-on-decode`.
    pub fn id(self) -> &'static str {
        match self {
            Feature::Structs => "structs",
            Feature::Enums => "enums",
            Feature::OptionalFields => "optional-fields",
            Feature::Arrays => "arrays",
            Feature::FixedArrays => "fixed-arrays",
            Feature::LargeFixedArraysNested => "large-fixed-arrays-nested",
            Feature::Unions => "unions",
            Feature::TypeAliases => "type-aliases",
            Feature::Constants => "constants",
            Feature::Protocols => "protocols",
            Feature::Errors => "errors",
            Feature::Throws => "throws",
            Feature::Validators => "validators",
            Feature::Imports => "imports",
            Feature::NestedNamespaces => "nested-namespaces",
            Feature::LibMode => "lib-mode",
            Feature::ExternalStd => "external-std",
            Feature::ValidateOnDecode => "validate-on-decode",
            Feature::FramingJsonRpc => "framing-jsonrpc",
            Feature::HandshakeNone => "handshake-none",
            Feature::FramingDatagram3 => "framing-datagram3",
            Feature::HandshakeComline2 => "handshake-comline2",
            Feature::Connections => "connections",
            Feature::Async => "async",
            Feature::FormatPacked => "format-packed",
            Feature::FormatJson => "format-json",
            Feature::FormatMsgPack => "format-msgpack",
            Feature::Bounds => "bounds",
            Feature::Chars => "chars",
        }
    }

    /// The feature an [`id`](Feature::id) names.
    pub fn from_id(id: &str) -> Option<Feature> {
        Feature::ALL.iter().copied().find(|f| f.id() == id)
    }

    /// A short name for messages and tables: "unions", "fixed arrays".
    pub fn title(self) -> &'static str {
        match self {
            Feature::Structs => "structs",
            Feature::Enums => "enums",
            Feature::OptionalFields => "optional fields",
            Feature::Arrays => "arrays",
            Feature::FixedArrays => "fixed-length arrays",
            Feature::LargeFixedArraysNested => "fixed arrays over 32 elements, nested",
            Feature::Unions => "unions",
            Feature::TypeAliases => "type aliases",
            Feature::Constants => "constants",
            Feature::Protocols => "protocols",
            Feature::Errors => "errors",
            Feature::Throws => "functions that throw",
            Feature::Validators => "validators",
            Feature::Imports => "imports between schemas",
            Feature::NestedNamespaces => "nested namespaces",
            Feature::LibMode => "`lib` mode",
            Feature::ExternalStd => "`external_std`",
            Feature::ValidateOnDecode => "`validate_on_decode`",
            Feature::FramingJsonRpc => "JSON-RPC framing",
            Feature::HandshakeNone => "no handshake",
            Feature::FramingDatagram3 => "`datagram@3` framing",
            Feature::HandshakeComline2 => "`comline@2` handshake",
            Feature::Connections => "connections",
            Feature::Async => "`async`",
            Feature::FormatPacked => "the `packed@1` format",
            Feature::FormatJson => "the `json@1` format",
            Feature::FormatMsgPack => "the `msgpack@1` format",
            Feature::Bounds => "bounds",
            Feature::Chars => "single characters",
        }
    }

    /// What using the feature looks like.
    pub fn description(self) -> &'static str {
        match self {
            Feature::Structs => "`struct` declarations",
            Feature::Enums => "`enum` declarations",
            Feature::OptionalFields => "fields marked `optional`",
            Feature::Arrays => "variable-length arrays, `T[]`",
            Feature::FixedArrays => "fixed-length arrays, `T[N]`",
            Feature::LargeFixedArraysNested => {
                "a `T[N]` with N over 32 anywhere but a field's own, non-optional type: \
                 inside another array, optional, an alias, an argument or a return"
            }
            Feature::Unions => "`union(A B ...)` types",
            Feature::TypeAliases => "`type Name = ...` declarations",
            Feature::Constants => "`const` declarations",
            Feature::Protocols => "`protocol` declarations and their functions",
            Feature::Errors => "`error` declarations",
            Feature::Throws => "functions declared with `! Error`",
            Feature::Validators => "`@validators` on fields and type aliases, checked in generated code",
            Feature::Imports => "`use` of another schema's declarations",
            Feature::NestedNamespaces => "schemas in subdirectories (`chat::admin`)",
            Feature::LibMode => "`--mode lib`: a buildable package, not bare source files",
            Feature::ExternalStd => "`[generate] external_std`: `std` referenced as its own package",
            Feature::ValidateOnDecode => "`[generate] validate_on_decode`: decoding runs the checks",
            Feature::FramingJsonRpc => "`@framing = \"jsonrpc\"` (or as the package default)",
            Feature::HandshakeNone => "`@handshake = \"none\"` (or as the package default)",
            Feature::FramingDatagram3 => {
                "`@framing = \"datagram@3\"` (or as the package default): calls addressed by wire numbers"
            }
            Feature::HandshakeComline2 => {
                "`@handshake = \"comline@2\"` (or as the package default): compatibility checked by each \
                 version's facts, calls the peer lacks failing locally"
            }
            Feature::Connections => {
                "protocols on a connection, with calls in both directions: `datagram@3` (the default) \
                 or `datagram@2`, or `@provider = Client` / `Multiple`"
            }
            Feature::Async => "`[generate] async`: connection protocols get async handlers and senders",
            Feature::FormatPacked => "`wire.format = \"packed@1\"`, the default: fixed-width fields, no tags",
            Feature::FormatJson => "`wire.format = \"json@1\"`",
            Feature::FormatMsgPack => "`wire.format = \"msgpack@1\"`",
            Feature::Bounds => {
                "a maximum or exact length on a string or list (`char[max 64]`, `char[8]`, `T[max N]`), \
                 or a depth limit (`@max_depth`): checked while decoding and encoding"
            }
            Feature::Chars => "`char`: one Unicode scalar value",
        }
    }
}

/// How far a generator implements a feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Implemented.
    Supported,
    /// Implemented with a limit, which the [`Support::note`] states.
    Partial,
    /// Generation succeeds, and the output is what it would be without the
    /// feature: the types are right, the feature has no effect.
    Ignored,
    /// Generation refuses a schema that uses it.
    Unsupported,
}

impl State {
    pub fn id(self) -> &'static str {
        match self {
            State::Supported => "supported",
            State::Partial => "partial",
            State::Ignored => "ignored",
            State::Unsupported => "unsupported",
        }
    }
}

/// One generator's support for one feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Support {
    pub state: State,
    /// What's missing or limited - required for `Partial`.
    pub note: Option<&'static str>,
    /// Where the rest is tracked: `core#96`.
    pub tracking: Option<&'static str>,
    /// The version this support starts at; below it the feature is
    /// unsupported. `None`: from the generator's floor.
    pub since: Option<Since>,
}

/// Where a feature's support starts: a version, which has to be one of the
/// generator's boundaries, and why it can't be earlier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Since {
    pub version: &'static str,
    /// "`async fn` in traits".
    pub why: &'static str,
}

impl Support {
    pub const fn supported() -> Self {
        Support {
            state: State::Supported,
            note: None,
            tracking: None,
            since: None,
        }
    }
    pub const fn partial(note: &'static str) -> Self {
        Support {
            state: State::Partial,
            note: Some(note),
            tracking: None,
            since: None,
        }
    }
    pub const fn ignored(note: &'static str) -> Self {
        Support {
            state: State::Ignored,
            note: Some(note),
            tracking: None,
            since: None,
        }
    }
    pub const fn unsupported(note: &'static str) -> Self {
        Support {
            state: State::Unsupported,
            note: Some(note),
            tracking: None,
            since: None,
        }
    }
    pub const fn tracked(mut self, tracking: &'static str) -> Self {
        self.tracking = Some(tracking);
        self
    }
    /// Only from `version` on, because of `why`: below it the feature is
    /// unsupported, and the refusal names the version to declare.
    pub const fn from(mut self, version: &'static str, why: &'static str) -> Self {
        self.since = Some(Since { version, why });
        self
    }

    /// Whether this is a gate that `version` is below.
    pub fn is_gated_at(&self, version: &LangVersion) -> bool {
        self.since
            .and_then(|since| LangVersion::parse(since.version))
            .is_some_and(|gate| *version < gate)
    }
}

/// A generator's whole table: one entry per [`Feature`].
pub type SupportTable = &'static [(Feature, Support)];

/// `feature`'s entry in `table`, as claimed: at the newest version, gates
/// aside. A feature missing from it counts as unsupported - conformance fails
/// a table with a gap.
pub fn support_for(table: SupportTable, feature: Feature) -> Support {
    table
        .iter()
        .find(|(f, _)| *f == feature)
        .map(|(_, s)| *s)
        .unwrap_or(Support::unsupported("not stated by the generator"))
}

/// `feature`'s support when generating for `version`: below its gate,
/// unsupported, saying why.
pub fn support_at(table: SupportTable, feature: Feature, version: &LangVersion) -> Support {
    let claim = support_for(table, feature);
    match claim.since {
        Some(since) if claim.is_gated_at(version) => Support {
            state: State::Unsupported,
            note: Some(since.why),
            ..claim
        },
        _ => claim,
    }
}

/// The request knobs that are features themselves.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub mode: Option<Mode>,
    pub external_std: bool,
    pub validate_on_decode: ValidateOnDecode,
    /// The package's `wire.format`, canonical; `None` is the default.
    pub wire_format: Option<String>,
    pub async_api: bool,
    /// The mappings, for what their framings state.
    pub encodings: BTreeMap<String, crate::protocol::EncodingMapping>,
}

impl Options {
    pub fn of(req: &GenerateRequest) -> Self {
        Options {
            mode: Some(req.mode),
            external_std: req.external_std,
            validate_on_decode: req.validate_on_decode,
            wire_format: Some(req.wire_format.clone()),
            async_api: req.async_api,
            encodings: req.encodings.clone(),
        }
    }
}

/// One use of a feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureUse {
    pub feature: Feature,
    /// The schema it's in; empty for a request option.
    pub namespace: String,
    /// What uses it, for messages: "struct 'User', field 'tags'".
    pub owner: String,
    /// The names, outermost first, of the unit that uses it within its
    /// schema: `["User", "tags"]`, `["Chat", "send", "x"]`. Empty for the
    /// schema itself, an import, or a request option.
    pub unit: Vec<String>,
    /// Where, in the schema's source; `None` when unknown or for an option.
    pub span: Option<(usize, usize)>,
}

/// What uses a feature, as [`FeatureUse::owner`] and [`FeatureUse::unit`]:
/// the phrase for a message and the path of names, built together as the
/// walk goes down, so neither is read off the other.
#[derive(Debug, Clone)]
struct Owner {
    text: String,
    unit: Vec<String>,
}

impl Owner {
    /// A declaration in the schema: `struct 'User'`, in the unit `User`.
    fn declared(kind: &str, name: &str) -> Self {
        Owner {
            text: format!("{kind} '{name}'"),
            unit: vec![name.to_string()],
        }
    }

    /// Something in the schema that's no unit: an import, the schema itself.
    fn schema(text: String) -> Self {
        Owner {
            text,
            unit: Vec::new(),
        }
    }

    /// A unit inside this one: `, field 'tags'`, in the unit `tags` of it.
    fn child(&self, kind: &str, name: &str) -> Self {
        let mut unit = self.unit.clone();
        unit.push(name.to_string());
        Owner {
            text: format!("{}, {kind} '{name}'", self.text),
            unit,
        }
    }

    /// A part of this unit that's no unit of its own (a validator is a
    /// reference, `return` a position): the phrase says it, the path doesn't.
    fn note(&self, suffix: &str) -> Self {
        Owner {
            text: format!("{}, {suffix}", self.text),
            unit: self.unit.clone(),
        }
    }
}

/// Every use of every feature in `schemas` under `options`, in schema order.
pub fn used(schemas: &[(String, Vec<FrozenUnit>)], options: &Options) -> Vec<FeatureUse> {
    let mut uses = Vec::new();
    let mut validators = false;
    let mut connections = false;
    // Whether a protocol encodes with the package's format: JSON-RPC
    // carries JSON whatever the package says.
    let mut formatted = false;

    for (namespace, units) in schemas {
        let mut push = |feature, owner: Owner, span: Option<(usize, usize)>| {
            uses.push(FeatureUse {
                feature,
                namespace: namespace.clone(),
                owner: owner.text,
                unit: owner.unit,
                span: known(span),
            });
        };

        if namespace.contains('/') || namespace.contains("::") {
            push(
                Feature::NestedNamespaces,
                Owner::schema(format!("schema '{namespace}'")),
                None,
            );
        }

        for unit in units {
            match unit {
                FrozenUnit::Import(path, _, span) => {
                    push(
                        Feature::Imports,
                        Owner::schema(format!("use {path}")),
                        Some(*span),
                    );
                }
                FrozenUnit::Struct {
                    name,
                    fields,
                    parameters,
                    span,
                    ..
                } => {
                    let owner = Owner::declared("struct", name);
                    push(Feature::Structs, owner.clone(), Some(*span));
                    if has_property(parameters, "max_depth") {
                        push(Feature::Bounds, owner.note("`@max_depth`"), Some(*span));
                    }
                    fields_of(&owner, fields, &mut push, &mut validators);
                }
                FrozenUnit::Error {
                    name, fields, span, ..
                } => {
                    let owner = Owner::declared("error", name);
                    push(Feature::Errors, owner.clone(), *span);
                    fields_of(&owner, fields, &mut push, &mut validators);
                }
                FrozenUnit::Enum { name, span, .. } => {
                    push(Feature::Enums, Owner::declared("enum", name), Some(*span));
                }
                FrozenUnit::TypeAlias {
                    name,
                    target,
                    parameters,
                    span,
                    ..
                } => {
                    let owner = Owner::declared("type alias", name);
                    push(Feature::TypeAliases, owner.clone(), Some(*span));
                    types_of(&owner, target, false, Some(*span), &mut push);
                    validators |= validator_refs(&owner, parameters, &mut push);
                }
                FrozenUnit::Constant { name, ty, span, .. } => {
                    let owner = Owner::declared("const", name);
                    push(Feature::Constants, owner.clone(), Some(*span));
                    types_of(&owner, ty, false, Some(*span), &mut push);
                }
                FrozenUnit::Protocol {
                    name,
                    functions,
                    parameters,
                    span,
                    ..
                } => {
                    let owner = Owner::declared("protocol", name);
                    push(Feature::Protocols, owner.clone(), Some(*span));
                    // Core has already applied the package's `wire.framing`.
                    if crate::connections::framing(parameters).name == FRAMING_JSONRPC {
                        push(Feature::FramingJsonRpc, owner.clone(), Some(*span));
                    }
                    let framing = crate::connections::framing_name(parameters);
                    formatted |= !crate::encodings::writes_json(&options.encodings, &framing);
                    // Core has already applied the package's default here.
                    if annotation(parameters, "handshake") == Some(HANDSHAKE_NONE) {
                        push(Feature::HandshakeNone, owner.clone(), Some(*span));
                    }
                    if framing == crate::facts::FRAMING_DATAGRAM3 {
                        push(Feature::FramingDatagram3, owner.clone(), Some(*span));
                    }
                    if crate::connections::handshake_name(parameters)
                        == crate::facts::HANDSHAKE_COMLINE2
                    {
                        push(Feature::HandshakeComline2, owner.clone(), Some(*span));
                    }
                    if crate::connections::on_connection(schemas, parameters) {
                        push(Feature::Connections, owner.clone(), Some(*span));
                        connections = true;
                    }
                    for function in functions {
                        let FrozenUnit::Function {
                            name: f,
                            arguments,
                            _return,
                            throws,
                            span,
                            ..
                        } = function
                        else {
                            continue;
                        };
                        let owner = owner.child("function", f);
                        if !throws.is_empty() {
                            push(Feature::Throws, owner.clone(), Some(*span));
                        }
                        for argument in arguments {
                            let owner = owner.child("argument", &argument.name);
                            types_of(&owner, &argument.ty, false, Some(argument.span), &mut push);
                        }
                        if let Some(ret) = _return {
                            types_of(&owner.note("return"), ret, false, Some(*span), &mut push);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let mut option = |feature, owner: &str| {
        uses.push(FeatureUse {
            feature,
            namespace: String::new(),
            owner: owner.to_string(),
            unit: Vec::new(),
            span: None,
        });
    };
    if options.mode == Some(Mode::Lib) {
        option(Feature::LibMode, "the generation request (`lib` mode)");
    }
    if options.external_std {
        option(
            Feature::ExternalStd,
            "the generation request (`external_std`)",
        );
    }
    // Only a schema with checks to run makes the option mean anything.
    if validators && options.validate_on_decode != ValidateOnDecode::Off {
        option(
            Feature::ValidateOnDecode,
            "the generation request (`validate_on_decode`)",
        );
    }
    // Only connection protocols have an async form.
    if connections && options.async_api {
        option(Feature::Async, "the generation request (`async`)");
    }
    if formatted {
        let format = options.wire_format.as_deref().unwrap_or(DEFAULT_FORMAT);
        let feature = match format {
            FORMAT_JSON => Some(Feature::FormatJson),
            FORMAT_MSGPACK => Some(Feature::FormatMsgPack),
            FORMAT_PACKED => Some(Feature::FormatPacked),
            _ => None,
        };
        if let Some(feature) = feature {
            option(feature, "the package's `wire.format`");
        }
    }

    uses
}

/// `span`, unless it's the `(0, 0)` of a unit frozen without one.
fn known(span: Option<(usize, usize)>) -> Option<(usize, usize)> {
    span.filter(|s| *s != (0, 0))
}

fn fields_of(
    owner: &Owner,
    fields: &[FrozenUnit],
    push: &mut impl FnMut(Feature, Owner, Option<(usize, usize)>),
    validators: &mut bool,
) {
    for field in fields {
        let FrozenUnit::Field {
            name,
            ty,
            optional,
            parameters,
            span,
            ..
        } = field
        else {
            continue;
        };
        let owner = owner.child("field", name);
        if *optional {
            push(Feature::OptionalFields, owner.clone(), Some(*span));
        }
        // A large fixed array is fine as a field's own, non-optional type.
        types_of(&owner, ty, !*optional, Some(*span), push);
        *validators |= validator_refs(&owner, parameters, push);
    }
}

/// The type features `ty` uses. `own_field` is whether `ty` is a struct or
/// error field's own, non-optional type - the one place a large fixed array
/// isn't "nested".
fn types_of(
    owner: &Owner,
    ty: &TypeRef,
    own_field: bool,
    span: Option<(usize, usize)>,
    push: &mut impl FnMut(Feature, Owner, Option<(usize, usize)>),
) {
    match ty {
        // An array of `char` is a string: bounded, or of an exact length.
        TypeRef::Array {
            element,
            length,
            max,
        } if **element == TypeRef::Primitive(PrimitiveType::Char) => {
            if length.is_some() || max.is_some() {
                push(Feature::Bounds, owner.clone(), span);
            }
        }
        TypeRef::Array {
            element,
            length,
            max,
        } => {
            if max.is_some() {
                push(Feature::Bounds, owner.clone(), span);
            }
            match length {
                None => push(Feature::Arrays, owner.clone(), span),
                Some(n) => {
                    push(Feature::FixedArrays, owner.clone(), span);
                    if *n > 32 && !own_field {
                        push(Feature::LargeFixedArraysNested, owner.clone(), span);
                    }
                }
            }
            types_of(owner, element, false, span, push);
        }
        TypeRef::Union(members) => {
            push(Feature::Unions, owner.clone(), span);
            for member in members {
                types_of(owner, member, false, span, push);
            }
        }
        TypeRef::Primitive(PrimitiveType::Char) => push(Feature::Chars, owner.clone(), span),
        TypeRef::Primitive(_) | TypeRef::Named(_) | TypeRef::Unit => {}
    }
}

/// Reports each `@validators` call in `parameters`; whether there was one.
fn validator_refs(
    owner: &Owner,
    parameters: &[FrozenUnit],
    push: &mut impl FnMut(Feature, Owner, Option<(usize, usize)>),
) -> bool {
    let mut any = false;
    for parameter in parameters {
        if let FrozenUnit::ValidatorRef { name, span, .. } = parameter {
            push(
                Feature::Validators,
                owner.note(&format!("validator '{name}'")),
                Some(*span),
            );
            any = true;
        }
    }
    any
}

/// Whether `parameters` set `@key`, whatever its value.
fn has_property(parameters: &[FrozenUnit], key: &str) -> bool {
    parameters
        .iter()
        .any(|p| matches!(p, FrozenUnit::Property { name, .. } if name == key))
}

/// The text of a scalar `@key = value` annotation.
fn annotation<'a>(parameters: &'a [FrozenUnit], key: &str) -> Option<&'a str> {
    parameters.iter().find_map(|p| match p {
        FrozenUnit::Property { name, value } if name == key => match value {
            ConfigValue::Str(s) | ConfigValue::Name(s) | ConfigValue::Path(s) => Some(s.as_str()),
            _ => None,
        },
        _ => None,
    })
}

/// Refuses `req` when it uses a feature `support` marks unsupported, naming
/// every such use - not only the first. A generator calls this before
/// generating anything.
pub fn refuse_unsupported(
    generator: &str,
    req: &GenerateRequest,
    support: SupportTable,
) -> Result<()> {
    let version = req.version()?;
    let uses = used(&req.schemas, &Options::of(req));
    let unsupported = unsupported_uses(&uses, support, &version);
    if unsupported.is_empty() {
        return Ok(());
    }
    Err(eyre!(refusal(generator, &unsupported, support, &version)))
}

/// The uses in `uses` whose feature `support` marks unsupported at `version`.
pub fn unsupported_uses<'u>(
    uses: &'u [FeatureUse],
    support: SupportTable,
    version: &LangVersion,
) -> Vec<&'u FeatureUse> {
    uses.iter()
        .filter(|u| support_at(support, u.feature, version).state == State::Unsupported)
        .collect()
}

/// The message for a refused request: one line per feature, then its uses.
/// `generator` is the language's name, which a gated feature's line uses to
/// name the version to declare (`rust#1.75.0`).
pub fn refusal(
    generator: &str,
    unsupported: &[&FeatureUse],
    support: SupportTable,
    version: &LangVersion,
) -> String {
    let mut by_feature: BTreeMap<Feature, Vec<&FeatureUse>> = BTreeMap::new();
    for u in unsupported {
        by_feature.entry(u.feature).or_default().push(u);
    }
    let mut message = String::new();
    for (feature, uses) in by_feature {
        let s = support_at(support, feature, version);
        match s.since.filter(|_| s.is_gated_at(version)) {
            Some(since) => {
                let _ = write!(
                    message,
                    "the {generator} generator supports {} ({}) only from `{generator}#{}`, and this target declares \
                     `{generator}#{version}`: {} - declare `{generator}#{}` or later",
                    feature.title(),
                    feature.id(),
                    since.version,
                    since.why,
                    since.version
                );
            }
            None => {
                let _ = write!(
                    message,
                    "the {generator} generator doesn't support {} ({})",
                    feature.title(),
                    feature.id()
                );
                if let Some(note) = s.note {
                    let _ = write!(message, ": {note}");
                }
            }
        }
        if let Some(tracking) = s.tracking {
            let _ = write!(message, " - {tracking} tracks it");
        }
        message.push('\n');
        for u in uses {
            match u.namespace.is_empty() {
                true => {
                    let _ = writeln!(message, "  - {}", u.owner);
                }
                false => {
                    let _ = writeln!(message, "  - schema '{}', {}", u.namespace, u.owner);
                }
            }
        }
    }
    message.trim_end().to_string()
}

/// The fixture `schemas` and `options` with `feature` taken out, for proving
/// an `Ignored` claim: output with and without it must be identical. `None`
/// for a feature that can't be taken out without changing what the schema
/// means - such a feature can't be `Ignored`.
pub fn strip(
    feature: Feature,
    schemas: &[(String, Vec<FrozenUnit>)],
    options: &Options,
) -> Option<(Vec<(String, Vec<FrozenUnit>)>, Options)> {
    let without_refs = |parameters: &[FrozenUnit]| -> Vec<FrozenUnit> {
        parameters
            .iter()
            .filter(|p| !matches!(p, FrozenUnit::ValidatorRef { .. }))
            .cloned()
            .collect()
    };
    match feature {
        Feature::Validators => {
            let schemas = schemas
                .iter()
                .map(|(namespace, units)| {
                    let units = units
                        .iter()
                        .map(|unit| {
                            let mut unit = unit.clone();
                            match &mut unit {
                                FrozenUnit::Struct { fields, .. }
                                | FrozenUnit::Error { fields, .. } => {
                                    for field in fields {
                                        if let FrozenUnit::Field { parameters, .. } = field {
                                            *parameters = without_refs(parameters);
                                        }
                                    }
                                }
                                FrozenUnit::TypeAlias { parameters, .. } => {
                                    *parameters = without_refs(parameters)
                                }
                                _ => {}
                            }
                            unit
                        })
                        .collect();
                    (namespace.clone(), units)
                })
                .collect();
            // No checks left to run on decode either.
            let options = Options {
                validate_on_decode: ValidateOnDecode::Off,
                ..options.clone()
            };
            Some((schemas, options))
        }
        Feature::ValidateOnDecode => {
            let options = Options {
                validate_on_decode: ValidateOnDecode::Off,
                ..options.clone()
            };
            Some((schemas.to_vec(), options))
        }
        Feature::Async => Some((
            schemas.to_vec(),
            Options {
                async_api: false,
                ..options.clone()
            },
        )),
        // Every maximum and exact string length dropped (`char[max 64]` and
        // `char[8]` back to `string`), and every depth limit.
        Feature::Bounds => {
            let schemas = schemas
                .iter()
                .map(|(namespace, units)| {
                    (
                        namespace.clone(),
                        units.iter().map(without_bounds).collect(),
                    )
                })
                .collect();
            Some((schemas, options.clone()))
        }
        _ => None,
    }
}

/// `unit` with its types' bounds and its depth limit removed.
fn without_bounds(unit: &FrozenUnit) -> FrozenUnit {
    let mut unit = unit.clone();
    let no_depth = |parameters: &mut Vec<FrozenUnit>| {
        parameters
            .retain(|p| !matches!(p, FrozenUnit::Property { name, .. } if name == "max_depth"))
    };
    let fields = |fields: &mut Vec<FrozenUnit>| {
        for field in fields {
            if let FrozenUnit::Field { ty, .. } = field {
                *ty = ty.without_bounds();
            }
        }
    };
    match &mut unit {
        FrozenUnit::Struct {
            fields: f,
            parameters,
            ..
        } => {
            fields(f);
            no_depth(parameters);
        }
        FrozenUnit::Error { fields: f, .. } => fields(f),
        FrozenUnit::TypeAlias {
            target, parameters, ..
        } => {
            *target = target.without_bounds();
            no_depth(parameters);
        }
        FrozenUnit::Constant { ty, .. } => *ty = ty.without_bounds(),
        FrozenUnit::Protocol { functions, .. } => {
            for function in functions {
                if let FrozenUnit::Function {
                    arguments, _return, ..
                } = function
                {
                    for argument in arguments {
                        argument.ty = argument.ty.without_bounds();
                    }
                    if let Some(ret) = _return {
                        *ret = ret.without_bounds();
                    }
                }
            }
        }
        _ => {}
    }
    unit
}

/// One generator's column in a rendered matrix.
pub struct Column<'a> {
    /// `rust`, `typescript`.
    pub generator: &'a str,
    pub versions: Versions,
    pub support: SupportTable,
}

/// The matrix as a Markdown table, one row per feature. `evidence` names,
/// per (generator, feature), the conformance fixtures that prove the claim;
/// pass an empty map to leave it out.
pub fn markdown(columns: &[Column], evidence: &BTreeMap<(String, Feature), Vec<String>>) -> String {
    let mut out = String::from("| Feature |");
    for c in columns {
        let _ = write!(out, " {} ≥ {} |", c.generator, c.versions.floor);
    }
    out.push_str("\n|---|");
    for _ in columns {
        out.push_str("---|");
    }
    out.push('\n');
    for &feature in Feature::ALL {
        let _ = write!(
            out,
            "| {} <br><sub>`{}`</sub> |",
            feature.title(),
            feature.id()
        );
        for c in columns {
            let s = support_for(c.support, feature);
            let mut cell = match s.state {
                State::Supported => "✅ supported".to_string(),
                State::Partial => "🟡 partial".to_string(),
                State::Ignored => "⚪ ignored".to_string(),
                State::Unsupported => "❌ unsupported".to_string(),
            };
            if let Some(since) = s.since {
                let _ = write!(cell, " from {}", since.version);
            }
            if let Some(note) = s.note {
                let _ = write!(cell, ": {note}");
            }
            if let Some(since) = s.since {
                let _ = write!(cell, " (❌ below: {})", since.why);
            }
            if let Some(tracking) = s.tracking {
                let _ = write!(cell, " ({tracking})");
            }
            // The first few; the JSON has them all.
            if let Some(fixtures) = evidence
                .get(&(c.generator.to_string(), feature))
                .filter(|f| !f.is_empty())
            {
                let mut shown = fixtures
                    .iter()
                    .take(3)
                    .map(|f| format!("`{f}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                if fixtures.len() > 3 {
                    let _ = write!(shown, " +{} more", fixtures.len() - 3);
                }
                let _ = write!(cell, " <br><sub>proven by {shown}</sub>");
            }
            let _ = write!(out, " {} |", cell.replace('|', "\\|"));
        }
        out.push('\n');
    }
    out
}

/// The matrix as JSON: `{ "features": [...], "generators": [...] }`, each
/// generator's `support` keyed by feature id.
pub fn json(
    columns: &[Column],
    evidence: &BTreeMap<(String, Feature), Vec<String>>,
) -> serde_json::Value {
    let features: Vec<serde_json::Value> = Feature::ALL
        .iter()
        .map(|f| serde_json::json!({ "id": f.id(), "title": f.title(), "description": f.description() }))
        .collect();
    let generators: Vec<serde_json::Value> = columns
        .iter()
        .map(|c| {
            let support: serde_json::Map<String, serde_json::Value> = Feature::ALL
                .iter()
                .map(|&f| {
                    let s = support_for(c.support, f);
                    let proof = evidence.get(&(c.generator.to_string(), f)).cloned().unwrap_or_default();
                    (
                        f.id().to_string(),
                        serde_json::json!({
                            "state": s.state.id(),
                            "note": s.note,
                            "tracking": s.tracking,
                            "since": s.since.map(|since| serde_json::json!({ "version": since.version, "why": since.why })),
                            "proven_by": proof,
                        }),
                    )
                })
                .collect();
            serde_json::json!({
                "name": c.generator,
                // The floor, as before ranges: what a target declaring the
                // oldest version gets.
                "version": c.versions.floor,
                "versions": {
                    "floor": c.versions.floor,
                    "changes": c.versions.changes,
                    "verified_up_to": c.versions.verified_up_to,
                    "forward_compatible": c.versions.forward_compatible,
                },
                "support": support,
            })
        })
        .collect();
    serde_json::json!({ "features": features, "generators": generators })
}

#[cfg(test)]
mod tests {
    use super::*;
    use comline_core::schema::ir::frozen::types::PrimitiveType;

    fn field(name: &str, ty: TypeRef, optional: bool) -> FrozenUnit {
        FrozenUnit::Field {
            docstring: None,
            parameters: vec![],
            optional,
            name: name.into(),
            ty,
            default: None,
            span: (10, 20),
        }
    }

    fn array(element: TypeRef, length: Option<u64>) -> TypeRef {
        TypeRef::Array {
            element: Box::new(element),
            length,
            max: None,
        }
    }

    #[test]
    fn bounds_and_chars_are_uses_and_bounds_strip() {
        let char_ = TypeRef::Primitive(PrimitiveType::Char);
        let bounded = |element: TypeRef, max: u64| TypeRef::Array {
            element: Box::new(element),
            length: None,
            max: Some(max),
        };
        let schemas = vec![(
            "api".to_string(),
            vec![FrozenUnit::Struct {
                docstring: None,
                parameters: vec![FrozenUnit::Property {
                    name: "max_depth".into(),
                    value: ConfigValue::Integer(8),
                }],
                name: "S".into(),
                fields: vec![
                    field("name", bounded(char_.clone(), 64), false),
                    field("code", array(char_.clone(), Some(2)), false),
                    field("tags", bounded(bounded(char_.clone(), 8), 4), false),
                    field("initial", char_.clone(), false),
                    field("plain", TypeRef::Primitive(PrimitiveType::String), false),
                ],
                span: (0, 100),
            }],
        )];
        let uses = used(&schemas, &Options::default());
        assert_eq!(
            ids(&uses),
            ["structs", "bounds", "arrays", "bounds", "chars"]
        );
        assert!(uses
            .iter()
            .any(|u| u.feature == Feature::Bounds && u.owner == "struct 'S', `@max_depth`"));
        // A string isn't an array: neither `arrays` nor `fixed-arrays` for it.
        assert!(!uses
            .iter()
            .any(|u| u.owner.ends_with("'code'") && u.feature != Feature::Bounds));

        let (stripped, _) = strip(Feature::Bounds, &schemas, &Options::default()).unwrap();
        assert_eq!(
            ids(&used(&stripped, &Options::default())),
            ["structs", "arrays", "chars"]
        );
    }

    #[test]
    fn a_use_has_the_path_of_the_unit_that_uses_it() {
        use comline_core::schema::ir::frozen::unit::FrozenArgument;

        let u8_ = TypeRef::Primitive(PrimitiveType::U8);
        let validated = FrozenUnit::Field {
            docstring: None,
            parameters: vec![FrozenUnit::ValidatorRef {
                name: "len".into(),
                args: vec![],
                span: (1, 2),
                arg_spans: vec![],
            }],
            optional: false,
            name: "v".into(),
            ty: TypeRef::Primitive(PrimitiveType::U8),
            default: None,
            span: (10, 20),
        };
        let schemas = vec![
            (
                "app".to_string(),
                vec![
                    FrozenUnit::Import("other".into(), None, (1, 2)),
                    FrozenUnit::Struct {
                        docstring: None,
                        parameters: vec![FrozenUnit::Property {
                            name: "max_depth".into(),
                            value: ConfigValue::Integer(8),
                        }],
                        name: "S".into(),
                        fields: vec![field("tags", array(u8_.clone(), None), false), validated],
                        span: (0, 100),
                    },
                    FrozenUnit::TypeAlias {
                        docstring: None,
                        parameters: vec![],
                        name: "B".into(),
                        target: array(u8_.clone(), None),
                        span: (0, 10),
                    },
                    FrozenUnit::Protocol {
                        docstring: String::new(),
                        parameters: vec![],
                        name: "Chat".into(),
                        functions: vec![FrozenUnit::Function {
                            docstring: String::new(),
                            name: "send".into(),
                            arguments: vec![FrozenArgument {
                                name: "x".into(),
                                ty: array(u8_.clone(), None),
                                span: (3, 4),
                                parameters: vec![],
                            }],
                            _return: Some(array(u8_.clone(), None)),
                            throws: vec![],
                            parameters: vec![],
                            span: (5, 6),
                        }],
                        span: (0, 100),
                    },
                ],
            ),
            ("a/b".to_string(), vec![]),
        ];
        let uses = used(&schemas, &Options::default());
        let unit = |feature: Feature, owner: &str| -> Vec<&str> {
            let found: Vec<_> = uses
                .iter()
                .filter(|u| u.feature == feature && u.owner == owner)
                .collect();
            assert_eq!(found.len(), 1, "{feature:?} {owner}: {uses:#?}");
            found[0].unit.iter().map(String::as_str).collect()
        };

        assert_eq!(
            unit(Feature::Arrays, "struct 'S', field 'tags'"),
            ["S", "tags"]
        );
        assert_eq!(
            unit(
                Feature::Arrays,
                "protocol 'Chat', function 'send', argument 'x'"
            ),
            ["Chat", "send", "x"]
        );
        assert_eq!(
            unit(Feature::Arrays, "protocol 'Chat', function 'send', return"),
            ["Chat", "send"]
        );
        assert_eq!(
            unit(
                Feature::Validators,
                "struct 'S', field 'v', validator 'len'"
            ),
            ["S", "v"]
        );
        assert_eq!(unit(Feature::Arrays, "type alias 'B'"), ["B"]);
        assert_eq!(unit(Feature::Bounds, "struct 'S', `@max_depth`"), ["S"]);
        assert_eq!(unit(Feature::Structs, "struct 'S'"), ["S"]);
        assert_eq!(unit(Feature::Protocols, "protocol 'Chat'"), ["Chat"]);
        assert!(unit(Feature::Imports, "use other").is_empty());
        assert!(unit(Feature::NestedNamespaces, "schema 'a/b'").is_empty());

        // A request option is in no schema, so in no unit.
        let options = Options {
            mode: Some(Mode::Lib),
            ..Options::default()
        };
        let option: Vec<_> = used(&[], &options);
        assert_eq!(option.len(), 1, "{option:?}");
        assert!(option[0].namespace.is_empty() && option[0].unit.is_empty());
    }

    fn ids(uses: &[FeatureUse]) -> Vec<&'static str> {
        let mut ids: Vec<_> = uses.iter().map(|u| u.feature.id()).collect();
        ids.dedup();
        ids
    }

    #[test]
    fn every_feature_has_a_unique_id() {
        for f in Feature::ALL {
            assert_eq!(Feature::from_id(f.id()), Some(*f));
        }
    }

    #[test]
    fn a_struct_reports_its_shapes_with_where() {
        let u8_ = TypeRef::Primitive(PrimitiveType::U8);
        let schemas = vec![(
            "api".to_string(),
            vec![FrozenUnit::Struct {
                docstring: None,
                parameters: vec![],
                name: "S".into(),
                fields: vec![
                    field("big", array(u8_.clone(), Some(64)), false),
                    field("nested", array(array(u8_.clone(), Some(64)), None), false),
                    field(
                        "either",
                        TypeRef::Union(vec![u8_.clone(), TypeRef::Named("T".into())]),
                        true,
                    ),
                ],
                span: (0, 100),
            }],
        )];
        let uses = used(&schemas, &Options::default());
        assert_eq!(
            ids(&uses),
            [
                "structs",
                "fixed-arrays",
                "arrays",
                "fixed-arrays",
                "large-fixed-arrays-nested",
                "optional-fields",
                "unions"
            ]
        );
        let union = uses.iter().find(|u| u.feature == Feature::Unions).unwrap();
        assert_eq!(union.owner, "struct 'S', field 'either'");
        assert_eq!(union.span, Some((10, 20)));
    }

    #[test]
    fn validate_on_decode_is_only_a_use_with_validators() {
        let options = Options {
            validate_on_decode: ValidateOnDecode::Collect,
            ..Options::default()
        };
        assert!(used(&[("api".into(), vec![])], &options).is_empty());

        let schemas = [("api".to_string(), checked_alias())];
        assert_eq!(
            ids(&used(&schemas, &options)),
            ["type-aliases", "validators", "validate-on-decode"]
        );

        // Stripped, neither is left.
        let (schemas, options) = strip(Feature::Validators, &schemas, &options).unwrap();
        assert_eq!(ids(&used(&schemas, &options)), ["type-aliases"]);
    }

    #[test]
    fn async_is_only_a_use_with_a_protocol_on_a_connection() {
        let protocol = |name: &str, parameters: Vec<FrozenUnit>| FrozenUnit::Protocol {
            docstring: String::new(),
            parameters,
            name: name.into(),
            functions: vec![],
            span: (0, 1),
        };
        let asynchronous = Options {
            async_api: true,
            ..Options::default()
        };
        // A lockstep client/server protocol has no async form.
        let datagram1 = FrozenUnit::Property {
            name: "framing".into(),
            value: ConfigValue::Str("datagram".into()),
        };
        let lockstep = [("api".to_string(), vec![protocol("Chat", vec![datagram1])])];
        assert_eq!(
            ids(&used(&lockstep, &asynchronous)),
            ["protocols", "format-packed"]
        );

        // A plain one is on a connection by default.
        let schemas = [("api".to_string(), vec![protocol("Chat", vec![])])];
        assert_eq!(
            ids(&used(&schemas, &asynchronous)),
            ["protocols", "connections", "async", "format-packed"]
        );
        let (schemas, options) = strip(Feature::Async, &schemas, &asynchronous).unwrap();
        assert_eq!(
            ids(&used(&schemas, &options)),
            ["protocols", "connections", "format-packed"]
        );
    }

    #[test]
    fn the_format_is_a_use_where_a_protocol_encodes_with_it() {
        let protocol = |parameters: Vec<FrozenUnit>| FrozenUnit::Protocol {
            docstring: String::new(),
            parameters,
            name: "P".into(),
            functions: vec![],
            span: (0, 1),
        };
        let msgpack = Options {
            wire_format: Some("msgpack@1".into()),
            ..Options::default()
        };
        let plain = [("api".to_string(), vec![protocol(vec![])])];
        assert!(ids(&used(&plain, &msgpack)).contains(&"format-msgpack"));
        // JSON-RPC carries JSON, and a package without protocols encodes
        // nothing.
        let jsonrpc = FrozenUnit::Property {
            name: "framing".into(),
            value: ConfigValue::Str("jsonrpc".into()),
        };
        let rpc = [("api".to_string(), vec![protocol(vec![jsonrpc])])];
        assert!(!ids(&used(&rpc, &msgpack))
            .iter()
            .any(|id| id.starts_with("format-")));
        assert!(!ids(&used(&[("api".to_string(), vec![])], &msgpack))
            .iter()
            .any(|id| id.starts_with("format-")));
    }

    /// `@validators = [V()]` on `type Name = string`.
    fn checked_alias() -> Vec<FrozenUnit> {
        vec![FrozenUnit::TypeAlias {
            docstring: None,
            parameters: vec![FrozenUnit::ValidatorRef {
                name: "V".into(),
                args: vec![],
                span: (3, 4),
                arg_spans: vec![],
            }],
            name: "Name".into(),
            target: TypeRef::Primitive(PrimitiveType::String),
            span: (0, 9),
        }]
    }

    #[test]
    fn a_refusal_names_every_use() {
        const TABLE: SupportTable = &[(
            Feature::Unions,
            Support::unsupported("no sum types yet").tracked("core#1"),
        )];
        let uses = vec![
            FeatureUse {
                feature: Feature::Unions,
                namespace: "api".into(),
                owner: "struct 'A', field 'x'".into(),
                unit: vec!["A".into(), "x".into()],
                span: None,
            },
            FeatureUse {
                feature: Feature::Unions,
                namespace: "api".into(),
                owner: "type alias 'B'".into(),
                unit: vec!["B".into()],
                span: None,
            },
        ];
        let at = LangVersion::parse("1.70.0").unwrap();
        let refused = unsupported_uses(&uses, TABLE, &at);
        assert_eq!(
            refusal("rust", &refused, TABLE, &at),
            "the rust generator doesn't support unions (unions): no sum types yet - core#1 tracks it\n  \
             - schema 'api', struct 'A', field 'x'\n  - schema 'api', type alias 'B'"
        );
    }

    #[test]
    fn below_its_gate_a_feature_is_refused_naming_the_version_to_declare() {
        const TABLE: SupportTable = &[(
            Feature::Connections,
            Support::supported().from("1.75.0", "`async fn` in traits"),
        )];
        let uses = vec![FeatureUse {
            feature: Feature::Connections,
            namespace: String::new(),
            owner: "protocol 'Chat'".into(),
            unit: vec!["Chat".into()],
            span: None,
        }];
        let at = |v| LangVersion::parse(v).unwrap();
        assert!(unsupported_uses(&uses, TABLE, &at("1.75")).is_empty());
        assert!(unsupported_uses(&uses, TABLE, &at("1.80.1")).is_empty());
        let refused = unsupported_uses(&uses, TABLE, &at("1.70.0"));
        assert_eq!(
            refusal("rust", &refused, TABLE, &at("1.70.0")),
            "the rust generator supports connections (connections) only from `rust#1.75.0`, and this target \
             declares `rust#1.70.0`: `async fn` in traits - declare `rust#1.75.0` or later\n  - protocol 'Chat'"
        );
    }

    #[test]
    fn the_matrix_shows_a_gate() {
        const TABLE: SupportTable = &[(
            Feature::Connections,
            Support::supported().from("1.75.0", "`async fn` in traits"),
        )];
        let versions = Versions {
            floor: "1.70.0",
            changes: &["1.75.0"],
            verified_up_to: "1.90.0",
            forward_compatible: true,
        };
        let columns = [Column {
            generator: "rust",
            versions,
            support: TABLE,
        }];
        let table = markdown(&columns, &BTreeMap::new());
        assert!(table.contains("| rust ≥ 1.70.0 |"), "{table}");
        assert!(
            table.contains("✅ supported from 1.75.0 (❌ below: `async fn` in traits)"),
            "{table}"
        );
        let json = json(&columns, &BTreeMap::new());
        assert_eq!(
            json["generators"][0]["support"]["connections"]["since"]["version"],
            "1.75.0"
        );
        assert_eq!(json["generators"][0]["versions"]["changes"][0], "1.75.0");
    }
}
