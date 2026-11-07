//! The facts a frozen version records for `comline@2` and for framings that
//! call by wire number (`datagram@3`, `calls = by_id`): what
//! [`GenerateRequest::facts`] carries (runtime#15).
//!
//! Core computes them from the package's history (`comline_core::compat`),
//! which a generator never sees: it reads frozen IR only, without core's
//! compiler. These types mirror core's `PackageFacts`, field for field and
//! with the same JSON, so `comline generate` hands them over as they are.
//!
//! **Numbers are never guessed.** A protocol that needs them and has none in
//! the facts is refused ([`check`]): falling back to its position would make
//! two versions of a package call each other's functions by mistake.

// Standard Uses
use std::collections::BTreeMap;

// Crate Uses
use crate::connections::{self, PackageConnection};
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
/// exactly as before.
pub fn needed(schemas: &[(String, Vec<FrozenUnit>)], encodings: &BTreeMap<String, EncodingMapping>) -> bool {
    connections::protocols(schemas).any(|(_, _, parameters, _)| needed_by(encodings, parameters))
}

/// The facts of protocol `namespace::name`, if the request has them.
pub fn protocol<'a>(req_facts: Option<&'a PackageFacts>, namespace: &str, name: &str) -> Option<&'a ProtocolFacts> {
    req_facts?.protocols.iter().find(|p| p.namespace == namespace && p.name == name)
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
/// Refuses a protocol that needs numbers the request doesn't have.
pub fn addressing(
    req_facts: Option<&PackageFacts>,
    schemas: &[(String, Vec<FrozenUnit>)],
    encodings: &BTreeMap<String, EncodingMapping>,
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
    let by_id = calls_by_id(encodings, &connections::framing_name(parameters));
    if !needed_by(encodings, parameters) {
        let protocol = connections::protocol_number(schemas, namespace, name).ok_or_else(|| {
            eyre!("protocol `{name}` has no number: more than 65536 protocols in the package")
        })?;
        return Ok(Addressing { protocol, calls: (0..names.len() as u16).collect(), checks: None, by_id });
    }
    let facts = protocol(req_facts, namespace, name).ok_or_else(|| missing(req_facts, namespace, name, parameters))?;
    let numbers: Vec<u16> = names
        .iter()
        .map(|f| {
            facts.functions.iter().find(|ff| ff.name == *f).map(|ff| ff.number).ok_or_else(|| {
                eyre!(
                    "protocol `{namespace}::{name}`, function `{f}`: the facts have no wire number for it, so they \
                     are older than the schema; run `comline build`, then generate again"
                )
            })
        })
        .collect::<Result<_>>()?;
    let calls = if by_id { numbers.clone() } else { (0..names.len() as u16).collect() };
    let checks = (connections::handshake_name(parameters) == HANDSHAKE_COMLINE2).then_some(numbers);
    Ok(Addressing { protocol: facts.number, calls, checks, by_id })
}

fn missing(req_facts: Option<&PackageFacts>, namespace: &str, name: &str, parameters: &[FrozenUnit]) -> eyre::Report {
    let why = format!(
        "protocol `{namespace}::{name}` uses `{}` with `{}`, which address it by the wire numbers its package's \
         history assigns",
        connections::framing_name(parameters),
        connections::handshake_name(parameters)
    );
    match req_facts {
        None => eyre!(
            "{why}, and the request carries no facts: `comline generate` supplies them from the package's build \
             history (run `comline build` first)"
        ),
        Some(_) => eyre!(
            "{why}, and the package's facts have none for it: a dependency's protocol has its own package's \
             numbers, and a new protocol has none until `comline build` records it"
        ),
    }
}

/// What a generator checks before rendering a request that needs facts:
/// every protocol that needs numbers has them; `comline@2` has a package id,
/// a version and one package connection carrying every protocol that runs
/// it, with one set of parameters; and no protocol off a connection runs it.
pub fn check(req: &GenerateRequest) -> Result<()> {
    if !needed(&req.schemas, &req.encodings) {
        return Ok(());
    }
    let mut problems = vec![];
    for (namespace, name, parameters, functions) in connections::protocols(&req.schemas) {
        if !needed_by(&req.encodings, parameters) {
            continue;
        }
        if let Err(e) =
            addressing(req.facts.as_ref(), &req.schemas, &req.encodings, namespace, name, parameters, functions)
        {
            problems.push(e.to_string());
            continue;
        }
        let on = connections::on_connection(&req.schemas, parameters);
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
    let runs = connections::protocols(&req.schemas).any(|(_, _, parameters, _)| {
        connections::handshake_name(parameters) == HANDSHAKE_COMLINE2
            && connections::on_connection(&req.schemas, parameters)
    });
    if !runs {
        return Ok(None);
    }
    match connections::package_connection(&req.schemas) {
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
        .find(|(_, _, parameters, _)| {
            connections::handshake_name(parameters) == HANDSHAKE_COMLINE2
                && connections::on_connection(&req.schemas, parameters)
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
/// It covers the protocols on the package connection that the package
/// itself declares, and the encodings that connection uses: the package's
/// format (JSON on a framing that writes it), the connection's framing, and
/// `comline@2` itself.
pub fn hello(req: &GenerateRequest, connection: &PackageConnection) -> Result<(Vec<u8>, Vec<HelloProtocol>)> {
    let facts = req.facts.as_ref().ok_or_else(|| {
        eyre!(
            "the package's connection runs `comline@2`, and the request carries no facts: `comline generate` \
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
        if !connections::on_connection(&req.schemas, parameters) {
            continue;
        }
        let Some(p) = protocol(Some(facts), namespace, name) else {
            // A dependency's protocol: not this package's to declare.
            continue;
        };
        let addressing = addressing(Some(facts), &req.schemas, &req.encodings, namespace, name, parameters, functions)?;
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
