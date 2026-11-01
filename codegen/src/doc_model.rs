//! The documentation model as a renderer reads it: the JSON shape of
//! `comline_core::doc::DocModel` (core's `schemas/doc-model.json`).
//!
//! These are the renderers' own types, not core's: core's model lives behind
//! its compiler, which a renderer built for `wasm32-wasip1` can't link, and a
//! renderer only needs to read the model. Everything is lenient: a missing
//! field takes its default, an unknown one is ignored.
//!
//! Shared by the official renderers (`comline-doc-markdown`,
//! `comline-doc-html`), with the helpers they agree on: unit names, kinds and
//! anchors, page paths, `@name:` parameter docs, and the wire summary's rows.

use std::collections::BTreeMap;

use comline_core::schema::ir::frozen::unit::Guarantee;
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

impl Unit {
    /// Its name; a `settings` block without one is `settings`. `None` for
    /// [`Unit::Unknown`].
    pub fn name(&self) -> Option<&str> {
        match self {
            Unit::Struct { name, .. }
            | Unit::Enum { name, .. }
            | Unit::Protocol { name, .. }
            | Unit::Error { name, .. }
            | Unit::Validator { name, .. }
            | Unit::Constant { name, .. }
            | Unit::TypeAlias { name, .. } => Some(name),
            Unit::Settings { name, .. } => Some(name.as_deref().unwrap_or("settings")),
            Unit::Unknown => None,
        }
    }

    /// Its kind as written: `struct`, `const`, `type`, ...
    pub fn kind(&self) -> &'static str {
        match self {
            Unit::Struct { .. } => "struct",
            Unit::Enum { .. } => "enum",
            Unit::Protocol { .. } => "protocol",
            Unit::Error { .. } => "error",
            Unit::Validator { .. } => "validator",
            Unit::Constant { .. } => "const",
            Unit::TypeAlias { .. } => "type",
            Unit::Settings { .. } => "settings",
            Unit::Unknown => "",
        }
    }

    pub fn docs(&self) -> Option<&Doc> {
        match self {
            Unit::Struct { docs, .. }
            | Unit::Enum { docs, .. }
            | Unit::Protocol { docs, .. }
            | Unit::Error { docs, .. }
            | Unit::Validator { docs, .. }
            | Unit::Constant { docs, .. }
            | Unit::TypeAlias { docs, .. }
            | Unit::Settings { docs, .. } => docs.as_ref(),
            Unit::Unknown => None,
        }
    }

    pub fn annotations(&self) -> &[Annotation] {
        match self {
            Unit::Struct { annotations, .. }
            | Unit::Protocol { annotations, .. }
            | Unit::Error { annotations, .. }
            | Unit::Validator { annotations, .. }
            | Unit::TypeAlias { annotations, .. } => annotations,
            _ => &[],
        }
    }
}

/// A unit's anchor: its name in lowercase. A function's is
/// `<protocol>-<function>`.
pub fn anchor(name: &str) -> String {
    name.to_lowercase()
}

/// A module's page, relative to the output root, with the extension `ext`:
/// `types::user` is `types/user.<ext>`.
pub fn page_of(module: &str, ext: &str) -> String {
    format!("{}.{ext}", module.replace("::", "/"))
}

/// `to` (a page path) relative to the page `from`.
pub fn relative(from: &str, to: &str) -> String {
    let from: Vec<&str> = from.split('/').collect();
    let to: Vec<&str> = to.split('/').collect();
    let from_dir = &from[..from.len() - 1];
    let common = from_dir.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; from_dir.len() - common];
    parts.extend(&to[common..]);
    parts.join("/")
}

/// The first non-empty line of `docs`.
pub fn summary(docs: &str) -> &str {
    docs.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("")
}

/// The docs of a field or argument: its own, else its parent's `@name:` line.
pub fn param_doc(own: Option<&Doc>, parent: Option<&Doc>, name: &str) -> String {
    if let Some(own) = own {
        if !own.text.trim().is_empty() {
            return own.text.trim().to_string();
        }
    }
    parent
        .and_then(|p| p.params.iter().find(|d| d.name == name))
        .map(|d| d.text.trim().to_string())
        .unwrap_or_default()
}

/// Where one wire choice came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceSource {
    /// `@format`, `@framing` or `@handshake`: the key.
    Annotation(&'static str),
    /// config.idp: `handshake`, `wire.format` or `wire.framing`.
    PackageDefault(String),
    CoreDefault,
    /// Set by JSON-RPC's format.
    Framing,
    Other(String),
}

/// One row of a wire summary's choices.
#[derive(Debug, Clone)]
pub struct ChoiceRow<'a> {
    /// `format`, `framing` or `handshake`.
    pub key: &'static str,
    pub value: &'a str,
    pub params: Vec<(&'a str, &'a str)>,
    pub source: ChoiceSource,
}

/// Whether a given-up guarantee was accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Accepted<'a> {
    /// Nothing to accept.
    None,
    ByEachConsumer,
    By(&'a str),
    NotAccepted,
}

/// One row of a wire summary's guarantees.
#[derive(Debug, Clone)]
pub struct GuaranteeView<'a> {
    /// In words, as the language server's hover puts it.
    pub guarantee: &'a str,
    pub held: bool,
    pub causes: String,
    pub accepted: Accepted<'a>,
}

impl ProtocolSummary {
    /// The format, framing and handshake rows: the same rows as the
    /// language server's hover (`protocol_summary.rs`) and `comline inspect`.
    pub fn choices(&self) -> Vec<ChoiceRow<'_>> {
        [("format", &self.format), ("framing", &self.framing), ("handshake", &self.handshake)]
            .into_iter()
            .map(|(key, choice)| ChoiceRow {
                key,
                value: &choice.value,
                params: choice.params.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect(),
                source: match choice.source.as_str() {
                    "annotation" => ChoiceSource::Annotation(key),
                    "package_default" if key == "handshake" => ChoiceSource::PackageDefault("handshake".into()),
                    "package_default" => ChoiceSource::PackageDefault(format!("wire.{key}")),
                    "core_default" => ChoiceSource::CoreDefault,
                    "framing" => ChoiceSource::Framing,
                    other => ChoiceSource::Other(other.to_string()),
                },
            })
            .collect()
    }

    /// The guarantee rows.
    pub fn guarantee_rows(&self) -> Vec<GuaranteeView<'_>> {
        self.guarantees
            .iter()
            .map(|row| GuaranteeView {
                guarantee: Guarantee::from_name(&row.guarantee).map(|g| g.describe()).unwrap_or(&row.guarantee),
                held: row.held,
                causes: row.causes.join("; "),
                accepted: match row.acceptance.state.as_str() {
                    "unknown" => Accepted::ByEachConsumer,
                    "accepted" => Accepted::By(row.acceptance.by.as_deref().unwrap_or("")),
                    "not_accepted" => Accepted::NotAccepted,
                    _ => Accepted::None,
                },
            })
            .collect()
    }

    /// The `@unverified` reason, when a guarantee was given up with one.
    pub fn unverified_reason(&self) -> Option<&str> {
        self.guarantees.iter().find_map(|g| g.reason.as_deref())
    }
}

/// One changelog entry, as `comline_core::schema::ir::diff` serializes it,
/// split for display: its kind in words (`Field added`) and its other fields
/// as `(name in words, value)`, a string value bare and any other as JSON.
/// `None` when the entry isn't an object.
pub fn change_parts(change: &Value) -> Option<(String, Vec<(String, String)>)> {
    let object = change.as_object()?;
    let kind = object.get("kind").and_then(|k| k.as_str()).unwrap_or("change");
    let mut words = kind.replace('_', " ");
    if let Some(first) = words.get(..1) {
        words = first.to_uppercase() + &words[1..];
    }
    let details = object
        .iter()
        .filter(|(k, _)| k.as_str() != "kind")
        .map(|(k, v)| {
            let v = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            (k.replace('_', " "), v)
        })
        .collect();
    Some((words, details))
}

/// A type as text: `Order[]`, `union(u8 string)`.
pub fn type_text(ty: &Type) -> String {
    match ty {
        Type::Primitive { name } | Type::Named { name, .. } => name.clone(),
        Type::Unit => "()".to_string(),
        Type::Array { element, length: Some(n) } => format!("{}[{n}]", type_text(element)),
        Type::Array { element, length: None } => format!("{}[]", type_text(element)),
        Type::Union { members } => {
            let members: Vec<String> = members.iter().map(type_text).collect();
            format!("union({})", members.join(" "))
        }
    }
}

/// A function's signature as text: `place(order: Order) -> bool ! OutOfStock`.
pub fn signature_text(f: &Function) -> String {
    let args: Vec<String> = f.arguments.iter().map(|a| format!("{}: {}", a.name, type_text(&a.ty))).collect();
    let mut out = format!("{}({})", f.name, args.join(", "));
    if let Some(returns) = &f.returns {
        out.push_str(&format!(" -> {}", type_text(returns)));
    }
    if !f.throws.is_empty() {
        let throws: Vec<String> = f.throws.iter().map(type_text).collect();
        out.push_str(&format!(" ! {}", throws.join(", ")));
    }
    out
}

/// A version's structure, for comparing versions side by side: one row per
/// unit (`api::Order`, its kind) and per member (`api::Order.total`, its
/// type; `api::Orders.place`, its signature). Structural only: whether a
/// difference is breaking comes from the changelog, never from here.
pub fn structure(model: &DocModel) -> BTreeMap<String, String> {
    let mut rows = BTreeMap::new();
    for module in &model.modules {
        for unit in &module.units {
            let Some(name) = unit.name() else { continue };
            let key = format!("{}::{name}", module.path);
            let mut member = |m: &str, cell: String| {
                rows.insert(format!("{key}.{m}"), cell);
            };
            let fields = |fields: &[Field], member: &mut dyn FnMut(&str, String)| {
                for f in fields {
                    let mut cell = type_text(&f.ty);
                    if f.optional {
                        cell = format!("optional {cell}");
                    }
                    if let Some(default) = &f.default {
                        cell.push_str(&format!(" = {default}"));
                    }
                    member(&f.name, cell);
                }
            };
            let head = match unit {
                Unit::Struct { fields: fs, .. } => {
                    fields(fs, &mut member);
                    "struct".to_string()
                }
                Unit::Enum { variants, .. } => {
                    for v in variants {
                        member(v, "variant".to_string());
                    }
                    "enum".to_string()
                }
                Unit::Error { ordinal, fields: fs, .. } => {
                    fields(fs, &mut member);
                    format!("error #{ordinal}")
                }
                Unit::Protocol { functions, .. } => {
                    for f in functions {
                        member(&f.name, signature_text(f));
                    }
                    "protocol".to_string()
                }
                Unit::Validator { targets, params, .. } => {
                    fields(params, &mut member);
                    let targets: Vec<String> = targets.iter().map(type_text).collect();
                    format!("validator for {}", targets.join(", "))
                }
                Unit::Constant { ty, value, .. } => match value {
                    Some(value) => format!("const {} = {value}", type_text(ty)),
                    None => format!("const {}", type_text(ty)),
                },
                Unit::TypeAlias { target, .. } => format!("alias of {}", type_text(target)),
                Unit::Settings { values, .. } => {
                    for v in values {
                        member(&v.key, v.value.clone());
                    }
                    "settings".to_string()
                }
                Unit::Unknown => continue,
            };
            rows.insert(key, head);
        }
    }
    rows
}

/// How much a changelog entry moves the version, by the list core put it
/// in: `breaking` is `major`, `features` is `minor`, `modifications` is
/// `patch`.
pub const SEVERITIES: [&str; 3] = ["major", "minor", "patch"];

/// The unit a changelog entry is about, as the declaration names it
/// (unqualified); empty when the entry names none.
pub fn change_unit(change: &Value) -> String {
    const KEYS: [&str; 6] = ["type_name", "enum_name", "protocol_name", "name", "old_name", "new_name"];
    let Some(object) = change.as_object() else { return String::new() };
    KEYS.iter().find_map(|k| object.get(*k).and_then(|v| v.as_str())).unwrap_or_default().to_string()
}

/// One changelog entry as plain text: its kind in words, then its fields.
pub fn change_text(change: &Value) -> String {
    let Some((words, details)) = change_parts(change) else { return change.to_string() };
    let details: Vec<String> = details.iter().map(|(k, v)| format!("{k}: {v}")).collect();
    if details.is_empty() {
        words
    } else {
        format!("{words}: {}", details.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert_eq!(relative("api.md", "types.md"), "types.md");
        assert_eq!(relative("types/user.md", "api.md"), "../api.md");
        assert_eq!(relative("types/user.md", "types/item.md"), "item.md");
        assert_eq!(relative("api.md", "types/user.md"), "types/user.md");
    }
}
