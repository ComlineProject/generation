//! The facts a frozen version records for `comline@2` and for framings that
//! call by wire number (`datagram@3`, `calls = by_id`): what
//! [`GenerateRequest::facts`] carries (runtime#15).
//!
//! Core computes them from the package's history (`comline_core::compat`),
//! which a generator never sees: it reads frozen IR only, without core's
//! compiler. These types mirror core's `PackageFacts`, field for field and
//! with the same JSON, so `comline generation` hands them over as they are.
//!
//! **Numbers are never guessed.** A protocol that needs them and has none in
//! the facts is refused ([`check`]): falling back to its position would make
//! two versions of a package call each other's functions by mistake.

// Standard Uses
use std::collections::BTreeMap;

// Crate Uses
use crate::connections::{self, PackageConnection, Provider};
use crate::protocol::{EncodingMapping, GenerateRequest};

// External Uses
use comline_core::encodings::{official, EncodingKind};
use comline_core::schema::idl::annotations::{HandshakeValue, FORMAT_JSON};
use comline_core::schema::ir::frozen::unit::FrozenUnit;
use eyre::{bail, eyre, Result};
use serde::{Deserialize, Serialize};

/// The framing that calls by wire numbers.
pub const FRAMING_DATAGRAM3: &str = "datagram@3";
/// The handshake that checks compatibility by facts.
pub const HANDSHAKE_COMLINE2: &str = "comline@2";

/// What a version provides for `comline@2`: core's `compat::PackageFacts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageFacts {
    /// `id` in `config.idp`; `None` for a version built without one.
    pub id: Option<String>,
    /// The version these are the facts of: `0.3.0`, `0.4.0-dev.2`.
    pub version: String,
    /// The package's own protocols, in schema and declaration order.
    pub protocols: Vec<ProtocolFacts>,
    /// Every encoding the protocols use, by kind then name.
    pub encodings: Vec<EncodingFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolFacts {
    /// `namespace::Protocol`.
    pub path: String,
    pub namespace: String,
    pub name: String,
    /// The release it last broke at.
    pub line: String,
    /// Its wire number, unique in the package.
    pub number: u16,
    /// blake3, hex, of its wire shape.
    pub shape_hash: String,
    pub functions: Vec<FunctionFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionFacts {
    pub name: String,
    /// The release it first appeared in.
    pub since: String,
    /// Its wire number, unique in its protocol.
    pub number: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncodingFacts {
    /// `format`, `framing` or `handshake`.
    pub kind: String,
    /// Canonical: `packed@1`, `datagram@3`, `comline@2`.
    pub name: String,
    pub params: BTreeMap<String, String>,
    /// blake3, hex, of the encoding's kind, name and parameters.
    pub param_hash: String,
}

/// Whether the framing `name` addresses calls by wire numbers
/// (`calls = by_id`): `datagram@3`, or a declared framing stating it.
pub fn calls_by_id(encodings: &BTreeMap<String, EncodingMapping>, name: &str) -> bool {
    let calls = match official::profile(EncodingKind::Framing, name) {
        Some(profile) => profile.stated("calls"),
        None => encodings.get(name).and_then(|mapping| mapping.calls.clone()),
    };
    calls.as_deref() == Some("by_id")
}

/// Whether a protocol with these parameters needs facts: its framing calls
/// by wire number, or its handshake is `comline@2`.
pub fn needed_by(encodings: &BTreeMap<String, EncodingMapping>, parameters: &[FrozenUnit]) -> bool {
    calls_by_id(encodings, &connections::framing_name(parameters))
        || connections::handshake_name(parameters) == HANDSHAKE_COMLINE2
}

/// Whether any protocol in `schemas` needs facts ([`needed_by`]). Nothing
/// else does, so a package that chooses neither gets a request, and output,
/// exactly as before. A package's dependencies aren't known here, so only
/// `std`'s protocols are left out; see [`needed_in`].
pub fn needed(schemas: &[(String, Vec<FrozenUnit>)], encodings: &BTreeMap<String, EncodingMapping>) -> bool {
    needed_among(schemas, &[], encodings)
}

/// [`needed`] for the request: its dependencies' protocols aren't the
/// package's to number, so they don't need its facts.
pub fn needed_in(req: &GenerateRequest) -> bool {
    needed_among(&req.schemas, &req.dependencies, &req.encodings)
}

fn needed_among(
    schemas: &[(String, Vec<FrozenUnit>)],
    dependencies: &[String],
    encodings: &BTreeMap<String, EncodingMapping>,
) -> bool {
    connections::protocols(schemas).any(|(namespace, name, parameters, _)| {
        connections::reached(dependencies, namespace, name) && needed_by(encodings, parameters)
    })
}

/// The segments of a namespace, whichever way it is spelled: a request's
/// schemas are `/`-joined (`chat/admin`, as the CLI builds them from the
/// schema's path), and the facts' namespaces are `::`-joined (`chat::admin`,
/// as core's frozen `Namespace` unit joins them). A namespace of one segment
/// is spelled the same either way.
pub fn namespace_segments(namespace: &str) -> impl Iterator<Item = &str> {
    namespace.split("::").flat_map(|part| part.split('/'))
}

/// A namespace in the request's spelling, `/`-joined.
pub fn canonical_namespace(namespace: &str) -> String {
    namespace_segments(namespace).collect::<Vec<_>>().join("/")
}

/// Whether two namespaces are the same one, whichever way each is spelled
/// ([`namespace_segments`]). Every comparison of a request's namespace with a
/// facts' goes through this.
pub fn same_namespace(a: &str, b: &str) -> bool {
    namespace_segments(a).eq(namespace_segments(b))
}

/// The facts of protocol `namespace::name`, if the request has them.
/// `namespace` is in either spelling ([`same_namespace`]).
pub fn protocol<'a>(req_facts: Option<&'a PackageFacts>, namespace: &str, name: &str) -> Option<&'a ProtocolFacts> {
    req_facts?.protocols.iter().find(|p| same_namespace(&p.namespace, namespace) && p.name == name)
}

/// How one protocol's calls are addressed on a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Addressing {
    /// The number its frames carry.
    pub protocol: u16,
    /// What each function's call carries, by declaration position: its wire
    /// number under a framing that calls by number, else its position.
    pub calls: Vec<u16>,
    /// Under `comline@2`: each function's wire number, by declaration
    /// position, which the handshake's `check_call` takes. The protocol's
    /// is [`protocol`](Self::protocol).
    pub checks: Option<Vec<u16>>,
    /// Whether the framing calls by number, so a dispatcher matches numbers.
    pub by_id: bool,
}

/// How `namespace::name`, on a connection, is addressed.
///
/// - Its protocol number is its wire number when the facts are needed
///   ([`needed_by`]), else its position among the package's protocols
///   ([`connections::protocol_number`]), as before.
/// - A call carries the function's wire number under a framing that calls
///   by number, else its position.
///
/// Refuses a protocol that needs numbers the request doesn't have. It
/// doesn't ask whether the package reaches the protocol, though a dependency's
/// has the numbers of its own package and isn't the package's to address:
/// [`addressing_in`] does.
pub fn addressing(
    req: &GenerateRequest,
    namespace: &str,
    name: &str,
    parameters: &[FrozenUnit],
    functions: &[FrozenUnit],
) -> Result<Addressing> {
    let names: Vec<&str> = functions
        .iter()
        .filter_map(|f| match f {
            FrozenUnit::Function { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let by_id = calls_by_id(&req.encodings, &connections::framing_name(parameters));
    if !needed_by(&req.encodings, parameters) {
        return positional(&req.schemas, namespace, name, names.len(), by_id);
    }
    let facts = protocol(req.facts.as_ref(), namespace, name)
        .ok_or_else(|| missing(req.facts.as_ref(), &req.dependencies, namespace, name, parameters))?;
    let numbers: Vec<u16> = names
        .iter()
        .map(|f| {
            facts.functions.iter().find(|ff| ff.name == *f).map(|ff| ff.number).ok_or_else(|| {
                eyre!(
                    "protocol `{namespace}::{name}`, function `{f}`: the facts have no wire number for it, so they \
                     are older than the schema; run `comline build`, then run `comline generation` again"
                )
            })
        })
        .collect::<Result<_>>()?;
    let calls = if by_id { numbers.clone() } else { (0..names.len() as u16).collect() };
    let checks = (connections::handshake_name(parameters) == HANDSHAKE_COMLINE2).then_some(numbers);
    Ok(Addressing { protocol: facts.number, calls, checks, by_id })
}

/// Where a protocol with no numbers of its own is addressed: by position,
/// as before facts.
fn positional(
    schemas: &[(String, Vec<FrozenUnit>)],
    namespace: &str,
    name: &str,
    functions: usize,
    by_id: bool,
) -> Result<Addressing> {
    let protocol = connections::protocol_number(schemas, namespace, name)
        .ok_or_else(|| eyre!("protocol `{name}` has no number: more than 65536 protocols in the package"))?;
    Ok(Addressing { protocol, calls: (0..functions as u16).collect(), checks: None, by_id })
}

/// [`addressing`] for a protocol of the request's package. One the package
/// doesn't reach ([`connections::reached`]) isn't on its connection, so
/// nothing addresses it: it gets the positions it would without facts, which
/// the generated type carries and no connection uses, and the facts aren't
/// asked for its numbers.
pub fn addressing_in(
    req: &GenerateRequest,
    namespace: &str,
    name: &str,
    parameters: &[FrozenUnit],
    functions: &[FrozenUnit],
) -> Result<Addressing> {
    if connections::reached(&req.dependencies, namespace, name) {
        return addressing(req, namespace, name, parameters, functions);
    }
    let functions = functions.iter().filter(|f| matches!(f, FrozenUnit::Function { .. })).count();
    let by_id = calls_by_id(&req.encodings, &connections::framing_name(parameters));
    positional(&req.schemas, namespace, name, functions, by_id)
}

fn missing(
    req_facts: Option<&PackageFacts>,
    dependencies: &[String],
    namespace: &str,
    name: &str,
    parameters: &[FrozenUnit],
) -> eyre::Report {
    let why = format!(
        "protocol `{namespace}::{name}` uses `{}` with `{}`, which address it by the wire numbers its package's \
         history assigns",
        connections::framing_name(parameters),
        connections::handshake_name(parameters)
    );
    let Some(facts) = req_facts else {
        return eyre!(
            "{why}, and the request carries no facts: `comline generation` supplies them from the package's build \
             history (run `comline build` first)"
        );
    };
    if connections::is_dependency(dependencies, namespace) {
        return eyre!(
            "{why}, and the package's facts have none for it: `{namespace}` is a dependency's schema, and a \
             dependency's protocol has its own package's numbers, so it can't be on a connection until core#277"
        );
    }
    let held = match facts.protocols.as_slice() {
        [] => "no protocols".to_string(),
        protocols => protocols.iter().map(|p| p.path.as_str()).collect::<Vec<_>>().join(", "),
    };
    // The same name in another namespace is the likely mismatch.
    let elsewhere = facts.protocols.iter().find(|p| p.name == name).map(|p| {
        format!(" (it has `{}`, in namespace `{}`, which isn't `{namespace}`)", p.name, p.namespace)
    });
    eyre!(
        "{why}, and the package's facts hold no protocol `{name}` in namespace `{namespace}`{}: a new protocol \
         has none until `comline build` records it, so run `comline build`, then `comline generation` again. The \
         facts hold: {held}",
        elsewhere.unwrap_or_default()
    )
}

/// What a generator checks before rendering a request that needs facts:
/// every protocol that needs numbers has them; `comline@2` has a package id,
/// a version and one package connection carrying every protocol that runs
/// it, with one set of parameters; and no protocol off a connection runs it.
/// Only the protocols the package reaches ([`connections::reached`]) are
/// checked: a dependency's isn't on its connection, so it has no numbers to
/// find and no handshake to run.
pub fn check(req: &GenerateRequest) -> Result<()> {
    if !needed_in(req) {
        return Ok(());
    }
    let mut problems = vec![];
    for (namespace, name, parameters, functions) in connections::protocols(&req.schemas) {
        if !connections::reached(&req.dependencies, namespace, name) || !needed_by(&req.encodings, parameters) {
            continue;
        }
        if let Err(e) = addressing(req, namespace, name, parameters, functions) {
            problems.push(e.to_string());
            continue;
        }
        let on = connections::on_connection_among(&req.schemas, &req.dependencies, parameters);
        if connections::handshake_name(parameters) == HANDSHAKE_COMLINE2 && !on {
            problems.push(format!(
                "protocol `{namespace}::{name}` runs `comline@2` off a connection (`{}`), which isn't supported: \
                 use a connection framing such as `datagram@3`",
                connections::framing_name(parameters)
            ));
        }
    }
    if !problems.is_empty() {
        bail!("{}", problems.join("\n"));
    }
    if let Some(connection) = comline2_connection(req)? {
        hello(req, &connection)?;
    }
    Ok(())
}

/// The package's connection when it runs `comline@2`; `Err` when some
/// protocol runs `comline@2` but the protocols on a connection don't share
/// one.
pub fn comline2_connection(req: &GenerateRequest) -> Result<Option<PackageConnection>> {
    let runs = connections::protocols(&req.schemas).any(|(namespace, name, parameters, _)| {
        connections::reached(&req.dependencies, namespace, name)
            && connections::handshake_name(parameters) == HANDSHAKE_COMLINE2
            && connections::on_connection_among(&req.schemas, &req.dependencies, parameters)
    });
    if !runs {
        return Ok(None);
    }
    match connections::package_connection_among(&req.schemas, &req.dependencies).map_err(|d| d.reason) {
        Ok(Some(c)) if c.handshake == HANDSHAKE_COMLINE2 => Ok(Some(c)),
        Ok(_) => Ok(None),
        Err(why) => bail!("`comline@2` runs one handshake per connection, and {why}"),
    }
}

/// `comline@2`'s four parameters, each its value as written or its default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comline2Params {
    /// `protocol` or `package`.
    pub scope: String,
    /// `degrade` or `refuse`.
    pub newer_consumer: String,
    /// `refuse` or `warn`.
    pub same_version_differs: String,
    /// `offer` or `symmetric`.
    pub exchange: String,
}

impl Comline2Params {
    pub fn of(value: &HandshakeValue) -> Comline2Params {
        let param = |key: &str| value.param(key).unwrap_or_default().to_string();
        Comline2Params {
            scope: param("scope"),
            newer_consumer: param("newer_consumer"),
            same_version_differs: param("same_version_differs"),
            exchange: param("exchange"),
        }
    }
}

/// The parameters the package connection's protocols state (the package
/// connection guarantees they're the same).
pub fn comline2_params(req: &GenerateRequest) -> Comline2Params {
    let value = connections::protocols(&req.schemas)
        .find(|(namespace, name, parameters, _)| {
            connections::reached(&req.dependencies, namespace, name)
                && connections::handshake_name(parameters) == HANDSHAKE_COMLINE2
                && connections::on_connection_among(&req.schemas, &req.dependencies, parameters)
        })
        .map(|(_, _, parameters, _)| HandshakeValue::of_protocol(parameters))
        .unwrap_or_else(|| HandshakeValue::named(HANDSHAKE_COMLINE2));
    Comline2Params::of(&value)
}

/// One protocol of the hello, for a generator's comments and lookups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelloProtocol {
    pub namespace: String,
    pub name: String,
    pub number: u16,
    pub line: String,
    pub functions: Vec<(String, u16)>,
}

/// This end's `comline@2` Hello, encoded as the handshake's spec has it
/// (docs `reference/wire/handshakes`): what a generator embeds. With the
/// protocols it covers, ascending by number.
///
/// It covers the protocols on the package connection, which are the ones the
/// package reaches and so, until core#277, the ones it declares itself, and
/// the encodings that connection uses: the package's
/// format (JSON on a framing that writes it), the connection's framing, and
/// `comline@2` itself.
///
/// [`hello_parts`] has the same pieces per protocol, for a Hello of fewer
/// protocols than the connection's.
pub fn hello(req: &GenerateRequest, connection: &PackageConnection) -> Result<(Vec<u8>, Vec<HelloProtocol>)> {
    let facts = req.facts.as_ref().ok_or_else(|| {
        eyre!(
            "the package's connection runs `comline@2`, and the request carries no facts: `comline generation` \
             supplies them from the package's build history (run `comline build` first)"
        )
    })?;
    let id = facts.id.as_deref().ok_or_else(|| {
        eyre!(
            "the package's connection runs `comline@2`, which identifies the package by its `id`, and version {} \
             was built without one: run `comline build`, which writes one into config.idp",
            facts.version
        )
    })?;
    let id = hash128(id).ok_or_else(|| eyre!("package id `{id}` isn't 32 hex digits"))?;
    let (release, pre) = version(&facts.version)
        .ok_or_else(|| eyre!("package version `{}` isn't `major.minor.patch[-pre]`", facts.version))?;

    // The connection's encodings, from the facts.
    let format = if crate::encodings::writes_json(&req.encodings, &connection.framing) {
        FORMAT_JSON.to_string()
    } else {
        crate::encodings::canonical(&req.wire_format)
    };
    let formats: Vec<&EncodingFacts> = facts.encodings.iter().filter(|e| e.kind == "format").collect();
    let mut encodings: Vec<(u8, [u8; 16])> = vec![];
    let pick = |e: &&EncodingFacts| match e.kind.as_str() {
        // One format in play is the connection's; of several, the one by name.
        "format" => formats.len() == 1 || e.name == format,
        "framing" => e.name == connection.framing,
        "handshake" => e.name == connection.handshake,
        _ => false,
    };
    for e in facts.encodings.iter().filter(pick) {
        let kind = match e.kind.as_str() {
            "format" => 0,
            "framing" => 1,
            _ => 2,
        };
        let hash = hash128(&e.param_hash).ok_or_else(|| eyre!("encoding `{}`'s hash isn't hex", e.name))?;
        encodings.push((kind, hash));
    }
    encodings.sort();
    encodings.dedup();
    for (kind, what) in [(0, "format"), (1, "framing"), (2, "handshake")] {
        if !encodings.iter().any(|(k, _)| *k == kind) {
            bail!("the package's facts record no {what} for its `comline@2` connection: run `comline build`");
        }
    }

    let mut protocols = vec![];
    for (namespace, name, parameters, functions) in connections::protocols(&req.schemas) {
        // A dependency's protocol isn't on the connection, so not this
        // package's to declare.
        if !connections::reached(&req.dependencies, namespace, name)
            || !connections::on_connection_among(&req.schemas, &req.dependencies, parameters)
        {
            continue;
        }
        let p = protocol(Some(facts), namespace, name)
            .ok_or_else(|| missing(Some(facts), &req.dependencies, namespace, name, parameters))?;
        let addressing = addressing_in(req, namespace, name, parameters, functions)?;
        let names = functions.iter().filter_map(|f| match f {
            FrozenUnit::Function { name, .. } => Some(name.clone()),
            _ => None,
        });
        let line = version(&p.line).map(|(r, _)| r).ok_or_else(|| eyre!("line `{}` isn't a release", p.line))?;
        let shape = hash128(&p.shape_hash).ok_or_else(|| eyre!("shape hash of `{}` isn't hex", p.path))?;
        let mut numbers = addressing.checks.clone().unwrap_or_default();
        numbers.sort_unstable();
        numbers.dedup();
        protocols.push((
            p.number,
            line,
            shape,
            numbers,
            HelloProtocol {
                namespace: namespace.to_string(),
                name: name.to_string(),
                number: p.number,
                line: p.line.clone(),
                functions: names.zip(addressing.checks.unwrap_or_default()).collect(),
            },
        ));
    }
    protocols.sort_by_key(|p| p.0);

    let exchange = Comline2Params::of(&connection.handshake_value).exchange;
    let mut out = vec![b'C', b'O', 2, 1, if exchange == "symmetric" { 1 } else { 0 }];
    out.extend_from_slice(&id);
    put_release(release, &mut out);
    varint(pre.len() as u32, &mut out);
    out.extend_from_slice(pre.as_bytes());
    varint(encodings.len() as u32, &mut out);
    for (kind, hash) in &encodings {
        out.push(*kind);
        out.extend_from_slice(hash);
    }
    varint(protocols.len() as u32, &mut out);
    for (number, line, shape, functions, _) in &protocols {
        varint(*number as u32, &mut out);
        put_release(*line, &mut out);
        out.extend_from_slice(shape);
        varint(functions.len() as u32, &mut out);
        for f in functions {
            varint(*f as u32, &mut out);
        }
    }
    Ok((out, protocols.into_iter().map(|p| p.4).collect()))
}

/// One protocol's entry of a Hello: what the peer is told about it, and what
/// the generator keeps beside it. The entry carries no direction on the wire;
/// [`provider`](Self::provider) is for the generated code, which knows
/// `@provider` and so which side provides the protocol and which calls it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelloEntry {
    pub namespace: String,
    pub name: String,
    /// Its wire number, unique in the package.
    pub number: u16,
    /// The release it last broke at, as the facts spell it.
    pub line: String,
    /// [`line`](Self::line) as `(major, minor, patch)`.
    pub release: (u32, u32, u32),
    /// The first 16 bytes of its shape hash.
    pub shape_hash: [u8; 16],
    /// Each function's name and wire number, in declaration order.
    pub functions: Vec<(String, u16)>,
    /// Which side implements it (`@provider`).
    pub provider: Provider,
}

impl HelloEntry {
    /// The function numbers the Hello lists: ascending, once each.
    pub fn function_numbers(&self) -> Vec<u16> {
        let mut numbers: Vec<u16> = self.functions.iter().map(|(_, number)| *number).collect();
        numbers.sort_unstable();
        numbers.dedup();
        numbers
    }

    /// The entry as the Hello's protocol list carries it.
    pub fn encode(&self, out: &mut Vec<u8>) {
        varint(self.number as u32, out);
        put_release(self.release, out);
        out.extend_from_slice(&self.shape_hash);
        let functions = self.function_numbers();
        varint(functions.len() as u32, out);
        for f in functions {
            varint(f as u32, out);
        }
    }
}

/// One encoding the connection uses, as the Hello lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HelloEncoding {
    /// `0` format, `1` framing, `2` handshake.
    pub kind: u8,
    /// The first 16 bytes of its parameter hash.
    pub hash: [u8; 16],
}

/// What a `comline@2` Hello is built from, per protocol: [`hello`]'s pieces
/// before they are put together, so generated code can put together the
/// Hello of the protocols a side provides and calls, instead of the one
/// Hello of the package's whole connection.
///
/// Everything that doesn't depend on which protocols are listed is here once:
/// the exchange, the package's id and version, and the connection's encodings
/// (the same whichever protocols a Hello lists). The package connection's
/// protocols are [`protocols`](Self::protocols), one [`HelloEntry`] each.
///
/// **Order.** A Hello is canonical: encodings ascending by kind then hash,
/// protocols ascending by wire number, each protocol's function numbers
/// ascending. [`encodings`](Self::encodings) and [`protocols`](Self::protocols)
/// are in that order already, and [`assemble`](Self::assemble) puts any subset
/// of the entries in it, whichever order they are given. The order the
/// protocols are provided or called in doesn't reach the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelloParts {
    /// `offer` or `symmetric`: a Hello's exchange flag.
    pub exchange: String,
    /// The package's `id`.
    pub package: [u8; 16],
    /// The version's release.
    pub version: (u32, u32, u32),
    /// The version's pre-release (`dev.2`), empty for a release.
    pub pre: String,
    pub encodings: Vec<HelloEncoding>,
    /// Every protocol on the package connection, ascending by wire number.
    pub protocols: Vec<HelloEntry>,
}

impl HelloParts {
    /// The Hello listing exactly `protocols`: those entries, in canonical
    /// order, with the package, its version and the connection's encodings.
    /// An entry given twice is listed once. With every entry
    /// ([`protocols`](Self::protocols)) it is [`hello`]'s output, byte for
    /// byte.
    pub fn assemble<'a>(&self, protocols: impl IntoIterator<Item = &'a HelloEntry>) -> Vec<u8> {
        let mut listed: Vec<&HelloEntry> = protocols.into_iter().collect();
        listed.sort_by_key(|p| p.number);
        listed.dedup_by_key(|p| p.number);
        let mut out = vec![b'C', b'O', 2, 1, if self.exchange == "symmetric" { 1 } else { 0 }];
        out.extend_from_slice(&self.package);
        put_release(self.version, &mut out);
        varint(self.pre.len() as u32, &mut out);
        out.extend_from_slice(self.pre.as_bytes());
        varint(self.encodings.len() as u32, &mut out);
        for encoding in &self.encodings {
            out.push(encoding.kind);
            out.extend_from_slice(&encoding.hash);
        }
        varint(listed.len() as u32, &mut out);
        for protocol in listed {
            protocol.encode(&mut out);
        }
        out
    }

    /// The Hello of the whole connection: every protocol listed.
    pub fn assemble_all(&self) -> Vec<u8> {
        self.assemble(&self.protocols)
    }
}

/// What this end's Hello is built from, per protocol ([`HelloParts`]): the
/// same facts, checks and errors as [`hello`], and the same protocols (those
/// on the package connection), but kept apart instead of put together.
///
/// A generator emits them, and generated `accept` / `connect` assemble the
/// Hello of the protocols actually provided and called. With every protocol
/// among them, that Hello is [`hello`]'s, byte for byte.
pub fn hello_parts(req: &GenerateRequest, connection: &PackageConnection) -> Result<HelloParts> {
    let facts = req.facts.as_ref().ok_or_else(|| {
        eyre!(
            "the package's connection runs `comline@2`, and the request carries no facts: `comline generation` \
             supplies them from the package's build history (run `comline build` first)"
        )
    })?;
    let id = facts.id.as_deref().ok_or_else(|| {
        eyre!(
            "the package's connection runs `comline@2`, which identifies the package by its `id`, and version {} \
             was built without one: run `comline build`, which writes one into config.idp",
            facts.version
        )
    })?;
    let package = hash128(id).ok_or_else(|| eyre!("package id `{id}` isn't 32 hex digits"))?;
    let (package_version, pre) = version(&facts.version)
        .ok_or_else(|| eyre!("package version `{}` isn't `major.minor.patch[-pre]`", facts.version))?;

    let format = if crate::encodings::writes_json(&req.encodings, &connection.framing) {
        FORMAT_JSON.to_string()
    } else {
        crate::encodings::canonical(&req.wire_format)
    };
    let formats = facts.encodings.iter().filter(|e| e.kind == "format").count();
    let mut encodings = vec![];
    for e in &facts.encodings {
        let (kind, wanted) = match e.kind.as_str() {
            // One format in play is the connection's; of several, the one by name.
            "format" => (0, formats == 1 || e.name == format),
            "framing" => (1, e.name == connection.framing),
            "handshake" => (2, e.name == connection.handshake),
            _ => continue,
        };
        if wanted {
            let hash = hash128(&e.param_hash).ok_or_else(|| eyre!("encoding `{}`'s hash isn't hex", e.name))?;
            encodings.push(HelloEncoding { kind, hash });
        }
    }
    encodings.sort();
    encodings.dedup();
    for (kind, what) in [(0, "format"), (1, "framing"), (2, "handshake")] {
        if !encodings.iter().any(|e| e.kind == kind) {
            bail!("the package's facts record no {what} for its `comline@2` connection: run `comline build`");
        }
    }

    let mut protocols = vec![];
    for (namespace, name, parameters, functions) in connections::protocols(&req.schemas) {
        // A dependency's protocol isn't on the connection, so not this
        // package's to declare.
        if !connections::reached(&req.dependencies, namespace, name)
            || !connections::on_connection_among(&req.schemas, &req.dependencies, parameters)
        {
            continue;
        }
        let p = protocol(Some(facts), namespace, name)
            .ok_or_else(|| missing(Some(facts), &req.dependencies, namespace, name, parameters))?;
        let addressing = addressing_in(req, namespace, name, parameters, functions)?;
        let names = functions.iter().filter_map(|f| match f {
            FrozenUnit::Function { name, .. } => Some(name.clone()),
            _ => None,
        });
        let release = version(&p.line).map(|(r, _)| r).ok_or_else(|| eyre!("line `{}` isn't a release", p.line))?;
        let shape_hash = hash128(&p.shape_hash).ok_or_else(|| eyre!("shape hash of `{}` isn't hex", p.path))?;
        protocols.push(HelloEntry {
            namespace: namespace.to_string(),
            name: name.to_string(),
            number: p.number,
            line: p.line.clone(),
            release,
            shape_hash,
            functions: names.zip(addressing.checks.unwrap_or_default()).collect(),
            provider: Provider::of(parameters),
        });
    }
    protocols.sort_by_key(|p| p.number);

    Ok(HelloParts {
        exchange: Comline2Params::of(&connection.handshake_value).exchange,
        package,
        version: package_version,
        pre: pre.to_string(),
        encodings,
        protocols,
    })
}

/// The first 16 bytes of a hex hash: `comline@2`'s 128-bit hashes.
fn hash128(hex: &str) -> Option<[u8; 16]> {
    if hex.len() < 32 || !hex.is_ascii() {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// `1.2.3-dev.4` as `((1, 2, 3), "dev.4")`.
fn version(v: &str) -> Option<((u32, u32, u32), &str)> {
    let (release, pre) = v.split_once('-').unwrap_or((v, ""));
    let mut parts = release.split('.').map(|p| p.parse::<u32>().ok());
    let r = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some((r, pre))
}

fn put_release((major, minor, patch): (u32, u32, u32), out: &mut Vec<u8>) {
    varint(major, out);
    varint(minor, out);
    varint(patch, out);
}

/// Shortest-form unsigned LEB128.
fn varint(mut value: u32, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::TargetLanguage;
    use crate::validation::ValidateOnDecode;
    use crate::{Mode, PackageMeta};
    use comline_core::schema::ir::frozen::unit::ConfigValue;

    /// A protocol with one function, on `datagram@3` with `comline@2`: what
    /// core writes into a package's protocols by default.
    fn protocol_unit(name: &str) -> FrozenUnit {
        let property = |name: &str, value: &str| FrozenUnit::Property {
            name: name.into(),
            value: ConfigValue::Str(value.into()),
        };
        FrozenUnit::Protocol {
            docstring: String::new(),
            parameters: vec![property("framing", FRAMING_DATAGRAM3), property("handshake", HANDSHAKE_COMLINE2)],
            name: name.into(),
            functions: vec![FrozenUnit::Function {
                docstring: String::new(),
                name: "ping".into(),
                arguments: vec![],
                _return: None,
                throws: vec![],
                parameters: vec![],
                span: (0, 0),
            }],
            span: (0, 0),
        }
    }

    /// A package `own` protocols in `api`, with a dependency `localdeps`
    /// declaring `Foo` in `localdeps/greeting`; no facts.
    fn request(own: &[&str]) -> GenerateRequest {
        GenerateRequest {
            mode: Mode::Code,
            schemas: vec![
                ("api".into(), own.iter().map(|name| protocol_unit(name)).collect()),
                ("localdeps/greeting".into(), vec![protocol_unit("Foo")]),
            ],
            package: PackageMeta { name: "demo".into(), version: "1.0.0".into() },
            target: TargetLanguage { language: "toy".into(), version: "1.0.0".into() },
            wire_format: "packed@1".into(),
            external_std: false,
            validate_on_decode: ValidateOnDecode::default(),
            async_api: false,
            encodings: BTreeMap::new(),
            facts: None,
            dependencies: vec!["localdeps".into()],
        }
    }

    /// `own`'s protocols numbered as a package's history would, with the
    /// one encoding each of format, framing and handshake `hello` looks for.
    fn facts_of(own: &[&str]) -> PackageFacts {
        let encoding = |kind: &str, name: &str| EncodingFacts {
            kind: kind.into(),
            name: name.into(),
            params: BTreeMap::new(),
            param_hash: "ab".repeat(32),
        };
        PackageFacts {
            id: Some("0123456789abcdef0123456789abcdef".into()),
            version: "1.0.0".into(),
            protocols: own
                .iter()
                .enumerate()
                .map(|(i, name)| ProtocolFacts {
                    path: format!("api::{name}"),
                    namespace: "api".into(),
                    name: name.to_string(),
                    line: "1.0.0".into(),
                    number: 7 + i as u16,
                    shape_hash: "5a".repeat(32),
                    functions: vec![FunctionFacts { name: "ping".into(), since: "1.0.0".into(), number: 3 }],
                })
                .collect(),
            encodings: vec![
                encoding("format", "packed@1"),
                encoding("framing", FRAMING_DATAGRAM3),
                encoding("handshake", HANDSHAKE_COMLINE2),
            ],
        }
    }

    fn foo(req: &GenerateRequest) -> (&str, &str, &[FrozenUnit], &[FrozenUnit]) {
        connections::protocols(&req.schemas).find(|(_, name, _, _)| *name == "Foo").unwrap()
    }

    #[test]
    fn a_dependencys_protocol_needs_no_facts_of_the_package() {
        let req = request(&[]);
        // Found by the old `needed`, which can't tell it from the package's.
        assert!(needed(&req.schemas, &req.encodings));
        assert!(!needed_in(&req));
        check(&req).unwrap();
    }

    #[test]
    fn an_own_protocol_still_needs_its_facts() {
        let mut req = request(&["Chat"]);
        assert!(needed_in(&req));
        let error = check(&req).unwrap_err().to_string();
        assert!(error.contains("`api::Chat`") && !error.contains("Foo"), "{error}");
        // With the facts, the dependency's protocol is left alone.
        req.facts = Some(facts_of(&["Chat"]));
        check(&req).unwrap();
    }

    #[test]
    fn a_dependencys_protocol_is_not_addressed() {
        let mut req = request(&["Chat"]);
        req.facts = Some(facts_of(&["Chat"]));
        let (namespace, name, parameters, functions) = foo(&req);
        // `addressing` doesn't ask whether it's reached, so it's refused for
        // having no numbers in the package's facts, and says why.
        let error = addressing(&req, namespace, name, parameters, functions).unwrap_err().to_string();
        assert!(error.contains("`localdeps/greeting::Foo`") && error.contains("core#277"), "{error}");
        assert!(!error.contains("comline build"), "{error}");
        // Without the dependency's name, `Foo` looks like the package's own.
        let mut undeclared = request(&["Chat"]);
        undeclared.facts = req.facts.clone();
        undeclared.dependencies.clear();
        let error = addressing(&undeclared, namespace, name, parameters, functions).unwrap_err().to_string();
        assert!(!error.contains("core#277") && error.contains("comline build"), "{error}");
        // With it, it has the position it would have without facts, and no
        // checks to run.
        let at = addressing_in(&req, namespace, name, parameters, functions).unwrap();
        assert_eq!(at, Addressing { protocol: 1, calls: vec![0], checks: None, by_id: true });
        // The package's own keeps its wire numbers.
        let (namespace, name, parameters, functions) =
            connections::protocols(&req.schemas).find(|(_, name, _, _)| *name == "Chat").unwrap();
        let at = addressing_in(&req, namespace, name, parameters, functions).unwrap();
        assert_eq!(at, Addressing { protocol: 7, calls: vec![3], checks: Some(vec![3]), by_id: true });
    }

    /// A package `own` protocols in `chat/admin`, a nested schema, and facts
    /// as core produces them: `::`-joined, where the request is `/`-joined.
    fn nested_request(own: &[&str]) -> GenerateRequest {
        let mut req = request(own);
        req.schemas[0].0 = "chat/admin".into();
        let mut facts = facts_of(own);
        for p in &mut facts.protocols {
            p.namespace = "chat::admin".into();
            p.path = format!("chat::admin::{}", p.name);
        }
        req.facts = Some(facts);
        req
    }

    #[test]
    fn namespaces_are_the_same_in_either_spelling() {
        assert!(same_namespace("chat/admin", "chat::admin"));
        assert!(same_namespace("chat::admin", "chat/admin"));
        assert!(same_namespace("chat/admin", "chat/admin"));
        assert!(same_namespace("chat::admin", "chat::admin"));
        assert!(same_namespace("chat", "chat"));
        assert!(same_namespace("a/b::c", "a::b/c"));
        assert!(!same_namespace("chat/admin", "chat"));
        assert!(!same_namespace("chat/admin", "chat::user"));
        assert!(!same_namespace("chat", "chat::admin"));
        // Neither is a prefix match, nor a substring one.
        assert!(!same_namespace("chat/admin", "chat/administrator"));
        assert!(!same_namespace("chat:admin", "chat::admin"));
        assert_eq!(canonical_namespace("chat::admin::v2"), "chat/admin/v2");
        assert_eq!(canonical_namespace("chat/admin"), "chat/admin");
    }

    #[test]
    fn facts_of_a_nested_namespace_are_found_in_either_spelling() {
        // Facts `::`-joined, request `/`-joined: what core and the CLI give.
        let req = nested_request(&["Chat"]);
        let facts = req.facts.as_ref();
        assert_eq!(protocol(facts, "chat/admin", "Chat").map(|p| p.number), Some(7));
        assert_eq!(protocol(facts, "chat::admin", "Chat").map(|p| p.number), Some(7));
        assert!(protocol(facts, "chat", "Chat").is_none());
        assert!(protocol(facts, "chat/admin", "Other").is_none());
        // And the other way: facts `/`-joined, request `::`-joined.
        let mut slashed = req.facts.clone().unwrap();
        slashed.protocols[0].namespace = "chat/admin".into();
        assert_eq!(protocol(Some(&slashed), "chat::admin", "Chat").map(|p| p.number), Some(7));
        assert_eq!(protocol(Some(&slashed), "chat/admin", "Chat").map(|p| p.number), Some(7));
    }

    #[test]
    fn a_nested_protocol_is_addressed_by_its_facts() {
        let req = nested_request(&["Chat"]);
        check(&req).unwrap();
        let (namespace, name, parameters, functions) =
            connections::protocols(&req.schemas).find(|(_, name, _, _)| *name == "Chat").unwrap();
        assert_eq!(namespace, "chat/admin");
        let at = addressing_in(&req, namespace, name, parameters, functions).unwrap();
        assert_eq!(at, Addressing { protocol: 7, calls: vec![3], checks: Some(vec![3]), by_id: true });
        let connection = comline2_connection(&req).unwrap().unwrap();
        let (_, protocols) = hello(&req, &connection).unwrap();
        let hello: Vec<_> = protocols.iter().map(|p| (p.namespace.as_str(), p.name.as_str(), p.number)).collect();
        assert_eq!(hello, [("chat/admin", "Chat", 7)]);

        // The same with the facts `/`-joined and the request `::`-joined.
        let mut req = nested_request(&["Chat"]);
        req.schemas[0].0 = "chat::admin".into();
        req.facts.as_mut().unwrap().protocols[0].namespace = "chat/admin".into();
        check(&req).unwrap();
    }

    #[test]
    fn a_nested_namespace_is_a_dependencys_in_either_spelling() {
        let dependencies = vec!["localdeps".to_string()];
        assert!(connections::is_dependency(&dependencies, "localdeps/a/b"));
        assert!(connections::is_dependency(&dependencies, "localdeps::a::b"));
        assert!(connections::is_dependency(&[], "std::http"));
        assert!(!connections::is_dependency(&dependencies, "chat::admin"));
        assert!(!connections::is_dependency(&dependencies, "localdeps"));
        assert!(!connections::is_dependency(&dependencies, "localdeps2::a"));
    }

    #[test]
    fn a_protocol_the_facts_lack_says_what_was_looked_for_and_what_they_hold() {
        // Facts for `chat::admin::Chat`, and a request for `chat/user`'s.
        let mut req = nested_request(&["Chat"]);
        req.schemas[0].0 = "chat/user".into();
        let error = check(&req).unwrap_err().to_string();
        assert!(error.contains("`chat/user::Chat`"), "{error}");
        assert!(error.contains("no protocol `Chat` in namespace `chat/user`"), "{error}");
        assert!(error.contains("`Chat`, in namespace `chat::admin`"), "{error}");
        assert!(error.contains("comline build") && error.contains("chat::admin::Chat"), "{error}");
        // It's the package's own, so there's nothing to wait for in core.
        assert!(!error.contains("core#277") && !error.contains("dependency"), "{error}");

        // Facts of no protocols at all.
        req.facts.as_mut().unwrap().protocols.clear();
        let error = check(&req).unwrap_err().to_string();
        assert!(error.contains("The facts hold: no protocols"), "{error}");

        // A dependency's has its own words.
        let mut req = request(&["Chat"]);
        req.facts = Some(facts_of(&["Chat"]));
        let (namespace, name, parameters, _) = foo(&req);
        let error = missing(req.facts.as_ref(), &req.dependencies, namespace, name, parameters).to_string();
        assert!(error.contains("a dependency's schema") && error.contains("core#277"), "{error}");
        assert!(!error.contains("comline build"), "{error}");

        // No facts at all is its own message.
        let error = missing(None, &req.dependencies, "api", "Chat", parameters).to_string();
        assert!(error.contains("carries no facts"), "{error}");
    }

    #[test]
    fn the_package_connection_carries_the_packages_protocols_only() {
        let mut req = request(&[]);
        assert_eq!(comline2_connection(&req).unwrap(), None);
        assert_eq!(connections::package_connection_among(&req.schemas, &req.dependencies).unwrap(), None);

        req = request(&["Chat"]);
        req.facts = Some(facts_of(&["Chat"]));
        let connection = comline2_connection(&req).unwrap().unwrap();
        let provided: Vec<_> = connection.server.iter().map(|p| (p.namespace.as_str(), p.name.as_str())).collect();
        assert_eq!(provided, [("api", "Chat")]);
        let (_, protocols) = hello(&req, &connection).unwrap();
        let hello: Vec<_> = protocols.iter().map(|p| (p.name.as_str(), p.number)).collect();
        assert_eq!(hello, [("Chat", 7)]);
    }

    #[test]
    fn the_hello_parts_give_the_hello_and_its_errors() {
        let mut req = request(&["Chat", "Other"]);
        req.facts = Some(facts_of(&["Chat", "Other"]));
        let connection = comline2_connection(&req).unwrap().unwrap();
        let (bytes, _) = hello(&req, &connection).unwrap();
        let parts = hello_parts(&req, &connection).unwrap();
        assert_eq!(parts.assemble_all(), bytes);
        let numbers: Vec<_> = parts.protocols.iter().map(|p| (p.name.as_str(), p.number)).collect();
        assert_eq!(numbers, [("Chat", 7), ("Other", 8)]);
        assert_eq!(parts.encodings.iter().map(|e| e.kind).collect::<Vec<_>>(), [0, 1, 2]);

        // The one listing fewer protocols lists exactly those.
        let other = parts.protocols.iter().filter(|p| p.name == "Other");
        let subset = parts.assemble(other);
        assert!(subset.len() < bytes.len());
        // One protocol: number 8, line 1.0.0, its shape hash, and function 3.
        let mut entry = vec![1, 8, 1, 0, 0];
        entry.extend([0x5a; 16]);
        entry.extend([1, 3]);
        assert!(subset.ends_with(&entry), "{subset:?}");

        // The same refusals, in the same words.
        let mut no_id = req.clone();
        no_id.facts.as_mut().unwrap().id = None;
        assert_eq!(
            hello_parts(&no_id, &connection).unwrap_err().to_string(),
            hello(&no_id, &connection).unwrap_err().to_string()
        );
        let mut no_facts = req.clone();
        no_facts.facts = None;
        assert_eq!(
            hello_parts(&no_facts, &connection).unwrap_err().to_string(),
            hello(&no_facts, &connection).unwrap_err().to_string()
        );
        let mut no_framing = req.clone();
        no_framing.facts.as_mut().unwrap().encodings.retain(|e| e.kind != "framing");
        assert_eq!(
            hello_parts(&no_framing, &connection).unwrap_err().to_string(),
            hello(&no_framing, &connection).unwrap_err().to_string()
        );
    }

    #[test]
    fn versions_and_hashes_parse() {
        assert_eq!(version("0.4.0-dev.2"), Some(((0, 4, 0), "dev.2")));
        assert_eq!(version("1.2.3"), Some(((1, 2, 3), "")));
        assert_eq!(version("1.2"), None);
        assert_eq!(hash128(&"ab".repeat(32)), Some([0xab; 16]));
        assert_eq!(hash128("ab"), None);
        let mut out = vec![];
        varint(300, &mut out);
        assert_eq!(out, [0xac, 0x02]);
    }
}
