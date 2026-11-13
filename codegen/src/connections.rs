//! Protocols that run on a connection: which ones, the number each carries
//! on the wire, and the hash a connection's handshake checks.
//!
//! A protocol runs on a `Connection` (the runtime's, calls in both
//! directions) by what its framing's frames are, not by the framing's name:
//!
//! - By default when the framing's frames go both ways (`directions: both`)
//!   and name the protocol (`protocol: named`): `datagram@3`, the default, and
//!   `datagram@2`.
//! - Otherwise when the client provides it (`@provider = Client` or
//!   `Multiple`), which core only allows with a framing that tells requests
//!   from replies.
//! - On a framing whose frames go both ways but don't name the protocol
//!   (`jsonrpc@1`), the frames look the same for every protocol. So once any
//!   protocol on that framing is client-provided, every protocol on it shares
//!   the connection: the server's API shares the connection its calls to the
//!   client use.
//!
//! The rest keep the lockstep `Client` / `Server`.
//!
//! The facts are the official framings' (`comline_core::encodings::official`).
//! A framing of a package's own has none here yet: the IR doesn't carry them.
//! Its protocols run on a connection only when each is client-provided
//! itself, and one that is takes no other protocol on its framing along,
//! since nothing says its frames go both ways without naming the protocol.
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
//! they agree. A protocol that needs the package's facts (`datagram@3`,
//! which calls by wire number, or `comline@2`) carries its wire number
//! instead: see [`crate::facts::addressing`].

use crate::protocol::EncodingMapping;
use std::collections::BTreeMap;

use comline_core::encodings::{official, EncodingKind};
use comline_core::schema::idl::annotations::{
    FramingValue, HandshakeValue, MethodNames, IMPLICIT_FRAMING,
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
/// Core has already written the package's `wire.framing`, or its own
/// default (`datagram@3`), into every protocol without its own, unless it's
/// `datagram@2`, what an absent one means
/// ([`IMPLICIT_FRAMING`]), so this is the whole resolution.
pub fn framing(parameters: &[FrozenUnit]) -> FramingValue {
    FramingValue::of_protocol(parameters)
        .or_else(|| FramingValue::named(IMPLICIT_FRAMING))
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
    written
        .map(crate::encodings::canonical)
        .unwrap_or_else(|| IMPLICIT_FRAMING.to_string())
}

/// A protocol's handshake by name, canonical: `comline@1` (what an absent
/// one means: core freezes `comline@2`, its default, in),
/// `comline@2`, `none`, or a namespaced one as declared. Either form of
/// `@handshake`: the name, or a dictionary with its parameters.
pub fn handshake_name(parameters: &[FrozenUnit]) -> String {
    match property(parameters, "handshake") {
        Some(name) => crate::encodings::canonical(name),
        None => crate::encodings::canonical(&HandshakeValue::of_protocol(parameters).name),
    }
}

/// A protocol's handshake with its parameters, as [`HandshakeValue`] reads
/// it: a parameter written as its default is the same as one left out.
pub fn handshake_value(parameters: &[FrozenUnit]) -> HandshakeValue {
    match property(parameters, "handshake") {
        Some(name) => HandshakeValue::named(&crate::encodings::canonical(name)),
        None => {
            let mut value = HandshakeValue::of_protocol(parameters);
            value.name = crate::encodings::canonical(&value.name);
            value
        }
    }
}

/// Whether a protocol with these parameters, in a package of `schemas`,
/// runs on a connection: the client provides it, or its framing's frames go
/// both ways and name the protocol, or they go both ways without naming it
/// and some protocol on the framing is client-provided (see the module).
pub fn on_connection(schemas: &[(String, Vec<FrozenUnit>)], parameters: &[FrozenUnit]) -> bool {
    if Provider::of(parameters) != Provider::Server {
        return true;
    }
    let name = framing_name(parameters);
    // A framing with no official profile states no facts here: it's a
    // connection only for a protocol the client provides.
    let Some(profile) = official::profile(EncodingKind::Framing, &name) else {
        return false;
    };
    if profile.both_directions() != Some(true) {
        return false;
    }
    profile.names_protocol() == Some(true) || client_provided_on(schemas, &name)
}

/// Whether any protocol in `schemas` on the framing `name` is client-provided,
/// which puts all of them on a connection when the framing's frames don't name
/// the protocol.
fn client_provided_on(schemas: &[(String, Vec<FrozenUnit>)], name: &str) -> bool {
    protocols(schemas).any(|(_, _, parameters, _)| {
        framing_name(parameters) == name && Provider::of(parameters) != Provider::Server
    })
}

/// Why a framing keeps a protocol that isn't client-provided off a
/// connection, as [`why_lockstep`] reads it. Each variant's text is the
/// sentence to show the person who wrote the schemas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lockstep {
    /// Its frames go one way (`directions: one`), so the server can't call the
    /// client on it: no protocol on it runs on a connection.
    OneDirection,
    /// Its frames go both ways but don't name the protocol, so it's a
    /// connection only once some protocol on it is client-provided, and none
    /// is: every protocol on it is the server's, one call at a time.
    UnnamedServerProvided,
    /// It's a framing of the package's own, so it states no facts here: a
    /// protocol on it runs on a connection only when it's client-provided.
    Unknown,
}

/// Why the framing `name` (any spelling) keeps its protocols off a connection
/// unless the client provides them, for a note on one that has none; `None`
/// for a framing that doesn't (frames both ways, naming the protocol:
/// `datagram@2`, `datagram@3`), where a protocol is on a connection already.
pub fn why_lockstep(name: &str) -> Option<Lockstep> {
    let Some(profile) = official::profile(EncodingKind::Framing, name) else {
        return Some(Lockstep::Unknown);
    };
    match (profile.both_directions(), profile.names_protocol()) {
        (Some(true), Some(true)) => None,
        (Some(true), _) => Some(Lockstep::UnnamedServerProvided),
        (Some(false), _) => Some(Lockstep::OneDirection),
        // An official framing's profile states `directions`; one that
        // doesn't is no more known than a custom framing.
        (None, _) => Some(Lockstep::Unknown),
    }
}

/// A framing's name for a message: the catalog's title would be best, but core
/// doesn't re-export it, so it's the canonical name in backticks (`jsonrpc`,
/// `datagram@2`, `com.acme.lines@1`), any spelling.
pub fn framing_title(name: &str) -> String {
    format!("`{}`", crate::encodings::canonical(name))
}

/// The method a framing that names its calls calls `function` of `protocol` (declared in the
/// schema `namespace`) by: `send`, or `chat.Chat/send` when qualified.
pub fn method_name(namespace: &str, protocol: &str, function: &str, names: MethodNames) -> String {
    match names {
        MethodNames::Bare => function.to_string(),
        MethodNames::Qualified => {
            format!(
                "{}.{protocol}/{function}",
                namespace.replace("::", ".").replace('/', ".")
            )
        }
    }
}

/// Refuses protocols on one connection whose bare method names one side
/// couldn't tell apart: two protocols the same side provides, both with a
/// function of the same name, on a framing that names its calls.
pub fn check(
    schemas: &[(String, Vec<FrozenUnit>)],
    encodings: &BTreeMap<String, EncodingMapping>,
) -> Result<()> {
    let problems = bare_name_clashes(schemas, encodings);
    if !problems.is_empty() {
        bail!("{}", problems.join("\n"));
    }
    Ok(())
}

/// [`check`]'s problems, one sentence each.
pub fn bare_name_clashes(
    schemas: &[(String, Vec<FrozenUnit>)],
    encodings: &BTreeMap<String, EncodingMapping>,
) -> Vec<String> {
    let mut problems = Vec::new();
    // Each side looks a method up among the protocols it provides.
    for (side, provides) in [
        ("server", [Provider::Server, Provider::Multiple]),
        ("client", [Provider::Client, Provider::Multiple]),
    ] {
        let mut seen: Vec<(&str, String)> = Vec::new();
        for (namespace, name, parameters, functions) in protocols(schemas) {
            let framing = framing_name(parameters);
            let bare = crate::encodings::names_calls(encodings, &framing)
                && MethodNames::of_protocol(parameters) == MethodNames::Bare;
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
                         provides both on one `{framing}` connection, where bare method names can't tell them \
                         apart - use `@framing = {{ name = \"{framing}\", method_names = qualified }}` on either"
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
    /// Canonical: `comline@1`, `comline@2` or `none`.
    pub handshake: String,
    /// The handshake with its parameters, which every protocol on the
    /// connection shares.
    pub handshake_value: HandshakeValue,
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
pub fn package_connection(
    schemas: &[(String, Vec<FrozenUnit>)],
) -> Result<Option<PackageConnection>, String> {
    let on: Vec<_> = protocols(schemas)
        .filter(|(_, _, parameters, _)| on_connection(schemas, parameters))
        .collect();
    let Some((first_ns, first, first_parameters, _)) = on.first() else {
        return Ok(None);
    };
    let framing_name = self::framing_name(first_parameters);
    let handshake_name = self::handshake_name(first_parameters);
    let handshake_value = self::handshake_value(first_parameters);
    for (namespace, name, parameters, _) in &on[1..] {
        let (f, h) = (
            self::framing_name(parameters),
            self::handshake_name(parameters),
        );
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
        if self::handshake_value(parameters) != handshake_value {
            return Err(format!(
                "`{first_ns}::{first}` and `{namespace}::{name}` give the `{handshake_name}` handshake different \
                 parameters, so there's no one connection to serve them on"
            ));
        }
    }
    let side = |sides: [Provider; 2]| {
        let provided: Vec<_> = on
            .iter()
            .filter(|(_, _, parameters, _)| sides.contains(&Provider::of(parameters)))
            .collect();
        provided
            .iter()
            .map(|(namespace, name, _, _)| {
                let clash = provided
                    .iter()
                    .filter(|(_, other, _, _)| other == name)
                    .count()
                    > 1;
                let field = match clash {
                    true => format!(
                        "{}_{}",
                        namespace.replace("::", "_").replace('/', "_"),
                        crate::naming::snake(name)
                    ),
                    false => crate::naming::snake(name),
                };
                Provided {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                    field,
                }
            })
            .collect()
    };
    Ok(Some(PackageConnection {
        framing: framing_name,
        handshake: handshake_name,
        handshake_value,
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
            FrozenUnit::Protocol {
                name,
                parameters,
                functions,
                ..
            } => Some((
                ns.as_str(),
                name.as_str(),
                parameters.as_slice(),
                functions.as_slice(),
            )),
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
    let all: Vec<FrozenUnit> = schemas
        .iter()
        .flat_map(|(_, units)| units.iter().cloned())
        .collect();
    schema_ir_hash(&all)
}

fn property<'a>(parameters: &'a [FrozenUnit], key: &str) -> Option<&'a str> {
    parameters.iter().find_map(|p| match p {
        FrozenUnit::Property {
            name,
            value: ConfigValue::Str(s) | ConfigValue::Name(s),
        } if name == key => Some(s.as_str()),
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
        FrozenUnit::Property {
            name: name.to_string(),
            value,
        }
    }

    #[test]
    fn numbers_count_every_protocol_in_schema_order() {
        let schemas = vec![
            (
                "chat".to_string(),
                vec![protocol("Chat", vec![]), protocol("Admin", vec![])],
            ),
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
                (
                    "method_names".to_string(),
                    ConfigValue::Name("qualified".to_string()),
                ),
            ]),
        )
    }

    fn with_functions(name: &str, parameters: Vec<FrozenUnit>, functions: &[&str]) -> FrozenUnit {
        let FrozenUnit::Protocol {
            docstring, span, ..
        } = protocol(name, vec![])
        else {
            unreachable!()
        };
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
            (
                "game".to_string(),
                vec![
                    protocol("Account", vec![]),
                    protocol("Player", vec![provider("Multiple")]),
                ],
            ),
            (
                "game::admin".to_string(),
                vec![
                    protocol("Player", vec![]),
                    protocol("Events", vec![provider("Client")]),
                ],
            ),
            (
                "legacy".to_string(),
                vec![protocol("Old", vec![framing("datagram")])],
            ),
        ];
        let c = package_connection(&schemas).unwrap().unwrap();
        assert_eq!(
            (c.framing.as_str(), c.handshake.as_str()),
            ("datagram@2", "comline@1")
        );
        let fields = |side: &[Provided]| side.iter().map(|p| p.field.clone()).collect::<Vec<_>>();
        // Two `Player`s on the server side: both prefixed; the lone one on
        // the client side isn't.
        assert_eq!(
            fields(&c.server),
            ["account", "game_player", "game_admin_player"]
        );
        assert_eq!(fields(&c.client), ["player", "events"]);

        assert_eq!(
            package_connection(&[(
                "legacy".to_string(),
                vec![protocol("Old", vec![framing("datagram")])]
            )]),
            Ok(None)
        );
    }

    #[test]
    fn protocols_that_disagree_have_no_package_connection() {
        let none = property("handshake", ConfigValue::Str("none".to_string()));
        let schemas = vec![(
            "app".to_string(),
            vec![protocol("A", vec![]), protocol("B", vec![none])],
        )];
        let err = package_connection(&schemas).unwrap_err();
        assert!(
            err.contains("`app::A` runs the `comline@1` handshake and `app::B` the `none` one"),
            "{err}"
        );

        let schemas = vec![(
            "app".to_string(),
            vec![
                protocol("A", vec![]),
                protocol("B", vec![provider("Client"), framing("jsonrpc")]),
            ],
        )];
        let err = package_connection(&schemas).unwrap_err();
        assert!(
            err.contains("`datagram@2` connection and `app::B` on a `jsonrpc` one"),
            "{err}"
        );
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
        assert!(on_connection(
            &schemas(vec![provider("Client"), qualified()]),
            &chat
        ));
        // A datagram@2 connection doesn't take JSON-RPC protocols along.
        assert!(!on_connection(
            &schemas(vec![provider("Client"), framing("datagram@2")]),
            &chat
        ));
    }

    #[test]
    fn a_method_is_the_function_or_qualified_by_its_protocol() {
        assert_eq!(
            method_name("chat", "Chat", "send", MethodNames::Bare),
            "send"
        );
        assert_eq!(
            method_name("chat", "Chat", "send", MethodNames::Qualified),
            "chat.Chat/send"
        );
        assert_eq!(
            method_name("chat::admin", "Admin", "kick", MethodNames::Qualified),
            "chat.admin.Admin/kick"
        );
    }

    #[test]
    fn bare_names_clash_on_any_framing_that_names_its_calls() {
        let lines = "com.acme.lines@1";
        let on_lines = |name, functions: &[&str]| {
            with_functions(name, vec![provider("Client"), framing(lines)], functions)
        };
        let clash = vec![(
            "app".to_string(),
            vec![on_lines("Chat", &["send"]), on_lines("Admin", &["send"])],
        )];
        let facts = |calls: &str| {
            BTreeMap::from([(
                lines.to_string(),
                EncodingMapping {
                    calls: Some(calls.to_string()),
                    ..Default::default()
                },
            )])
        };
        let problems = bare_name_clashes(&clash, &facts("by_name"));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("one `com.acme.lines@1` connection"),
            "{problems:?}"
        );
        assert!(bare_name_clashes(&clash, &facts("by_index")).is_empty());
    }

    #[test]
    fn bare_names_one_side_could_confuse_are_refused() {
        let chat = |parameters| with_functions("Chat", parameters, &["send", "count"]);
        let admin = |parameters| with_functions("Admin", parameters, &["send"]);
        let events = with_functions(
            "Events",
            vec![provider("Client"), framing("jsonrpc")],
            &["send"],
        );

        // Both provided by the server, both bare: `send` is ambiguous.
        let clash = vec![(
            "app".to_string(),
            vec![
                chat(vec![framing("jsonrpc")]),
                admin(vec![framing("jsonrpc")]),
                events.clone(),
            ],
        )];
        let problems = bare_name_clashes(&clash, &Default::default());
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("`app::Chat` and `app::Admin` both have a function `send`"),
            "{problems:?}"
        );
        assert!(
            problems[0].contains("the server provides both"),
            "{problems:?}"
        );
        assert!(check(&clash, &Default::default()).is_err());

        // Qualifying either is enough; `Events` is the client's, so its
        // `send` never meets the server's.
        let fine = vec![(
            "app".to_string(),
            vec![
                chat(vec![framing("jsonrpc")]),
                admin(vec![qualified()]),
                events.clone(),
            ],
        )];
        assert!(check(&fine, &Default::default()).is_ok());

        // Off a connection there's one protocol per server, so nothing clashes.
        let lockstep = vec![(
            "app".to_string(),
            vec![
                chat(vec![framing("jsonrpc")]),
                admin(vec![framing("jsonrpc")]),
            ],
        )];
        assert!(check(&lockstep, &Default::default()).is_ok());
    }

    #[test]
    fn a_dictionary_handshake_is_read_by_its_name() {
        let dict = |entries: Vec<(&str, &str)>| {
            property(
                "handshake",
                ConfigValue::Dict(
                    entries
                        .into_iter()
                        .map(|(k, v)| (k.to_string(), ConfigValue::Str(v.to_string())))
                        .collect(),
                ),
            )
        };
        let params = vec![dict(vec![
            ("name", "comline@2"),
            ("newer_consumer", "refuse"),
        ])];
        assert_eq!(handshake_name(&params), "comline@2");
        assert_eq!(
            handshake_value(&params).param("newer_consumer"),
            Some("refuse")
        );
        assert_eq!(
            handshake_name(&[dict(vec![("name", "comline@2")])]),
            "comline@2"
        );
        assert_eq!(
            handshake_name(&[property("handshake", ConfigValue::Str("comline@2".into()))]),
            "comline@2"
        );
        assert_eq!(handshake_name(&[]), "comline@1");

        // One connection, one set of parameters.
        let schemas = vec![(
            "app".to_string(),
            vec![
                protocol(
                    "A",
                    vec![framing("datagram@3"), dict(vec![("name", "comline@2")])],
                ),
                protocol(
                    "B",
                    vec![
                        framing("datagram@3"),
                        dict(vec![("name", "comline@2"), ("scope", "package")]),
                    ],
                ),
            ],
        )];
        let err = package_connection(&schemas).unwrap_err();
        assert!(err.contains("different parameters"), "{err}");
    }

    #[test]
    fn datagram3_puts_a_protocol_on_a_connection() {
        let parameters = vec![framing("datagram@3")];
        let schemas = vec![("app".to_string(), vec![protocol("P", parameters.clone())])];
        assert!(on_connection(&schemas, &parameters));
    }

    /// What each official framing's facts decide, one protocol on it alone:
    /// `directions` and `protocol` read from core's profiles.
    #[test]
    fn the_official_framings_are_judged_by_their_facts() {
        let alone = |framing_name: &str, provider_name: Option<&str>| {
            let mut parameters = vec![framing(framing_name)];
            parameters.extend(provider_name.map(provider));
            let schemas = vec![("app".to_string(), vec![protocol("P", parameters.clone())])];
            on_connection(&schemas, &parameters)
        };
        // Both ways and named: a connection by default.
        assert!(alone("datagram@2", None));
        assert!(alone("datagram@3", None));
        // One way: only a client-provided protocol (which core would refuse).
        assert!(!alone("datagram@1", None));
        assert!(!alone("datagram", None));
        assert!(alone("datagram@1", Some("Client")));
        // Both ways, unnamed: only when some protocol on it is client-provided.
        assert!(!alone("jsonrpc@1", None));
        assert!(!alone("json-rpc", None));
        assert!(alone("jsonrpc@1", Some("Client")));
        assert!(alone("jsonrpc", Some("Multiple")));
    }

    /// Frames that go both ways without naming the protocol look the same for
    /// every protocol, so one client-provided protocol takes the rest of its
    /// framing along, and no other framing's.
    #[test]
    fn an_unnamed_framing_shares_its_connection_by_facts() {
        let on = |name: &str, others: Vec<FrozenUnit>| {
            let schemas = vec![
                ("a".to_string(), vec![protocol("A", vec![framing(name)])]),
                ("b".to_string(), vec![protocol("B", others)]),
            ];
            on_connection(&schemas, &[framing(name)])
        };
        assert!(on(
            "jsonrpc@1",
            vec![framing("json-rpc"), provider("Client")]
        ));
        assert!(!on("jsonrpc@1", vec![framing("json-rpc")]));
        // One way, so nothing is shared.
        assert!(!on(
            "datagram@1",
            vec![framing("datagram"), provider("Client")]
        ));
        // Another framing's client-provided protocol doesn't spread here.
        assert!(!on(
            "jsonrpc@1",
            vec![framing("datagram@3"), provider("Client")]
        ));
    }

    /// A framing of a package's own states no facts here, so it behaves as it
    /// did before they were read: its protocol is on a connection when it's
    /// client-provided, and nothing spreads to another protocol on it.
    #[test]
    fn a_custom_framing_is_on_a_connection_only_for_a_client_provider() {
        let lines = "com.acme.lines@1";
        let alone = |parameters: Vec<FrozenUnit>| {
            let schemas = vec![("app".to_string(), vec![protocol("P", parameters.clone())])];
            on_connection(&schemas, &parameters)
        };
        assert!(!alone(vec![framing(lines)]));
        assert!(!alone(vec![framing(lines), provider("Server")]));
        assert!(alone(vec![framing(lines), provider("Client")]));
        assert!(alone(vec![framing(lines), provider("Multiple")]));

        // Two on one custom framing, one client-provided: only that one.
        let served = vec![framing(lines)];
        let dialled = vec![framing(lines), provider("Client")];
        let schemas = vec![(
            "app".to_string(),
            vec![
                protocol("Served", served.clone()),
                protocol("Dialled", dialled.clone()),
            ],
        )];
        assert!(!on_connection(&schemas, &served));
        assert!(on_connection(&schemas, &dialled));
        let c = package_connection(&schemas).unwrap().unwrap();
        let fields = |side: &[Provided]| side.iter().map(|p| p.field.clone()).collect::<Vec<_>>();
        // `Served` stays the lockstep server's: not on the connection.
        assert_eq!(fields(&c.server), Vec::<String>::new());
        assert_eq!(fields(&c.client), ["dialled"]);
        assert_eq!(c.framing, lines);
    }

    #[test]
    fn why_a_framing_is_lockstep_is_read_from_its_facts() {
        assert_eq!(why_lockstep("datagram@1"), Some(Lockstep::OneDirection));
        assert_eq!(why_lockstep("datagram"), Some(Lockstep::OneDirection));
        assert_eq!(
            why_lockstep("jsonrpc@1"),
            Some(Lockstep::UnnamedServerProvided)
        );
        assert_eq!(
            why_lockstep("json-rpc"),
            Some(Lockstep::UnnamedServerProvided)
        );
        assert_eq!(why_lockstep("datagram@2"), None);
        assert_eq!(why_lockstep("datagram@3"), None);
        assert_eq!(why_lockstep("com.acme.lines@1"), Some(Lockstep::Unknown));
    }

    #[test]
    fn a_framing_is_named_by_its_canonical_name_in_backticks() {
        assert_eq!(framing_title("jsonrpc@1"), "`jsonrpc`");
        assert_eq!(framing_title("datagram@3"), "`datagram@3`");
        assert_eq!(framing_title("com.acme.lines@1"), "`com.acme.lines@1`");
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
