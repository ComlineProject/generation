//! Which type implements each encoding in a target's language: the
//! resolution that fills [`GenerateRequest::encodings`] (core#120).
//!
//! An encoding (a format, a framing or a handshake) is declared in a
//! package's `config.idp`, the official ones by the embedded official
//! package, each with suggested mappings per language. A consumer may map or
//! override one in `comline.toml`. For one language, an encoding's mapping is:
//!
//! 1. the consumer's override, if there is one;
//! 2. else the declaring package's `map` for that language;
//! 3. else nothing, which is an error only if the package uses it: as its
//!    wire format, as a protocol's framing, or as a protocol's handshake.
//!
//! **Keys** are canonical spellings, the ones generators see in the IR: an
//! official encoding's canonical spelling as core resolves it (`jsonrpc@1`
//! is `jsonrpc`, `datagram@1` is `datagram`), any other name as declared.
//! The CLI and the playground both resolve through here, so they agree.

// Standard Uses
use std::collections::{BTreeMap, BTreeSet};

// Crate Uses
use crate::connections;
use crate::protocol::{EncodingMapping, GenerateRequest};

// External Uses
use comline_core::encodings::{official, EncodingKind};
use comline_core::package::config::ir::frozen::FrozenUnit as ConfigUnit;
use comline_core::schema::idl::annotations::{FORMAT_JSON, FRAMING_JSONRPC, HANDSHAKE_NONE};
use comline_core::schema::ir::frozen::unit::FrozenUnit;
use eyre::{bail, Result};

const KINDS: [EncodingKind; 3] = [EncodingKind::Format, EncodingKind::Framing, EncodingKind::Handshake];

/// The canonical spelling of an encoding name: an official encoding's, in
/// any of its spellings, as core resolves it (`json-rpc` and `jsonrpc@1` are
/// `jsonrpc`); any other name unchanged.
pub fn canonical(name: &str) -> String {
    KINDS.iter().find_map(|kind| official::resolve(*kind, name)).unwrap_or(name).to_string()
}

/// Every encoding a package uses, canonical: its wire format, and each
/// protocol's framing and handshake (`none` is no handshake, so not one).
/// A JSON-RPC protocol carries JSON whatever the wire format is, so it uses
/// `json@1` too.
pub fn used(schemas: &[(String, Vec<FrozenUnit>)], wire_format: &str) -> BTreeSet<String> {
    let mut used = BTreeSet::from([canonical(wire_format)]);
    for (_, _, parameters, _) in connections::protocols(schemas) {
        let framing = connections::framing_name(parameters);
        if framing == FRAMING_JSONRPC {
            used.insert(FORMAT_JSON.to_string());
        }
        used.insert(framing);
        let handshake = connections::handshake_name(parameters);
        if handshake != HANDSHAKE_NONE {
            used.insert(handshake);
        }
    }
    used
}

/// Resolve every encoding's mapping onto `language`, by canonical name.
///
/// `official` is the official package's declarations
/// ([`official::units`]), `declared` the frozen configs of the package and
/// its dependencies (anything but [`ConfigUnit::Encoding`] is skipped), and
/// `overrides` the consumer's mappings from `comline.toml`, keyed by any
/// spelling. `used` is what the package uses ([`used`]): each of those left
/// unmapped is an error, one sentence per encoding, naming the encoding, the
/// language and how to map it.
///
/// The result has every encoding that has a mapping, used or not.
pub fn resolve(
    language: &str,
    official: &[ConfigUnit],
    declared: &[ConfigUnit],
    overrides: &BTreeMap<String, EncodingMapping>,
    used: &BTreeSet<String>,
) -> Result<BTreeMap<String, EncodingMapping>> {
    let mut resolved = BTreeMap::new();
    for unit in official.iter().chain(declared) {
        let ConfigUnit::Encoding { name, map, .. } = unit else { continue };
        if let Some(m) = map.get(language) {
            resolved.insert(
                canonical(name),
                EncodingMapping { type_path: m.ty.clone(), package: m.package.clone(), version: m.version.clone() },
            );
        }
    }
    for (name, mapping) in overrides {
        resolved.insert(canonical(name), mapping.clone());
    }
    let missing: Vec<&String> = used.iter().filter(|name| !resolved.contains_key(*name)).collect();
    if !missing.is_empty() {
        let lines: Vec<String> = missing
            .iter()
            .map(|name| {
                format!(
                    "encoding `{name}` has no {language} mapping: map it in comline.toml, under `[generate]` \
                     or the target, as `encodings = {{ \"{name}\" = \"<type>\" }}` or \
                     `encodings = {{ \"{name}\" = {{ type = \"<type>\", package = \"<package>\", version = \"<version>\" }} }}`"
                )
            })
            .collect();
        bail!("{}", lines.join("\n"));
    }
    Ok(resolved)
}

/// What a generator checks before rendering: every encoding the request's
/// package uses has a mapping in `req.encodings`. One sentence per missing
/// encoding, naming it and the target's language; after this, a generator
/// may look any used encoding up and rely on finding it.
pub fn check(req: &GenerateRequest) -> Result<()> {
    let missing: Vec<String> = used(&req.schemas, &req.wire_format)
        .into_iter()
        .filter(|name| !req.encodings.contains_key(name))
        .map(|name| {
            format!(
                "encoding `{name}` has no {} mapping in the request: map it in comline.toml, as \
                 `encodings = {{ \"{name}\" = \"<type>\" }}`",
                req.target.language
            )
        })
        .collect();
    if !missing.is_empty() {
        bail!("{}", missing.join("\n"));
    }
    Ok(())
}

/// The type a used encoding maps to. Only for a name [`check`] has
/// already vouched for.
pub fn type_of<'a>(encodings: &'a BTreeMap<String, EncodingMapping>, name: &str) -> &'a str {
    match encodings.get(name) {
        Some(mapping) => &mapping.type_path,
        None => panic!("encoding `{name}` wasn't checked: call encodings::check first"),
    }
}

/// Fill `req.encodings` for its target's language: [`resolve`] over what
/// the request's package uses.
pub fn fill(
    req: &mut GenerateRequest,
    official: &[ConfigUnit],
    declared: &[ConfigUnit],
    overrides: &BTreeMap<String, EncodingMapping>,
) -> Result<()> {
    let used = used(&req.schemas, &req.wire_format);
    req.encodings = resolve(&req.target.language, official, declared, overrides, &used)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use comline_core::encodings::EncodingMapping as Suggested;
    use comline_core::schema::ir::frozen::unit::ConfigValue;

    fn mapping(ty: &str) -> EncodingMapping {
        EncodingMapping { type_path: ty.into(), package: None, version: None }
    }

    fn declared(name: &str, kind: EncodingKind, rust: Option<&str>) -> ConfigUnit {
        let mut map = BTreeMap::new();
        if let Some(ty) = rust {
            map.insert("rust".to_string(), Suggested { ty: ty.into(), package: Some("acme".into()), version: Some("1".into()) });
        }
        ConfigUnit::Encoding { name: name.into(), kind, profile_hash: String::new(), map }
    }

    fn protocol(framing: &str) -> FrozenUnit {
        FrozenUnit::Protocol {
            docstring: String::new(),
            parameters: vec![FrozenUnit::Property { name: "framing".into(), value: ConfigValue::Str(framing.into()) }],
            name: "P".into(),
            functions: vec![],
            span: (0, 0),
        }
    }

    #[test]
    fn official_keys_are_canonical() {
        assert_eq!(canonical("jsonrpc@1"), "jsonrpc");
        assert_eq!(canonical("json-rpc"), "jsonrpc");
        assert_eq!(canonical("datagram@1"), "datagram");
        assert_eq!(canonical("datagram@2"), "datagram@2");
        assert_eq!(canonical("com.acme.lines@1"), "com.acme.lines@1");
        let all = resolve("rust", official::units(), &[], &BTreeMap::new(), &BTreeSet::new()).unwrap();
        let keys: Vec<&str> = all.keys().map(String::as_str).collect();
        assert_eq!(keys, ["comline@1", "datagram", "datagram@2", "json@1", "jsonrpc", "msgpack@1", "packed@1"]);
        assert_eq!(all["jsonrpc"].type_path, "comline_runtime::framing::JsonRpcFraming");
    }

    #[test]
    fn an_override_beats_the_declaration() {
        let declared = [declared("com.acme.lines@1", EncodingKind::Framing, Some("acme::Lines"))];
        let used = BTreeSet::from(["com.acme.lines@1".to_string()]);
        let suggested = resolve("rust", official::units(), &declared, &BTreeMap::new(), &used).unwrap();
        assert_eq!(suggested["com.acme.lines@1"].type_path, "acme::Lines");
        assert_eq!(suggested["com.acme.lines@1"].package.as_deref(), Some("acme"));
        let overrides = BTreeMap::from([("com.acme.lines@1".to_string(), mapping("mine::Lines"))]);
        let overridden = resolve("rust", official::units(), &declared, &overrides, &used).unwrap();
        assert_eq!(overridden["com.acme.lines@1"], mapping("mine::Lines"));
        // An override of an official name, by its declared spelling, lands on
        // the canonical key.
        let overrides = BTreeMap::from([("jsonrpc@1".to_string(), mapping("mine::Rpc"))]);
        let overridden = resolve("rust", official::units(), &[], &overrides, &BTreeSet::new()).unwrap();
        assert_eq!(overridden["jsonrpc"], mapping("mine::Rpc"));
        assert!(!overridden.contains_key("jsonrpc@1"));
    }

    #[test]
    fn an_unmapped_encoding_is_an_error_only_when_used() {
        let declared = [declared("com.acme.lines@1", EncodingKind::Framing, None)];
        assert!(resolve("rust", official::units(), &declared, &BTreeMap::new(), &BTreeSet::new()).is_ok());
        let used = BTreeSet::from(["com.acme.lines@1".to_string()]);
        let err = resolve("rust", official::units(), &declared, &BTreeMap::new(), &used).unwrap_err().to_string();
        assert!(err.starts_with("encoding `com.acme.lines@1` has no rust mapping: map it in comline.toml"), "{err}");
        // MessagePack has no TypeScript mapping.
        let used = BTreeSet::from(["msgpack@1".to_string()]);
        let err = resolve("typescript", official::units(), &[], &BTreeMap::new(), &used).unwrap_err().to_string();
        assert!(err.contains("`msgpack@1` has no typescript mapping"), "{err}");
    }

    #[test]
    fn used_names_the_format_framings_and_handshakes() {
        let schemas = vec![("app".to_string(), vec![protocol("com.acme.lines@1"), protocol("json-rpc")])];
        let used: Vec<String> = used(&schemas, "packed@1").into_iter().collect();
        assert_eq!(used, ["com.acme.lines@1", "comline@1", "json@1", "jsonrpc", "packed@1"]);
    }
}
