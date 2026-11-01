//! The renderers' documentation model types (`comline_codegen::doc_model`)
//! against core's (`comline_core::doc`, `schemas/doc-model.json`)
//! (generation#96).
//!
//! The renderers' golden model is checked against core's schema and must
//! round-trip through core's types unchanged, so it has exactly the shape
//! core writes today. The renderers' golden pages then prove these types
//! read every part of it. When core's model changes, this fails: regenerate
//! the golden model from core, and the golden pages show what moved.

use std::path::Path;

use comline_codegen::doc_model::{DocModel, Type, Unit};

fn golden() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../doc-markdown/tests/golden/model.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_golden_model_is_valid_against_cores_schema() {
    let validator = jsonschema::draft7::new(&comline_core::doc::json_schema()).expect("core's schema is valid draft-07");
    let golden = golden();
    let errors: Vec<String> = validator.iter_errors(&golden).map(|e| format!("{} at {}", e, e.instance_path())).collect();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn the_golden_model_round_trips_through_cores_types() {
    let golden = golden();
    let model: comline_core::doc::DocModel = serde_json::from_value(golden.clone()).unwrap();
    assert_eq!(serde_json::to_value(&model).unwrap(), golden, "core's model has moved; regenerate the golden model");
}

#[test]
fn these_types_read_every_unit_of_it() {
    let model: DocModel = serde_json::from_value(golden()).unwrap();
    assert_eq!(model.model_version, comline_core::doc::DOC_MODEL_VERSION);
    let units: Vec<&Unit> = model.modules.iter().flat_map(|m| &m.units).collect();
    assert!(!units.is_empty());
    for unit in units {
        assert!(!matches!(unit, Unit::Unknown), "a unit kind these types don't know");
        if let Unit::Protocol { wire, .. } = unit {
            assert!(wire.as_ref().is_some_and(|w| !w.guarantees.is_empty()));
        }
        if let Unit::Struct { fields, .. } = unit {
            assert!(fields.iter().all(|f| !matches!(&f.ty, Type::Named { target: None, .. })));
        }
    }
}
