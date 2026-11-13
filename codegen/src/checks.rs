//! The checks a generation run makes beyond generating, as structured
//! diagnostics: what a person who asked for something would otherwise take
//! for a bug in the generator when the output doesn't show it.
//!
//! - [`ASYNC_UNUSED`]: `async = true`, but no protocol is on a connection.
//! - [`NO_PACKAGE_CONNECTION`]: the protocols on connections disagree on a
//!   framing or handshake, so the generators leave out the package's `serve`
//!   and `connect`.
//! - [`NO_CONNECTION_PROTOCOLS`]: the quiet case of the same, a note: no
//!   protocol is on a connection, so there's nothing to serve.
//! - [`IGNORED_FEATURE`]: the generator generates without a feature the
//!   package uses.
//! - [`METHOD_NAME_CLASH`]: bare JSON-RPC method names that one side
//!   couldn't tell apart; no generator can emit them.
//!
//! They're core's [`Diagnostic`]s, so the CLI, the language server and the
//! Observatory show the same ones. A message is a sentence for whoever reads
//! it with the target named next to it: the CLI puts the language in front
//! (`rust`: ...), the language server needn't. The frozen IR the checks read
//! keeps no spans, so a diagnostic about one place names its unit by path in
//! [`Diagnostic::unit`] (`app::Chat`, `app::Chat::send`), which a tool with
//! the source resolves to a span; one about the whole package has none.
//!
//! The codes are declared here, as core declares its own. A code's severity
//! is its own, never a call site's choice.

use std::collections::{BTreeMap, BTreeSet};

use comline_core::diagnostics::{Diagnostic, DiagnosticCode};
use comline_core::schema::ir::frozen::unit::FrozenUnit;

use crate::connections;
use crate::features::{self, State};
use crate::protocol::{EncodingMapping, GenerateRequest};
use crate::Resolved;

/// `async = true` on a package where no protocol runs on a connection, so the
/// output is the same as without it. Package-level: no unit.
pub const ASYNC_UNUSED: DiagnosticCode = DiagnosticCode::warning("generation::async-unused");

/// The package's protocols on connections disagree on the framing or the
/// handshake, so there's no one connection to serve them on: no package
/// `serve` / `connect`, though each protocol still has its own. Its unit is
/// the first protocol that disagrees with the first on a connection.
pub const NO_PACKAGE_CONNECTION: DiagnosticCode =
    DiagnosticCode::warning("generation::no-package-connection");

/// Protocols are declared, but none is on a connection, so there's no package
/// `Services` / `serve` / `connect` to look for. Not a problem, a note, and a
/// code of its own, since a code's severity is intrinsic. Package-level: no
/// unit.
pub const NO_CONNECTION_PROTOCOLS: DiagnosticCode =
    DiagnosticCode::info("generation::no-connection-protocols");

/// The generator ignores a feature the package uses: it generates, but
/// without the feature. Its unit is the first use of it.
pub const IGNORED_FEATURE: DiagnosticCode = DiagnosticCode::warning("generation::ignored-feature");

/// Two protocols one side provides on one connection have a function of the
/// same name, where JSON-RPC's bare method names can't tell them apart. Its
/// unit is the second of the two protocols.
pub const METHOD_NAME_CLASH: DiagnosticCode =
    DiagnosticCode::error("generation::method-name-clash");

/// What generating `request` for one target warns about, in the order a
/// person reads it: the features the generator ignores, an `async` setting
/// that does nothing, then what there is to know about the package's
/// connection.
///
/// `resolved` is the target's generator, for what it ignores
/// ([`IGNORED_FEATURE`]); `None` for a generator that isn't built in, whose
/// support table isn't known here, which skips that check. `connections_doc`
/// is the generator's own page on connections (`Description::docs`, topic
/// `connections`), which [`NO_CONNECTION_PROTOCOLS`] links to when there is
/// one.
///
/// A feature the generator doesn't support at all isn't here: it refuses the
/// request, naming every use.
pub fn generation(
    request: &GenerateRequest,
    resolved: Option<&Resolved>,
    connections_doc: Option<&str>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if let Some(resolved) = resolved {
        diagnostics.extend(ignored_features(request, resolved));
    }
    diagnostics.extend(async_unused(request));
    diagnostics.extend(package_connection(request, connections_doc));
    diagnostics
}

/// The [`METHOD_NAME_CLASH`] errors for `schemas`: protocols on one
/// connection whose bare method names a side couldn't tell apart. `framings`
/// is what each framing the package can see states, by canonical name, as the
/// generation request's `encodings` carries them. One per function and side,
/// as [`connections::bare_name_clashes`] counts them, with the same wording.
pub fn method_name_clashes(
    schemas: &[(String, Vec<FrozenUnit>)],
    framings: &BTreeMap<String, EncodingMapping>,
) -> Vec<Diagnostic> {
    connections::name_clashes(schemas, framings)
        .into_iter()
        .map(|clash| Diagnostic::new(METHOD_NAME_CLASH, clash.message()).with_unit(clash.second))
        .collect()
}

/// `async = true` changes only protocols on a connection; with none, the
/// output is the same as without it, which looks like the setting did nothing.
/// A protocol is on one by default when its framing's frames go both ways and
/// name the protocol, so that's a package whose protocols are all lockstep:
/// each on a framing that keeps it off a connection
/// ([`connections::why_lockstep`]), which the message names.
fn async_unused(request: &GenerateRequest) -> Option<Diagnostic> {
    if !request.async_api {
        return None;
    }

    let uses = features::used(&request.schemas, &features::Options::of(request));
    if uses.iter().any(|u| u.feature == features::Feature::Async) {
        return None;
    }

    let lockstep: Vec<String> = framings_in_use(request)
        .iter()
        .filter_map(|framing| lockstep_description(framing))
        .collect();

    let why = if lockstep.is_empty() {
        "the package has no protocols".to_owned()
    } else {
        format!(
            "every protocol here is lockstep: on {}",
            lockstep.join(" or ")
        )
    };

    Some(Diagnostic::new(
        ASYNC_UNUSED,
        format!(
            "`async = true`, but no protocol runs on a connection, so the output \
             is the same as without it - {why}"
        ),
    ))
}

/// The framings the package's protocols are on, canonical (`jsonrpc`,
/// `datagram@2`, `com.acme.lines@1`), once each.
fn framings_in_use(request: &GenerateRequest) -> BTreeSet<String> {
    let mut framings = BTreeSet::new();
    for (_, units) in &request.schemas {
        for unit in units {
            if let FrozenUnit::Protocol { parameters, .. } = unit {
                framings.insert(connections::framing_name(parameters));
            }
        }
    }

    framings
}

/// A framing for a message about protocols that aren't on a connection: its
/// title, and why its protocols stay off one unless the client provides
/// them. `None` for a framing that has no such reason (its frames go both
/// ways and name the protocol), where a protocol is on a connection already.
fn lockstep_description(framing: &str) -> Option<String> {
    let reason = match connections::why_lockstep(framing)? {
        connections::Lockstep::OneDirection => {
            "its frames go one way, so the server can't call the client"
        }
        connections::Lockstep::UnnamedServerProvided => {
            "its frames don't name the protocol, and none of its protocols is client-provided"
        }
        connections::Lockstep::Unknown => {
            "a framing of the package's own: a protocol on it is on a connection only when \
             the client provides it"
        }
    };

    Some(format!(
        "{} ({reason})",
        connections::framing_title(framing)
    ))
}

/// The package's `serve` / `connect` need its protocols on one connection;
/// when they disagree on the framing or handshake, the generators leave them
/// out, which would otherwise look like they forgot.
fn package_connection(
    request: &GenerateRequest,
    connections_doc: Option<&str>,
) -> Option<Diagnostic> {
    match connections::package_connection_of(&request.schemas) {
        Err(disagreement) => Some(
            Diagnostic::new(
                NO_PACKAGE_CONNECTION,
                format!(
                    "no package `serve` / `connect` - {}; each protocol still has its own",
                    disagreement.reason
                ),
            )
            .with_unit(disagreement.protocol),
        ),
        Ok(None) => no_connection_protocols(request, connections_doc),
        Ok(Some(_)) => None,
    }
}

/// The quiet case: protocols are declared, but none is on a connection, so
/// there is no `Services` / `serve` to look for. A package with no protocols
/// has nothing to serve and gets no note. `connections_doc` is the generator's
/// own page on connections, which the note links to when it has one.
fn no_connection_protocols(
    request: &GenerateRequest,
    connections_doc: Option<&str>,
) -> Option<Diagnostic> {
    let framings = framings_in_use(request);

    if framings.is_empty() {
        return None;
    }

    // Each framing's title and why it keeps its protocols off a connection;
    // one with no such reason (not expected here) is just its title.
    let framings: Vec<String> = framings
        .iter()
        .map(|f| lockstep_description(f).unwrap_or_else(|| connections::framing_title(f)))
        .collect();

    let page = connections_doc
        .map(|url| format!(" ({url})"))
        .unwrap_or_default();

    Some(Diagnostic::new(
        NO_CONNECTION_PROTOCOLS,
        format!(
            "no package `Services` / `serve` / `connect` - every protocol is \
             server-provided on {}, and none is on a connection{page}",
            framings.join(" or ")
        ),
    ))
}

/// A diagnostic per feature `request` uses that this generator ignores - it
/// generates, but without the feature (a TypeScript target with
/// `@validators` gets the types, not the checks). The message continues
/// "ignores": what it says, with the generator's note and where it's tracked.
fn ignored_features(request: &GenerateRequest, resolved: &Resolved) -> Vec<Diagnostic> {
    let uses = features::used(&request.schemas, &features::Options::of(request));
    let mut ignored: BTreeMap<features::Feature, Vec<&features::FeatureUse>> = BTreeMap::new();
    for u in &uses {
        if resolved.support(u.feature).state == State::Ignored {
            ignored.entry(u.feature).or_default().push(u);
        }
    }

    ignored
        .into_iter()
        .map(|(feature, uses)| {
            let s = resolved.support(feature);
            let mut message = feature.title().to_owned();

            if let Some(note) = s.note {
                message.push_str(&format!(": {note}"));
            }

            if let Some(tracking) = s.tracking {
                message.push_str(&format!(" ({tracking})"));
            }

            let first = uses[0];
            let place = match first.namespace.is_empty() {
                true => first.owner.clone(),
                false => format!("schema '{}', {}", first.namespace, first.owner),
            };

            message.push_str(&format!(" - used by {place}"));
            if uses.len() > 1 {
                message.push_str(&format!(" and {} more", uses.len() - 1));
            }

            let diagnostic = Diagnostic::new(IGNORED_FEATURE, message);
            match unit_of(first) {
                Some(unit) => diagnostic.with_unit(unit),
                None => diagnostic,
            }
        })
        .collect()
}

/// The unit a [`features::FeatureUse`] is in, by path: the schema's namespace,
/// then the declaration's name, then the member's (`app::User::tags`,
/// `app::Chat::send`), read off the quoted names in its `owner` phrase
/// (`struct 'User', field 'tags'`, `protocol 'Chat', function 'send',
/// argument 'to'`). A use with no declaration, such as an import or a nested
/// schema, is the schema itself. A request option isn't in a unit at all.
///
/// A name past the member, an argument or a validator, isn't part of the path:
/// the path ends where the units a tool can find do.
fn unit_of(feature_use: &features::FeatureUse) -> Option<String> {
    if feature_use.namespace.is_empty() {
        return None;
    }

    let owner = feature_use.owner.as_str();
    let names: Vec<&str> = match owner.starts_with("schema '") {
        true => Vec::new(),
        false => owner.split('\'').skip(1).step_by(2).take(2).collect(),
    };

    let mut unit = feature_use.namespace.clone();
    for name in names {
        unit.push_str("::");
        unit.push_str(name);
    }

    Some(unit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::{Feature, Support};
    use crate::protocol::TargetLanguage;
    use crate::validation::ValidateOnDecode;
    use crate::versions::{LangVersion, Versions};
    use crate::{Generator, Mode, PackageMeta};
    use comline_core::diagnostics::Severity;
    use comline_core::schema::ir::frozen::unit::ConfigValue;

    fn property(name: &str, value: ConfigValue) -> FrozenUnit {
        FrozenUnit::Property {
            name: name.to_string(),
            value,
        }
    }

    fn framing(f: &str) -> FrozenUnit {
        property("framing", ConfigValue::Str(f.to_string()))
    }

    fn provider(p: &str) -> FrozenUnit {
        property("provider", ConfigValue::Name(p.to_string()))
    }

    fn handshake(h: &str) -> FrozenUnit {
        property("handshake", ConfigValue::Str(h.to_string()))
    }

    fn protocol(name: &str, parameters: Vec<FrozenUnit>, functions: &[&str]) -> FrozenUnit {
        FrozenUnit::Protocol {
            docstring: String::new(),
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
            span: (0, 0),
        }
    }

    fn request(schemas: Vec<(String, Vec<FrozenUnit>)>) -> GenerateRequest {
        GenerateRequest {
            mode: Mode::Code,
            schemas,
            package: PackageMeta {
                name: "demo".into(),
                version: "1.0.0".into(),
            },
            target: TargetLanguage {
                language: "toy".into(),
                version: "1.0.0".into(),
            },
            wire_format: "packed@1".into(),
            external_std: false,
            validate_on_decode: ValidateOnDecode::default(),
            async_api: false,
            encodings: BTreeMap::new(),
            facts: None,
        }
    }

    fn app(units: Vec<FrozenUnit>) -> Vec<(String, Vec<FrozenUnit>)> {
        vec![("app".to_string(), units)]
    }

    /// A generator of a made-up language that ignores `ignored`.
    fn resolved(table: features::SupportTable) -> Resolved {
        fn generate(_: &GenerateRequest) -> eyre::Result<Vec<crate::GeneratedFile>> {
            Ok(vec![])
        }
        Resolved {
            generator: Generator {
                language: "toy",
                ext: "toy",
                generate,
                support: table,
                versions: Versions {
                    floor: "1.0.0",
                    changes: &[],
                    verified_up_to: "1.0.0",
                    forward_compatible: true,
                },
                docs: &[],
            },
            version: LangVersion::parse("1.0.0").unwrap(),
        }
    }

    const PROTOCOLS_IGNORED: features::SupportTable = &[(
        Feature::Protocols,
        Support::ignored("they're generated as plain interfaces").tracked("core#1"),
    )];

    const LIB_MODE_IGNORED: features::SupportTable =
        &[(Feature::LibMode, Support::ignored("it makes no package"))];

    #[test]
    fn the_codes_have_the_severities_they_mean() {
        assert_eq!(ASYNC_UNUSED.id, "generation::async-unused");
        assert_eq!(ASYNC_UNUSED.severity, Severity::Warning);
        assert_eq!(
            NO_PACKAGE_CONNECTION.id,
            "generation::no-package-connection"
        );
        assert_eq!(NO_PACKAGE_CONNECTION.severity, Severity::Warning);
        assert_eq!(
            NO_CONNECTION_PROTOCOLS.id,
            "generation::no-connection-protocols"
        );
        assert_eq!(NO_CONNECTION_PROTOCOLS.severity, Severity::Info);
        assert_eq!(IGNORED_FEATURE.id, "generation::ignored-feature");
        assert_eq!(IGNORED_FEATURE.severity, Severity::Warning);
        assert_eq!(METHOD_NAME_CLASH.id, "generation::method-name-clash");
        assert_eq!(METHOD_NAME_CLASH.severity, Severity::Error);
    }

    #[test]
    fn async_with_every_protocol_lockstep_is_a_package_level_warning() {
        let mut request = request(app(vec![protocol("Chat", vec![framing("jsonrpc")], &[])]));
        request.async_api = true;

        let found = generation(&request, None, None);
        let async_unused: Vec<_> = found.iter().filter(|d| d.code == ASYNC_UNUSED).collect();
        assert_eq!(async_unused.len(), 1, "{found:?}");
        let d = async_unused[0];
        assert_eq!(d.code.severity, Severity::Warning);
        assert_eq!(d.unit, None);
        assert_eq!(d.help, None);
        assert_eq!(
            d.message,
            "`async = true`, but no protocol runs on a connection, so the output is the same \
             as without it - every protocol here is lockstep: on JSON-RPC (its frames don't \
             name the protocol, and none of its protocols is client-provided)"
        );

        // Off, or in use, it's silent.
        request.async_api = false;
        assert!(generation(&request, None, None)
            .iter()
            .all(|d| d.code != ASYNC_UNUSED));
        let mut on_one = self::request(app(vec![protocol("Chat", vec![], &[])]));
        on_one.async_api = true;
        assert!(generation(&on_one, None, None)
            .iter()
            .all(|d| d.code != ASYNC_UNUSED));
    }

    #[test]
    fn async_with_no_protocols_says_so() {
        let mut request = request(app(vec![]));
        request.async_api = true;
        let found = generation(&request, None, None);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            found[0].message,
            "`async = true`, but no protocol runs on a connection, so the output is the same \
             as without it - the package has no protocols"
        );
    }

    #[test]
    fn protocols_that_disagree_have_no_package_connection_naming_the_one_that_does() {
        let request = request(app(vec![
            protocol("A", vec![], &[]),
            protocol("B", vec![handshake("none")], &[]),
        ]));
        let found = generation(&request, None, None);
        assert_eq!(found.len(), 1, "{found:?}");
        let d = &found[0];
        assert_eq!(d.code, NO_PACKAGE_CONNECTION);
        assert_eq!(d.code.severity, Severity::Warning);
        assert_eq!(d.unit.as_deref(), Some("app::B"));
        assert_eq!(
            d.message,
            "no package `serve` / `connect` - `app::A` runs the `comline@1` handshake and \
             `app::B` the `none` one, so there's no one connection to serve them on; each \
             protocol still has its own"
        );

        // On the framing, too.
        let request = self::request(app(vec![
            protocol("A", vec![], &[]),
            protocol("B", vec![provider("Client"), framing("jsonrpc")], &[]),
        ]));
        let found = generation(&request, None, None);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].unit.as_deref(), Some("app::B"));
        assert!(
            found[0]
                .message
                .contains("`datagram@2` connection and `app::B` on a `jsonrpc` one"),
            "{found:?}"
        );
    }

    #[test]
    fn no_protocol_on_a_connection_is_a_note_with_the_generators_page() {
        let request = request(app(vec![protocol("Chat", vec![framing("jsonrpc")], &[])]));
        let expected = "no package `Services` / `serve` / `connect` - every protocol is \
                        server-provided on JSON-RPC (its frames don't name the protocol, and none \
                        of its protocols is client-provided), and none is on a connection";

        let found = generation(&request, None, None);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].code, NO_CONNECTION_PROTOCOLS);
        assert_eq!(found[0].code.severity, Severity::Info);
        assert_eq!(found[0].unit, None);
        assert_eq!(found[0].help, None);
        assert_eq!(found[0].message, expected);

        let found = generation(&request, None, Some("https://example.com/connections"));
        assert_eq!(
            found[0].message,
            format!("{expected} (https://example.com/connections)")
        );

        // Nothing declared, nothing to note.
        assert!(generation(&self::request(app(vec![])), None, None).is_empty());
        // A package that has a connection has nothing to note either.
        assert!(generation(
            &self::request(app(vec![protocol("Chat", vec![], &[])])),
            None,
            None
        )
        .is_empty());
    }

    #[test]
    fn a_framing_of_the_packages_own_is_named_as_such() {
        let request = request(app(vec![protocol(
            "Chat",
            vec![framing("com.acme.lines@1")],
            &[],
        )]));
        let found = generation(&request, None, None);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].message.contains(
                "server-provided on `com.acme.lines@1` (a framing of the package's own: a \
                 protocol on it is on a connection only when the client provides it)"
            ),
            "{found:?}"
        );
    }

    #[test]
    fn an_ignored_feature_names_its_first_use_and_how_many_more() {
        let resolved = resolved(PROTOCOLS_IGNORED);
        let request = request(vec![
            (
                "app".to_string(),
                vec![
                    protocol("Chat", vec![], &[]),
                    protocol("Admin", vec![], &[]),
                ],
            ),
            (
                "app::more".to_string(),
                vec![protocol("Other", vec![], &[])],
            ),
        ]);

        let found = generation(&request, Some(&resolved), None);
        assert_eq!(found.len(), 1, "{found:?}");
        let d = &found[0];
        assert_eq!(d.code, IGNORED_FEATURE);
        assert_eq!(d.code.severity, Severity::Warning);
        assert_eq!(d.unit.as_deref(), Some("app::Chat"));
        assert_eq!(
            d.message,
            format!(
                "{}: they're generated as plain interfaces (core#1) - used by schema 'app', \
                 protocol 'Chat' and 2 more",
                Feature::Protocols.title()
            )
        );

        // Without the generator, there's nothing to say about what it ignores.
        assert!(generation(&request, None, None)
            .iter()
            .all(|d| d.code != IGNORED_FEATURE));
    }

    #[test]
    fn an_ignored_request_option_is_in_no_unit() {
        let resolved = resolved(LIB_MODE_IGNORED);
        let mut request = request(app(vec![]));
        request.mode = Mode::Lib;

        let found = generation(&request, Some(&resolved), None);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].unit, None);
        assert_eq!(
            found[0].message,
            format!(
                "{}: it makes no package - used by the generation request (`lib` mode)",
                Feature::LibMode.title()
            )
        );
    }

    #[test]
    fn a_feature_use_is_in_the_unit_its_owner_phrase_names() {
        let in_schema = |namespace: &str, owner: &str| features::FeatureUse {
            feature: Feature::Structs,
            namespace: namespace.to_string(),
            owner: owner.to_string(),
            span: None,
        };
        let unit = |namespace: &str, owner: &str| unit_of(&in_schema(namespace, owner));

        assert_eq!(unit("app", "struct 'User'").as_deref(), Some("app::User"));
        assert_eq!(
            unit("app", "struct 'User', field 'tags'").as_deref(),
            Some("app::User::tags")
        );
        assert_eq!(
            unit("app", "struct 'User', field 'tags', validator 'len'").as_deref(),
            Some("app::User::tags")
        );
        assert_eq!(
            unit("app", "protocol 'Chat', function 'send', argument 'to'").as_deref(),
            Some("app::Chat::send")
        );
        assert_eq!(unit("app", "use other::thing").as_deref(), Some("app"));
        assert_eq!(unit("a/b", "schema 'a/b'").as_deref(), Some("a/b"));
        assert_eq!(unit("", "the generation request (`lib` mode)"), None);
    }

    #[test]
    fn bare_method_names_that_clash_are_errors_naming_the_second_protocol() {
        let schemas = app(vec![
            protocol("Chat", vec![framing("jsonrpc")], &["send", "count"]),
            protocol("Admin", vec![framing("jsonrpc")], &["send"]),
            protocol(
                "Events",
                vec![provider("Client"), framing("jsonrpc")],
                &["send"],
            ),
        ]);

        let found = method_name_clashes(&schemas, &BTreeMap::new());
        assert_eq!(found.len(), 1, "{found:?}");
        let d = &found[0];
        assert_eq!(d.code, METHOD_NAME_CLASH);
        assert_eq!(d.code.severity, Severity::Error);
        assert_eq!(d.unit.as_deref(), Some("app::Admin"));
        assert_eq!(
            d.message,
            "protocols `app::Chat` and `app::Admin` both have a function `send`, and the server \
             provides both on one `jsonrpc` connection, where bare method names can't tell them \
             apart - use `@framing = { name = \"jsonrpc\", method_names = qualified }` on either"
        );

        // The messages are the strings the old check gave.
        assert_eq!(
            connections::bare_name_clashes(&schemas, &BTreeMap::new()),
            [d.message.clone()]
        );

        // Off a connection there's nothing to confuse.
        let lockstep = app(vec![
            protocol("Chat", vec![framing("jsonrpc")], &["send"]),
            protocol("Admin", vec![framing("jsonrpc")], &["send"]),
        ]);
        assert!(method_name_clashes(&lockstep, &BTreeMap::new()).is_empty());
    }
}
