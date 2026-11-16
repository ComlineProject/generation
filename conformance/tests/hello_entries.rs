//! A Hello is built from per-protocol entries, so a side can list only the
//! protocols it provides and calls (generation#153): what
//! `facts::hello_parts` gives every generator, checked against the one
//! package-wide Hello they all embed today (`facts::hello`).
//!
//! What every generator shares is checked here, on the request each gets. The
//! goldens hold each generator's own output for the fixtures.

use comline_codegen::connections::Provider;
use comline_codegen::validation::ValidateOnDecode;
use comline_codegen::{connections, facts, GenerateRequest, PackageMeta, TargetLanguage};
use comline_conformance::{corpus, facts_for, Fixture};
use comline_core::schema::idl::annotations::DEFAULT_FORMAT;
use comline_core::schema::ir::frozen::unit::FrozenUnit;

/// The request a generator for `language` gets for `fx`, as the harness
/// builds it: with the facts of the package's own schemas when they're needed.
fn request(fx: &Fixture, language: &str) -> GenerateRequest {
    let wire_format = fx.wire_format.unwrap_or(DEFAULT_FORMAT);
    let dependencies: Vec<String> = fx.dependencies.iter().map(|d| d.to_string()).collect();
    let mut req = GenerateRequest {
        mode: fx.mode,
        schemas: fx.schemas.clone(),
        package: PackageMeta { name: "conformance".into(), version: "0.0.0".into() },
        target: TargetLanguage { language: language.into(), version: "1.85.0".into() },
        wire_format: wire_format.into(),
        external_std: fx.external_std,
        validate_on_decode: ValidateOnDecode::Off,
        async_api: fx.async_api,
        encodings: comline_codegen::encodings::official_for(language),
        facts: None,
        dependencies,
    };
    if facts::needed_in(&req) {
        let own: Vec<_> = fx
            .schemas
            .iter()
            .filter(|(namespace, _)| !connections::is_dependency(&req.dependencies, namespace))
            .cloned()
            .collect();
        req.facts = Some(facts_for(&own, wire_format));
    }
    req
}

/// Every fixture whose package connection runs `comline@2`, for each
/// language: its request and connection.
fn hellos() -> Vec<(String, GenerateRequest, connections::PackageConnection)> {
    let mut out = vec![];
    for fx in corpus() {
        for language in ["rust", "typescript"] {
            let req = request(&fx, language);
            // A package whose protocols disagree on a connection has no Hello
            // to build, and no generator emits one.
            if let Ok(Some(connection)) = facts::comline2_connection(&req) {
                out.push((format!("{} ({language})", fx.name), req, connection));
            }
        }
    }
    out
}

#[test]
fn every_protocol_assembled_is_the_package_wide_hello_for_every_fixture() {
    let hellos = hellos();
    // Not vacuous: the corpus has several packages with a Hello.
    let names: Vec<&str> = hellos.iter().map(|(name, _, _)| name.as_str()).collect();
    for expected in [
        "handshake_comline2 (rust)",
        "connections_default (typescript)",
        "nested_protocol (rust)",
        "dependency_and_own_protocols (typescript)",
        "connections_subset (rust)",
        "connections_subset (typescript)",
    ] {
        assert!(names.contains(&expected), "{expected} has no Hello: {names:?}");
    }
    for (name, req, connection) in &hellos {
        let (bytes, protocols) = facts::hello(req, connection).unwrap();
        let parts = facts::hello_parts(req, connection).unwrap();
        assert_eq!(parts.assemble_all(), bytes, "{name}");
        assert_eq!(parts.assemble(parts.protocols.iter().rev()), bytes, "{name}, given in reverse");
        // The same protocols, with what a generator writes beside the Hello.
        let entries: Vec<_> = parts
            .protocols
            .iter()
            .map(|p| (p.namespace.as_str(), p.name.as_str(), p.number, p.line.as_str(), p.functions.clone()))
            .collect();
        let listed: Vec<_> = protocols
            .iter()
            .map(|p| (p.namespace.as_str(), p.name.as_str(), p.number, p.line.as_str(), p.functions.clone()))
            .collect();
        assert_eq!(entries, listed, "{name}");
        // Canonical: ascending by number, each protocol's function numbers too.
        assert!(parts.protocols.windows(2).all(|w| w[0].number < w[1].number), "{name}");
        assert!(parts.encodings.windows(2).all(|w| w[0] < w[1]), "{name}");
    }
}

/// The corpus's `connections_subset`, with its connection.
fn subset() -> (GenerateRequest, connections::PackageConnection) {
    let fx = corpus().into_iter().find(|fx| fx.name == "connections_subset").unwrap();
    let req = request(&fx, "rust");
    let connection = facts::comline2_connection(&req).unwrap().unwrap();
    (req, connection)
}

/// `req` with only the protocols named `keep` left in, and in its facts: the
/// package a side that provides and calls just those would be, whose
/// package-wide Hello is what a subset's Hello must be. The protocols keep
/// their wire numbers, which the package's history gave them.
fn only(req: &GenerateRequest, keep: &[&str]) -> GenerateRequest {
    let mut reduced = req.clone();
    for (_, units) in &mut reduced.schemas {
        units.retain(|u| !matches!(u, FrozenUnit::Protocol { name, .. } if !keep.contains(&name.as_str())));
    }
    reduced.facts.as_mut().unwrap().protocols.retain(|p| keep.contains(&p.name.as_str()));
    reduced
}

#[test]
fn the_subset_fixture_has_two_applications_with_a_protocol_per_side() {
    let (req, connection) = subset();
    let parts = facts::hello_parts(&req, &connection).unwrap();
    let entries: Vec<_> = parts
        .protocols
        .iter()
        .map(|p| (p.namespace.as_str(), p.name.as_str(), p.number, p.provider))
        .collect();
    assert_eq!(
        entries,
        [
            ("conformance", "Chat", 0, Provider::Server),
            ("conformance", "Events", 1, Provider::Client),
            ("conformance/admin", "Admin", 2, Provider::Server),
            ("conformance/admin", "Alerts", 3, Provider::Client),
        ]
    );
    // The facts number functions backwards from their positions; the Hello
    // lists their numbers ascending, and a generator keeps the names.
    let chat = &parts.protocols[0];
    assert_eq!(chat.functions, [("send".to_string(), 1), ("leave".to_string(), 0)]);
    assert_eq!(chat.function_numbers(), [0, 1]);
}

#[test]
fn any_subset_assembled_is_the_hello_of_a_package_of_just_those_protocols() {
    let (req, connection) = subset();
    let parts = facts::hello_parts(&req, &connection).unwrap();
    let names: Vec<&str> = parts.protocols.iter().map(|p| p.name.as_str()).collect();
    // The encodings are the connection's, so they're the same bytes in every
    // Hello, up to the protocols: a Hello that lists none is those and a zero
    // count.
    let mut header = parts.assemble(std::iter::empty());
    assert_eq!(header.pop(), Some(0));

    for mask in 1u32..(1 << names.len()) {
        let chosen: Vec<&str> =
            names.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, n)| *n).collect();
        let reduced = only(&req, &chosen);
        let connection = facts::comline2_connection(&reduced).unwrap().unwrap();
        let (expected, _) = facts::hello(&reduced, &connection).unwrap();

        let entries: Vec<_> = parts.protocols.iter().filter(|p| chosen.contains(&p.name.as_str())).collect();
        let bytes = parts.assemble(entries.iter().copied());
        assert_eq!(bytes, expected, "{chosen:?}");
        assert_eq!(&bytes[..header.len()], &header[..], "{chosen:?}: the encodings are listed once, the same");

        // However they are given, and however many times.
        let mut shuffled: Vec<_> = entries.iter().rev().chain(entries.iter()).copied().collect();
        shuffled.rotate_left(chosen.len() / 2);
        assert_eq!(parts.assemble(shuffled), expected, "{chosen:?}, out of order and doubled");
    }
}

#[test]
fn a_hello_of_no_protocol_lists_none() {
    let (req, connection) = subset();
    let parts = facts::hello_parts(&req, &connection).unwrap();
    let bytes = parts.assemble(std::iter::empty());
    assert_eq!(bytes.last(), Some(&0));
    assert!(bytes.len() < parts.assemble_all().len());
}
