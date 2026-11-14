//! A package that uses only a dependency's types generates, though the
//! dependency declares a protocol: its protocol isn't on the package's
//! connection, so nothing addresses it, checks it or serves it
//! (generation#143, step 0 of the dependency protocols proposal).
//!
//! What every generator shares is checked here, on the request each gets:
//! `facts::check`, which they all run first, and the package connection and
//! addressing they build from. The fixtures aren't in the corpus yet: a
//! generator that hasn't moved to `package_connection_among` and
//! `addressing_in` still puts the dependency's protocol on `Services`, and a
//! golden would pin that.

use std::collections::BTreeMap;

use comline_codegen::validation::ValidateOnDecode;
use comline_codegen::{connections, facts, GenerateRequest, PackageMeta, TargetLanguage};
use comline_conformance::{dependency_and_own_protocols, dependency_protocol, Fixture};
use comline_core::schema::ir::frozen::unit::FrozenUnit;

/// The request `comline generation` would send for `fx`: with the facts of
/// the package's own schemas when they're needed.
fn request(fx: &Fixture) -> GenerateRequest {
    let dependencies: Vec<String> = fx.dependencies.iter().map(|d| d.to_string()).collect();
    let own: Vec<_> =
        fx.schemas.iter().filter(|(namespace, _)| !connections::is_dependency(&dependencies, namespace)).cloned().collect();
    let encodings = comline_codegen::encodings::official_for("rust");
    let mut req = GenerateRequest {
        mode: fx.mode,
        schemas: fx.schemas.clone(),
        package: PackageMeta { name: "conformance".into(), version: "0.0.0".into() },
        target: TargetLanguage { language: "rust".into(), version: "1.85.0".into() },
        wire_format: "packed@1".into(),
        external_std: false,
        validate_on_decode: ValidateOnDecode::Off,
        async_api: false,
        encodings,
        facts: None,
        dependencies,
    };
    if facts::needed_in(&req) {
        req.facts = Some(comline_conformance::facts_for(&own, "packed@1"));
    }
    req
}

#[test]
fn a_package_using_only_a_dependencys_types_needs_no_facts_and_has_no_connection() {
    let req = request(&dependency_protocol());
    // The dependency's protocol runs `comline@2` on `datagram@3`, which is
    // why every protocol was checked against the package's facts.
    assert!(facts::needed(&req.schemas, &req.encodings));
    assert!(!facts::needed_in(&req));
    assert!(req.facts.is_none());
    facts::check(&req).unwrap();
    assert_eq!(connections::package_connection_among(&req.schemas, &req.dependencies).unwrap(), None);
    assert_eq!(facts::comline2_connection(&req).unwrap(), None);
}

#[test]
fn a_dependencys_protocol_is_not_addressed_beside_the_packages_own() {
    let req = request(&dependency_and_own_protocols());
    assert!(facts::needed_in(&req));
    // The facts are the package's: they have no numbers for `Foo`.
    let numbered: Vec<_> = req.facts.as_ref().unwrap().protocols.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(numbered, ["api::Chat"]);
    facts::check(&req).unwrap();

    let connection = facts::comline2_connection(&req).unwrap().unwrap();
    let served: Vec<_> = connection.server.iter().map(|p| format!("{}::{}", p.namespace, p.name)).collect();
    assert_eq!(served, ["api::Chat"]);
    let (_, hello) = facts::hello(&req, &connection).unwrap();
    assert_eq!(hello.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Chat"]);

    // `Chat` is addressed by its wire numbers, `Foo` by position.
    let mut addressed = BTreeMap::new();
    for (namespace, units) in &req.schemas {
        for unit in units {
            let FrozenUnit::Protocol { name, parameters, functions, .. } = unit else { continue };
            let at = facts::addressing_in(&req, namespace, name, parameters, functions).unwrap();
            addressed.insert(name.as_str(), (at.protocol, at.checks.is_some()));
        }
    }
    let facts_number = req.facts.as_ref().unwrap().protocols[0].number;
    assert_eq!(addressed["Chat"], (facts_number, true));
    assert_eq!(addressed["Foo"], (0, false));
}
