//! A protocol in a nested schema (`chat/admin`) finds its facts, which name
//! its namespace the way core does (`chat::admin`): a request's namespaces
//! are `/`-joined, the facts' `::`-joined.
//!
//! What every generator shares is checked here, on the request each gets:
//! `facts::check`, which they all run first, and the package connection,
//! Hello and addressing they build from. The fixture is in the corpus too,
//! so each generator's own output for it is held by the goldens.

use comline_codegen::validation::ValidateOnDecode;
use comline_codegen::{facts, GenerateRequest, PackageMeta, TargetLanguage};
use comline_conformance::{facts_for, nested_protocol, Fixture};

/// The request `comline generation` would send for `fx`, with its facts.
fn request(fx: &Fixture) -> GenerateRequest {
    let mut req = GenerateRequest {
        mode: fx.mode,
        schemas: fx.schemas.clone(),
        package: PackageMeta { name: "conformance".into(), version: "0.0.0".into() },
        target: TargetLanguage { language: "rust".into(), version: "1.85.0".into() },
        wire_format: "packed@1".into(),
        external_std: false,
        validate_on_decode: ValidateOnDecode::Off,
        async_api: false,
        encodings: comline_codegen::encodings::official_for("rust"),
        facts: None,
        dependencies: vec![],
    };
    assert!(facts::needed_in(&req));
    req.facts = Some(facts_for(&fx.schemas, "packed@1"));
    req
}

#[test]
fn the_facts_and_the_request_spell_the_namespace_differently() {
    let req = request(&nested_protocol());
    assert_eq!(req.schemas[0].0, "chat/admin");
    let facts = req.facts.as_ref().unwrap();
    assert_eq!(facts.protocols[0].namespace, "chat::admin");
    assert_eq!(facts.protocols[0].path, "chat::admin::Admin");
}

#[test]
fn a_nested_protocol_finds_its_facts() {
    let req = request(&nested_protocol());
    facts::check(&req).unwrap();

    let connection = facts::comline2_connection(&req).unwrap().unwrap();
    let (_, hello) = facts::hello(&req, &connection).unwrap();
    let hello: Vec<_> = hello.iter().map(|p| (p.namespace.as_str(), p.name.as_str(), p.number)).collect();
    assert_eq!(hello, [("chat/admin", "Admin", 0)]);
}

#[test]
fn a_protocol_in_another_namespace_is_not_found_and_says_so() {
    let mut req = request(&nested_protocol());
    req.schemas[0].0 = "chat/user".into();
    let error = facts::check(&req).unwrap_err().to_string();
    assert!(error.contains("no protocol `Admin` in namespace `chat/user`"), "{error}");
    assert!(error.contains("chat::admin::Admin") && error.contains("comline build"), "{error}");
    assert!(!error.contains("core#277"), "{error}");
}
