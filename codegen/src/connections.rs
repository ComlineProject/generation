//! Protocols that run on a connection: which ones, the number each carries
//! on the wire, and the hash a connection's handshake checks.
//!
//! A protocol runs on a `Connection` (the runtime's, calls in both
//! directions) when it uses `datagram@2`, the default, or when the client
//! provides it (`@provider = Client` or `Multiple`), which core only allows
//! with a framing that tells requests from replies. JSON-RPC's frames look the same
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

use comline_core::schema::idl::annotations::{
    FramingValue, MethodNames, DEFAULT_FRAMING, DEFAULT_HANDSHAKE, FRAMING_DATAGRAM2, FRAMING_JSONRPC,
};
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

/// A protocol's framing: its `@framing` in either form, else `datagram@2`.
/// Core has already written the package's `wire.framing` into every
/// protocol without its own, so this is the whole resolution.
pub fn framing(parameters: &[FrozenUnit]) -> FramingValue {
    FramingValue::of_protocol(parameters)
        .or_else(|| FramingValue::named(DEFAULT_FRAMING))
        .expect("the default framing is a known one")
}

/// A protocol's framing by name, canonical: an official one in its
/// canonical spelling (`jsonrpc`, `datagram@2`), a namespaced one as
/// declared (`com.acme.lines@1`), else `datagram@2`. Unlike [`framing`],
/// which only knows the official framings, this is every framing core lets
/// through: what a generator looks up in the request's `encodings`.
pub fn framing_name(parameters: &[FrozenUnit]) -> String {
    if let Some(value) = FramingValue::of_protocol(parameters) {
        return value.name.to_string();
    }
    let written = parameters.iter().find_map(|p| match p {
        FrozenUnit::Property { name, value } if name == "framing" => match value {
            ConfigValue::Str(s) | ConfigValue::Name(s) => Some(s.as_str()),
            ConfigValue::Dict(_) => match value.entry("name") {
                Some(ConfigValue::Str(s)) => Some(s.as_str()),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    });
    written.map(crate::encodings::canonical).unwrap_or_else(|| DEFAULT_FRAMING.to_string())
}

/// A protocol's handshake by name, canonical: `comline@1` (the default),
/// `none`, or a namespaced one as declared.
pub fn handshake_name(parameters: &[FrozenUnit]) -> String {
    property(parameters, "handshake").map(crate::encodings::canonical).unwrap_or_else(|| DEFAULT_HANDSHAKE.to_string())
}

/// Whether a protocol with these parameters, in a package of `schemas`,
/// runs on a connection.
pub fn on_connection(schemas: &[(String, Vec<FrozenUnit>)], parameters: &[FrozenUnit]) -> bool {
    let name = framing_name(parameters);
    name == FRAMING_DATAGRAM2
        || Provider::of(parameters) != Provider::Server
        || (name == FRAMING_JSONRPC && jsonrpc_on_connection(schemas))
}

/// Whether any JSON-RPC protocol in `schemas` runs on a connection, which
/// puts all of them on it.
fn jsonrpc_on_connection(schemas: &[(String, Vec<FrozenUnit>)]) -> bool {
    protocols(schemas).any(|(_, _, parameters, _)| {
        framing_name(parameters) == FRAMING_JSONRPC && Provider::of(parameters) != Provider::Server
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
pub fn check(schemas: &[(String, Vec<FrozenUnit>)]) -> Result<()> {
    let problems = bare_name_clashes(schemas);
    if !problems.is_empty() {
        bail!("{}", problems.join("\n"));
    }
    Ok(())
}

/// [`check`]'s problems, one sentence each.
pub fn bare_name_clashes(schemas: &[(String, Vec<FrozenUnit>)]) -> Vec<String> {
    let mut problems = Vec::new();
    // Each side looks a method up among the protocols it provides.
    for (side, provides) in [
        ("server", [Provider::Server, Provider::Multiple]),
        ("client", [Provider::Client, Provider::Multiple]),
    ] {
        let mut seen: Vec<(&str, String)> = Vec::new();
        for (namespace, name, parameters, functions) in protocols(schemas) {
            let framing = framing(parameters);
            let bare = framing.name == FRAMING_JSONRPC && framing.method_names == MethodNames::Bare;
            if !bare
                || !provides.contains(&Provider::of(parameters))
                || !on_connection(schemas, parameters)
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

/// One protocol a side provides on the package's connection, for the
/// `Services` a generator emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provided {
    /// The schema it's declared in.
    pub namespace: String,
    pub name: String,
    /// Its field in `Services`, snake case: the protocol's name, prefixed
    /// with its namespace only where two the same side provides would
    /// otherwise share one.
    pub field: String,
}

/// The package's one connection: how it frames and greets, and what each
/// side provides on it. A generator emits `Services` and a package-level
/// `serve` / `connect` from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageConnection {
    /// Canonical: `datagram@2`, `jsonrpc`, or a namespaced framing.
    pub framing: String,
    /// Canonical: `comline@1` or `none`.
    pub handshake: String,
    /// What the accepting side provides (`@provider = Server`, the default,
    /// and `Multiple`).
    pub server: Vec<Provided>,
    /// What the dialling side provides (`Client` and `Multiple`).
    pub client: Vec<Provided>,
}

/// The package's connection, when its protocols on one agree on a framing
/// and a handshake. `Ok(None)` when no protocol is on a connection; `Err`,
/// one sentence, when they disagree, since no single connection carries
/// them.
pub fn package_connection(schemas: &[(String, Vec<FrozenUnit>)]) -> Result<Option<PackageConnection>, String> {
    let on: Vec<_> = protocols(schemas).filter(|(_, _, parameters, _)| on_connection(schemas, parameters)).collect();
    let Some((first_ns, first, first_parameters, _)) = on.first() else {
        return Ok(None);
    };
    let framing_name = self::framing_name(first_parameters);
    let handshake_name = self::handshake_name(first_parameters);
    for (namespace, name, parameters, _) in &on[1..] {
        let (f, h) = (self::framing_name(parameters), self::handshake_name(parameters));
        if f != framing_name {
            return Err(format!(
                "`{first_ns}::{first}` is on a `{framing_name}` connection and `{namespace}::{name}` on a `{f}` \
                 one, so there's no one connection to serve them on"
            ));
        }
        if h != handshake_name {
            return Err(format!(
                "`{first_ns}::{first}` runs the `{handshake_name}` handshake and `{namespace}::{name}` the `{h}` \
                 one, so there's no one connection to serve them on"
            ));
        }
    }
    let side = |sides: [Provider; 2]| {
        let provided: Vec<_> =
            on.iter().filter(|(_, _, parameters, _)| sides.contains(&Provider::of(parameters))).collect();
        provided
            .iter()
            .map(|(namespace, name, _, _)| {
                let clash = provided.iter().filter(|(_, other, _, _)| other == name).count() > 1;
                let field = match clash {
                    true => format!("{}_{}", namespace.replace("::", "_").replace('/', "_"), crate::naming::snake(name)),
                    false => crate::naming::snake(name),
                };
                Provided { namespace: namespace.to_string(), name: name.to_string(), field }
            })
            .collect()
    };
    Ok(Some(PackageConnection {
        framing: framing_name,
        handshake: handshake_name,
        server: side([Provider::Server, Provider::Multiple]),
        client: side([Provider::Client, Provider::Multiple]),
    }))
}

/// Every protocol in `schemas`: its namespace, name, parameters and
/// functions.
pub(crate) fn protocols(
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
    fn datagram2_by_default_or_a_client_provider_puts_a_protocol_on_a_connection() {
        let alone = |parameters: Vec<FrozenUnit>| {
            let schemas = vec![("app".to_string(), vec![protocol("P", parameters.clone())])];
            on_connection(&schemas, &parameters)
        };
        assert!(alone(vec![]));
        assert!(alone(vec![provider("Server")]));
        assert!(alone(vec![framing("datagram@2")]));
        assert!(!alone(vec![framing("datagram")]));
        assert!(!alone(vec![framing("datagram@1")]));
        assert!(!alone(vec![framing("jsonrpc")]));
        assert!(alone(vec![provider("Client"), framing("jsonrpc")]));
        assert!(alone(vec![provider("Client"), qualified()]));
        assert!(alone(vec![provider("Multiple")]));
    }

    #[test]
    fn the_package_connection_says_what_each_side_provides() {
        let schemas = vec![
            ("game".to_string(), vec![protocol("Account", vec![]), protocol("Player", vec![provider("Multiple")])]),
            ("game::admin".to_string(), vec![protocol("Player", vec![]), protocol("Events", vec![provider("Client")])]),
            ("legacy".to_string(), vec![protocol("Old", vec![framing("datagram")])]),
        ];
        let c = package_connection(&schemas).unwrap().unwrap();
        assert_eq!((c.framing.as_str(), c.handshake.as_str()), ("datagram@2", "comline@1"));
        let fields = |side: &[Provided]| side.iter().map(|p| p.field.clone()).collect::<Vec<_>>();
        // Two `Player`s on the server side: both prefixed; the lone one on
        // the client side isn't.
        assert_eq!(fields(&c.server), ["account", "game_player", "game_admin_player"]);
        assert_eq!(fields(&c.client), ["player", "events"]);

        assert_eq!(package_connection(&[("legacy".to_string(), vec![protocol("Old", vec![framing("datagram")])])]), Ok(None));
    }

    #[test]
    fn protocols_that_disagree_have_no_package_connection() {
        let none = property("handshake", ConfigValue::Str("none".to_string()));
        let schemas =
            vec![("app".to_string(), vec![protocol("A", vec![]), protocol("B", vec![none])])];
        let err = package_connection(&schemas).unwrap_err();
        assert!(err.contains("`app::A` runs the `comline@1` handshake and `app::B` the `none` one"), "{err}");

        let schemas = vec![(
            "app".to_string(),
            vec![protocol("A", vec![]), protocol("B", vec![provider("Client"), framing("jsonrpc")])],
        )];
        let err = package_connection(&schemas).unwrap_err();
        assert!(err.contains("`datagram@2` connection and `app::B` on a `jsonrpc` one"), "{err}");
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
        assert!(!on_connection(&schemas(vec![framing("jsonrpc")]), &chat));
        assert!(on_connection(&schemas(vec![provider("Client"), qualified()]), &chat));
        // A datagram@2 connection doesn't take JSON-RPC protocols along.
        assert!(!on_connection(&schemas(vec![provider("Client"), framing("datagram@2")]), &chat));
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
        let problems = bare_name_clashes(&clash);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("`app::Chat` and `app::Admin` both have a function `send`"), "{problems:?}");
        assert!(problems[0].contains("the server provides both"), "{problems:?}");
        assert!(check(&clash).is_err());

        // Qualifying either is enough; `Events` is the client's, so its
        // `send` never meets the server's.
        let fine = vec![("app".to_string(), vec![chat(vec![framing("jsonrpc")]), admin(vec![qualified()]), events.clone()])];
        assert!(check(&fine).is_ok());

        // Off a connection there's one protocol per server, so nothing clashes.
        let lockstep = vec![("app".to_string(), vec![chat(vec![framing("jsonrpc")]), admin(vec![framing("jsonrpc")])])];
        assert!(check(&lockstep).is_ok());
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
