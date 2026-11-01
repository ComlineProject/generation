//! The documentation model as this renderer reads it: the JSON shape of
//! `comline_core::doc::DocModel` (core's `schemas/doc-model.json`).
//!
//! These are the renderer's own types, not core's: core's model lives behind
//! its compiler, which a renderer built for `wasm32-wasip1` can't link, and a
//! renderer only needs to read the model. Everything is lenient: a missing
//! field takes its default, an unknown one is ignored.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DocModel {
    pub model_version: u32,
    pub package: Package,
    pub modules: Vec<Module>,
    pub changelog: Vec<Changelog>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Package {
    pub name: String,
    pub version: Option<String>,
    pub docs: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Module {
    pub path: String,
    pub docs: Option<String>,
    pub units: Vec<Unit>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Doc {
    pub text: String,
    pub params: Vec<DocParam>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DocParam {
    pub name: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Annotation {
    pub key: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Unit {
    Struct {
        name: String,
        #[serde(default)]
        docs: Option<Doc>,
        #[serde(default)]
        annotations: Vec<Annotation>,
        #[serde(default)]
        fields: Vec<Field>,
    },
    Enum {
        name: String,
        #[serde(default)]
        docs: Option<Doc>,
        #[serde(default)]
        variants: Vec<String>,
    },
    Protocol {
        name: String,
        #[serde(default)]
        docs: Option<Doc>,
        #[serde(default)]
        annotations: Vec<Annotation>,
        #[serde(default)]
        functions: Vec<Function>,
        #[serde(default)]
        wire: Option<ProtocolSummary>,
    },
    Error {
        name: String,
        #[serde(default)]
        docs: Option<Doc>,
        #[serde(default)]
        annotations: Vec<Annotation>,
        #[serde(default)]
        ordinal: u16,
        #[serde(default)]
        message: String,
        #[serde(default)]
        fields: Vec<Field>,
    },
    Validator {
        name: String,
        #[serde(default)]
        docs: Option<Doc>,
        #[serde(default)]
        annotations: Vec<Annotation>,
        #[serde(default)]
        targets: Vec<Type>,
        #[serde(default)]
        params: Vec<Field>,
        #[serde(default)]
        asserts: Vec<Assert>,
    },
    Constant {
        name: String,
        #[serde(default)]
        docs: Option<Doc>,
        #[serde(rename = "type")]
        ty: Type,
        #[serde(default)]
        value: Option<String>,
    },
    TypeAlias {
        name: String,
        #[serde(default)]
        docs: Option<Doc>,
        #[serde(default)]
        annotations: Vec<Annotation>,
        target: Type,
    },
    Settings {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        docs: Option<Doc>,
        #[serde(default)]
        values: Vec<SettingsEntry>,
    },
    /// A kind this renderer doesn't know (a newer model): skipped.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Field {
    pub name: String,
    #[serde(default)]
    pub docs: Option<Doc>,
    #[serde(default)]
    pub annotations: Vec<Annotation>,
    #[serde(rename = "type")]
    pub ty: Type,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub default: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Function {
    pub name: String,
    #[serde(default)]
    pub docs: Option<Doc>,
    #[serde(default)]
    pub annotations: Vec<Annotation>,
    #[serde(default)]
    pub arguments: Vec<Argument>,
    #[serde(default)]
    pub returns: Option<Type>,
    #[serde(default)]
    pub throws: Vec<Type>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Argument {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: Type,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Assert {
    pub condition: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SettingsEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Type {
    Primitive {
        name: String,
    },
    Named {
        name: String,
        #[serde(default)]
        target: Option<UnitRef>,
    },
    Array {
        element: Box<Type>,
        #[serde(default)]
        length: Option<u64>,
    },
    Union {
        #[serde(default)]
        members: Vec<Type>,
    },
    Unit,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UnitRef {
    pub package: String,
    pub schema: String,
    pub name: String,
}

/// `comline_core::inspect::ProtocolSummary`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProtocolSummary {
    pub format: Choice,
    pub framing: Choice,
    pub handshake: Choice,
    pub unverified: bool,
    pub guarantees: Vec<GuaranteeRow>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Choice {
    pub value: String,
    pub params: BTreeMap<String, String>,
    /// `annotation`, `package_default`, `core_default` or `framing`.
    pub source: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct GuaranteeRow {
    /// Its written name: `schema_agreement`.
    pub guarantee: String,
    pub held: bool,
    pub causes: Vec<String>,
    pub reason: Option<String>,
    pub acceptance: Acceptance,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Acceptance {
    /// `not_needed`, `unknown`, `accepted` or `not_accepted`.
    pub state: String,
    /// Who accepted it, for `accepted`.
    pub by: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Changelog {
    pub from: String,
    pub to: String,
    /// Each change as `comline_core::schema::ir::diff` serializes it: a
    /// `kind` and that kind's fields.
    pub breaking: Vec<Value>,
    pub features: Vec<Value>,
    pub modifications: Vec<Value>,
}
