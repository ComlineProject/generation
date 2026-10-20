//! Protocols that run on a connection: which ones, the number each carries
//! on the wire, and the hash a connection's handshake checks.
//!
//! A protocol runs on a `Connection` (the runtime's, calls in both
//! directions) when it uses `datagram@2`, or when the client provides it
//! (`@provider = Client` or `Multiple`), which core only allows with a
//! framing that tells requests from replies. The rest keep the lockstep
//! `Client` / `Server`.
//!
//! Every generator numbers protocols the same way, here: under `comline@1`
//! a protocol's number is its position among the package's protocols, in
//! the order the request lists the schemas. Both ends were generated from
//! the same IR, which the handshake checks with [`connection_ir_hash`], so
//! they agree. `comline@2` will hand numbers out at the handshake instead.

use comline_core::schema::idl::annotations::FRAMING_DATAGRAM2;
use comline_core::schema::ir::frozen::schema_ir_hash;
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};

/// Which side implements a protocol (`@provider`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// The side that accepted the connection. The default.
    Server,
    /// The side that dialled: the server calls it.
    Client,
    /// Both sides: calls go either way.
    Multiple,
}

impl Provider {
    pub fn of(parameters: &[FrozenUnit]) -> Provider {
        match property(parameters, "provider") {
            Some("Client") => Provider::Client,
            Some("Multiple") => Provider::Multiple,
            _ => Provider::Server,
        }
    }
}

/// Whether a protocol with these parameters runs on a connection, its own
/// `@framing` first, then the package's `default_framing`.
pub fn on_connection(parameters: &[FrozenUnit], default_framing: Option<&str>) -> bool {
    let framing = property(parameters, "framing").or(default_framing).and_then(|f| {
        comline_core::schema::idl::annotations::canonical_value("framing", f)
    });
    framing == Some(FRAMING_DATAGRAM2) || Provider::of(parameters) != Provider::Server
}

/// The number `protocol`, declared in the schema `namespace`, carries on a
/// connection: its position among every protocol in `schemas`.
pub fn protocol_number(
    schemas: &[(String, Vec<FrozenUnit>)],
    namespace: &str,
    protocol: &str,
) -> Option<u16> {
    schemas
        .iter()
        .flat_map(|(ns, units)| {
            units.iter().filter_map(move |u| match u {
                FrozenUnit::Protocol { name, .. } => Some((ns.as_str(), name.as_str())),
                _ => None,
            })
        })
        .position(|(ns, name)| ns == namespace && name == protocol)
        .and_then(|i| u16::try_from(i).ok())
}

/// The digest a connection's `comline@1` handshake checks: every schema in
/// `schemas`, in order, so both ends agree on all of them and on the
/// numbers [`protocol_number`] gives their protocols.
pub fn connection_ir_hash(schemas: &[(String, Vec<FrozenUnit>)]) -> u64 {
    let all: Vec<FrozenUnit> = schemas.iter().flat_map(|(_, units)| units.iter().cloned()).collect();
    schema_ir_hash(&all)
}

fn property<'a>(parameters: &'a [FrozenUnit], key: &str) -> Option<&'a str> {
    parameters.iter().find_map(|p| match p {
        FrozenUnit::Property { name, value: ConfigValue::Str(s) | ConfigValue::Name(s) } if name == key => {
            Some(s.as_str())
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn protocol(name: &str, parameters: Vec<FrozenUnit>) -> FrozenUnit {
        FrozenUnit::Protocol {
            docstring: String::new(),
            parameters,
            name: name.to_string(),
            functions: vec![],
            span: (0, 0),
        }
    }

    fn property(name: &str, value: ConfigValue) -> FrozenUnit {
        FrozenUnit::Property { name: name.to_string(), value }
    }

    #[test]
    fn numbers_count_every_protocol_in_schema_order() {
        let schemas = vec![
            ("chat".to_string(), vec![protocol("Chat", vec![]), protocol("Admin", vec![])]),
            ("models".to_string(), vec![]),
            ("events".to_string(), vec![protocol("Events", vec![])]),
        ];
        assert_eq!(protocol_number(&schemas, "chat", "Chat"), Some(0));
        assert_eq!(protocol_number(&schemas, "chat", "Admin"), Some(1));
        assert_eq!(protocol_number(&schemas, "events", "Events"), Some(2));
        assert_eq!(protocol_number(&schemas, "events", "Chat"), None);
    }

    #[test]
    fn datagram2_or_a_client_provider_puts_a_protocol_on_a_connection() {
        let framing = |f: &str| property("framing", ConfigValue::Str(f.to_string()));
        let provider = |p: &str| property("provider", ConfigValue::Name(p.to_string()));
        assert!(!on_connection(&[], None));
        assert!(!on_connection(&[framing("jsonrpc")], None));
        assert!(on_connection(&[framing("datagram@2")], None));
        assert!(on_connection(&[], Some("datagram@2")));
        assert!(!on_connection(&[framing("datagram")], Some("datagram@2")));
        assert!(on_connection(&[provider("Client"), framing("jsonrpc")], None));
        assert!(on_connection(&[provider("Multiple")], None));
        assert!(!on_connection(&[provider("Server")], None));
    }

    #[test]
    fn the_connection_hash_covers_every_schema() {
        let one = vec![("a".to_string(), vec![protocol("A", vec![])])];
        let mut two = one.clone();
        two.push(("b".to_string(), vec![protocol("B", vec![])]));
        assert_ne!(connection_ir_hash(&one), connection_ir_hash(&two));
        assert_eq!(connection_ir_hash(&two), connection_ir_hash(&two.clone()));
    }
}
