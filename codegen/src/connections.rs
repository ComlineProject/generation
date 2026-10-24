//! Protocols that run on a connection: which ones, the number each carries
//! on the wire, and the hash a connection's handshake checks.
//!
//! A protocol runs on a `Connection` (the runtime's, calls in both
//! directions) when it uses `datagram@2`, or when the client provides it
//! (`@provider = Client` or `Multiple`), which core only allows with a
//! framing that tells requests from replies. JSON-RPC's frames look the same
//! either way, so once any JSON-RPC protocol in the package runs on a
//! connection, every JSON-RPC protocol does: the server's API shares the
//! connection its calls to the client use. The rest keep the lockstep
//! `Client` / `Server`.
//!
//! JSON-RPC names a call by its method, not by protocol and number: the
//! function's name (`bare`, the default), or `namespace.Protocol/function`
//! (`qualified`), as `@framing`'s `method_names` says. [`check`] refuses
//! bare names that would collide on one connection.
//!
//! Every generator numbers protocols the same way, here: under `comline@1`
//! a protocol's number is its position among the package's protocols, in
//! the order the request lists the schemas. Both ends were generated from
//! the same IR, which the handshake checks with [`connection_ir_hash`], so
//! they agree. `comline@2` will hand numbers out at the handshake instead.

use comline_core::schema::idl::annotations::{FramingValue, MethodNames, FRAMING_DATAGRAM2, FRAMING_JSONRPC};
use comline_core::schema::ir::frozen::schema_ir_hash;
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};
use eyre::{bail, Result};

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

/// A protocol's framing: its own `@framing` in either form, else the
/// package's `default_framing`. `None` when neither names a known one.
pub fn framing(parameters: &[FrozenUnit], default_framing: Option<&str>) -> Option<FramingValue> {
    let own = parameters.iter().find_map(|p| match p {
        FrozenUnit::Property { name, value } if name == "framing" => Some(value),
        _ => None,
    });
    match own {
        Some(value) => FramingValue::of(value),
        None => default_framing.and_then(FramingValue::named),
    }
}

/// Whether a protocol with these parameters, in a package of `schemas`,
/// runs on a connection.
pub fn on_connection(
    schemas: &[(String, Vec<FrozenUnit>)],
    parameters: &[FrozenUnit],
    default_framing: Option<&str>,
) -> bool {
    let name = framing(parameters, default_framing).map(|f| f.name);
    name == Some(FRAMING_DATAGRAM2)
        || Provider::of(parameters) != Provider::Server
        || (name == Some(FRAMING_JSONRPC) && jsonrpc_on_connection(schemas, default_framing))
}

/// Whether any JSON-RPC protocol in `schemas` runs on a connection, which
/// puts all of them on it.
fn jsonrpc_on_connection(schemas: &[(String, Vec<FrozenUnit>)], default_framing: Option<&str>) -> bool {
    protocols(schemas).any(|(_, _, parameters, _)| {
        framing(parameters, default_framing).map(|f| f.name) == Some(FRAMING_JSONRPC)
            && Provider::of(parameters) != Provider::Server
    })
}

/// The method JSON-RPC calls `function` of `protocol` (declared in the
/// schema `namespace`) by: `send`, or `chat.Chat/send` when qualified.
pub fn method_name(namespace: &str, protocol: &str, function: &str, names: MethodNames) -> String {
    match names {
        MethodNames::Bare => function.to_string(),
        MethodNames::Qualified => {
            format!("{}.{protocol}/{function}", namespace.replace("::", ".").replace('/', "."))
        }
    }
}

/// Refuses JSON-RPC protocols on one connection whose bare method names one
/// side couldn't tell apart: two protocols the same side provides, both
/// with a function of the same name.
pub fn check(schemas: &[(String, Vec<FrozenUnit>)], default_framing: Option<&str>) -> Result<()> {
    let problems = bare_name_clashes(schemas, default_framing);
    if !problems.is_empty() {
        bail!("{}", problems.join("\n"));
    }
    Ok(())
}

/// [`check`]'s problems, one sentence each.
pub fn bare_name_clashes(schemas: &[(String, Vec<FrozenUnit>)], default_framing: Option<&str>) -> Vec<String> {
    let mut problems = Vec::new();
    // Each side looks a method up among the protocols it provides.
    for (side, provides) in [
        ("server", [Provider::Server, Provider::Multiple]),
        ("client", [Provider::Client, Provider::Multiple]),
    ] {
        let mut seen: Vec<(&str, String)> = Vec::new();
        for (namespace, name, parameters, functions) in protocols(schemas) {
            let framing = framing(parameters, default_framing);
            let bare = framing.is_some_and(|f| f.name == FRAMING_JSONRPC && f.method_names == MethodNames::Bare);
            if !bare
                || !provides.contains(&Provider::of(parameters))
                || !on_connection(schemas, parameters, default_framing)
            {
                continue;
            }
            let protocol = format!("{namespace}::{name}");
            for function in functions.iter().filter_map(|f| match f {
                FrozenUnit::Function { name, .. } => Some(name.as_str()),
                _ => None,
            }) {
                match seen.iter().find(|(f, _)| *f == function) {
                    Some((_, other)) if *other != protocol => problems.push(format!(
                        "protocols `{other}` and `{protocol}` both have a function `{function}`, and the {side} \
                         provides both on one JSON-RPC connection, where bare method names can't tell them \
                         apart - use `@framing = {{ name = \"jsonrpc\", method_names = qualified }}` on either"
                    )),
                    Some(_) => {}
                    None => seen.push((function, protocol.clone())),
                }
            }
        }
    }
    problems
}

/// Every protocol in `schemas`: its namespace, name, parameters and
/// functions.
fn protocols(
    schemas: &[(String, Vec<FrozenUnit>)],
) -> impl Iterator<Item = (&str, &str, &[FrozenUnit], &[FrozenUnit])> {
    schemas.iter().flat_map(|(ns, units)| {
        units.iter().filter_map(move |u| match u {
            FrozenUnit::Protocol { name, parameters, functions, .. } => {
                Some((ns.as_str(), name.as_str(), parameters.as_slice(), functions.as_slice()))
            }
            _ => None,
        })
    })
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

    fn framing(f: &str) -> FrozenUnit {
        property("framing", ConfigValue::Str(f.to_string()))
    }

    fn provider(p: &str) -> FrozenUnit {
        property("provider", ConfigValue::Name(p.to_string()))
    }

    fn qualified() -> FrozenUnit {
        property(
            "framing",
            ConfigValue::Dict(vec![
                ("name".to_string(), ConfigValue::Str("jsonrpc".to_string())),
                ("method_names".to_string(), ConfigValue::Name("qualified".to_string())),
            ]),
        )
    }

    fn with_functions(name: &str, parameters: Vec<FrozenUnit>, functions: &[&str]) -> FrozenUnit {
        let FrozenUnit::Protocol { docstring, span, .. } = protocol(name, vec![]) else { unreachable!() };
        FrozenUnit::Protocol {
            docstring,
            parameters,
            name: name.to_string(),
            functions: functions
                .iter()
                .map(|f| FrozenUnit::Function {
                    docstring: String::new(),
                    name: f.to_string(),
                    arguments: vec![],
                    _return: None,
                    throws: vec![],
                    parameters: vec![],
                    span: (0, 0),
                })
                .collect(),
            span,
        }
    }

    #[test]
    fn datagram2_or_a_client_provider_puts_a_protocol_on_a_connection() {
        let alone = |parameters: Vec<FrozenUnit>, default: Option<&str>| {
            let schemas = vec![("app".to_string(), vec![protocol("P", parameters.clone())])];
            on_connection(&schemas, &parameters, default)
        };
        assert!(!alone(vec![], None));
        assert!(!alone(vec![framing("jsonrpc")], None));
        assert!(alone(vec![framing("datagram@2")], None));
        assert!(alone(vec![], Some("datagram@2")));
        assert!(!alone(vec![framing("datagram")], Some("datagram@2")));
        assert!(alone(vec![provider("Client"), framing("jsonrpc")], None));
        assert!(alone(vec![provider("Client"), qualified()], None));
        assert!(alone(vec![provider("Multiple")], None));
        assert!(!alone(vec![provider("Server")], None));
    }

    #[test]
    fn one_json_rpc_protocol_on_a_connection_takes_the_others_with_it() {
        let chat = vec![framing("json-rpc")];
        let schemas = |client: Vec<FrozenUnit>| {
            vec![
                ("chat".to_string(), vec![protocol("Chat", chat.clone())]),
                ("events".to_string(), vec![protocol("Events", client)]),
            ]
        };
        assert!(!on_connection(&schemas(vec![framing("jsonrpc")]), &chat, None));
        assert!(on_connection(&schemas(vec![provider("Client"), qualified()]), &chat, None));
        // A datagram@2 connection doesn't take JSON-RPC protocols along.
        assert!(!on_connection(&schemas(vec![provider("Client"), framing("datagram@2")]), &chat, None));
    }

    #[test]
    fn a_method_is_the_function_or_qualified_by_its_protocol() {
        assert_eq!(method_name("chat", "Chat", "send", MethodNames::Bare), "send");
        assert_eq!(method_name("chat", "Chat", "send", MethodNames::Qualified), "chat.Chat/send");
        assert_eq!(method_name("chat::admin", "Admin", "kick", MethodNames::Qualified), "chat.admin.Admin/kick");
    }

    #[test]
    fn bare_names_one_side_could_confuse_are_refused() {
        let chat = |parameters| with_functions("Chat", parameters, &["send", "count"]);
        let admin = |parameters| with_functions("Admin", parameters, &["send"]);
        let events = with_functions("Events", vec![provider("Client"), framing("jsonrpc")], &["send"]);

        // Both provided by the server, both bare: `send` is ambiguous.
        let clash = vec![("app".to_string(), vec![chat(vec![framing("jsonrpc")]), admin(vec![framing("jsonrpc")]), events.clone()])];
        let problems = bare_name_clashes(&clash, None);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("`app::Chat` and `app::Admin` both have a function `send`"), "{problems:?}");
        assert!(problems[0].contains("the server provides both"), "{problems:?}");
        assert!(check(&clash, None).is_err());

        // Qualifying either is enough; `Events` is the client's, so its
        // `send` never meets the server's.
        let fine = vec![("app".to_string(), vec![chat(vec![framing("jsonrpc")]), admin(vec![qualified()]), events.clone()])];
        assert!(check(&fine, None).is_ok());

        // Off a connection there's one protocol per server, so nothing clashes.
        let lockstep = vec![("app".to_string(), vec![chat(vec![framing("jsonrpc")]), admin(vec![framing("jsonrpc")])])];
        assert!(check(&lockstep, None).is_ok());
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
