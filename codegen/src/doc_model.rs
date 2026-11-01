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

/// One changelog entry, ready to show: how much it moves the version, a
/// short title, the unit (and member) it's about, and either its old and
/// new IDL lines (core's `old` / `new`) or, for a kind with nothing to show
/// as IDL, a sentence. The renderers agree on all of it; they only differ in
/// markup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// `major`, `minor` or `patch`: the list core put the entry in.
    pub severity: &'static str,
    /// The unit it's about, as the declaration names it (unqualified);
    /// empty when it names none (a namespace rename).
    pub unit: String,
    /// The member within the unit, when it's about one: a field, a
    /// variant or a function.
    pub member: Option<String>,
    /// `Greeter.greet` and what happened to it: `signature changed`.
    pub subject: String,
    pub what: String,
    pub old: Option<String>,
    pub new: Option<String>,
    /// Said in words, for an entry with no IDL lines.
    pub sentence: Option<String>,
}

impl Entry {
    /// `Greeter.greet: signature changed`, as plain text.
    pub fn title(&self) -> String {
        if self.subject.is_empty() {
            capitalize(&self.what)
        } else {
            format!("{}: {}", self.subject, self.what)
        }
    }
}

/// How much a changelog entry moves the version, by the list core put it
/// in: `breaking` is `major`, `features` is `minor`, `modifications` is
/// `patch`.
pub const SEVERITIES: [&str; 3] = ["major", "minor", "patch"];

/// A changelog's entries, each with the severity of the list it's in.
pub fn entries(log: &Changelog) -> Vec<Entry> {
    SEVERITIES
        .iter()
        .zip([&log.breaking, &log.features, &log.modifications])
        .flat_map(|(severity, list)| list.iter().map(|change| entry(change, severity)))
        .collect()
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// One entry as `comline_core::schema::ir::diff` serializes it (a `kind`
/// and that kind's fields, plus core's `old` / `new` lines), ready to show.
pub fn entry(change: &Value, severity: &'static str) -> Entry {
    let get = |k: &str| change.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let opt = |k: &str| change.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let kind = get("kind");
    let line = |k: &str| opt(k).filter(|l| !l.trim().is_empty());
    let (old, new) = (line("old"), line("new"));

    // The unit and member it's about.
    let (unit, member) = if let Some(target) = opt("target") {
        match target.split_once('.') {
            Some((u, m)) => (u.to_string(), Some(m.to_string())),
            None => (target, None),
        }
    } else if let Some(t) = opt("type_name") {
        (t, opt("field_name"))
    } else if let Some(e) = opt("enum_name") {
        let variant = if kind == "renamed_enum_variant" { None } else { opt("variant") };
        (e, variant)
    } else if let Some(p) = opt("protocol_name") {
        (p, opt("function_name"))
    } else if kind == "renamed_protocol" {
        (get("new_name"), None)
    } else if let Some(n) = opt("name").filter(|_| kind != "renamed_namespace") {
        match n.split_once('.') {
            Some((u, m)) => (u.to_string(), Some(m.to_string())),
            None => (n, None),
        }
    } else {
        (String::new(), None)
    };
    let subject = match (&member, unit.is_empty()) {
        (_, true) => String::new(),
        (Some(m), false) => format!("{unit}.{m}"),
        (None, false) => unit.clone(),
    };

    let words = |k: &str| k.replace('_', " ");
    let what = match kind.as_str() {
        "changed_function_signature" => "signature changed".to_string(),
        "changed_field_type" | "changed_constant_type" => "type changed".to_string(),
        "changed_field_optionality" => {
            if change.get("now_optional").and_then(|v| v.as_bool()) == Some(true) {
                "made optional".to_string()
            } else {
                "made required".to_string()
            }
        }
        "renamed_enum_variant" => format!("variant {} renamed to {}", get("old_name"), get("new_name")),
        "renamed_protocol" => format!("renamed from {}", get("old_name")),
        "renamed_namespace" => format!("namespace {} renamed to {}", get("old_name"), get("new_name")),
        "added_thrown_error" => format!("now throws {}", get("error")),
        "removed_thrown_error" => format!("no longer throws {}", get("error")),
        "changed_error_ordinal" => "ordinal changed".to_string(),
        "changed_type_alias_target" => "target changed".to_string(),
        "changed_validator" => "validator changed".to_string(),
        "changed_constant_value" => "value changed".to_string(),
        "changed_field_default" => "default changed".to_string(),
        "changed_error_message" => "message changed".to_string(),
        "changed_wire_annotation" | "changed_annotation" => format!("@{} changed", get("key")),
        "gave_up_guarantees" => "gives up guarantees".to_string(),
        "restored_guarantees" => "keeps its guarantees again".to_string(),
        "docstring_changed" | "doc_changed" => "documentation changed".to_string(),
        "type_respelled" => "type respelled".to_string(),
        "deprecated" => "deprecated".to_string(),
        "validation_tightened" => "validation tightened".to_string(),
        "validation_loosened" => "validation loosened".to_string(),
        k if k.starts_with("reordered_") => format!("{} reordered", words(&k["reordered_".len()..])),
        k if k.starts_with("added_") => format!("{} added", words(&k["added_".len()..])),
        k if k.starts_with("removed_") => format!("{} removed", words(&k["removed_".len()..])),
        "" => "changed".to_string(),
        k => words(k),
    };

    let sentence = if old.is_some() || new.is_some() {
        None
    } else {
        let list = |k: &str| {
            change
                .get(k)
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|g| g.as_str()).collect::<Vec<_>>().join(", "))
                .unwrap_or_default()
        };
        let from_to = |a: &str, b: &str| match (opt(a), opt(b)) {
            (Some(a), Some(b)) => Some(format!("From {a} to {b}.")),
            _ => None,
        };
        match kind.as_str() {
            "docstring_changed" | "doc_changed" => Some("Its documentation changed; nothing on the wire did.".to_string()),
            "changed_error_ordinal" => {
                let n = |k: &str| change.get(k).map(|v| v.to_string()).unwrap_or_default();
                Some(format!("Its ordinal, which travels on the wire, moved from {} to {}.", n("old_ordinal"), n("new_ordinal")))
            }
            "gave_up_guarantees" => Some(format!("It gives up: {}.", list("guarantees"))),
            "restored_guarantees" => Some(format!("It keeps again: {}.", list("guarantees"))),
            "reordered_functions" => Some("Its functions moved, and its framing calls them by position.".to_string()),
            "reordered_fields" => Some("Its fields moved, and a format in play keys them by position.".to_string()),
            "reordered_enum_variants" => {
                Some("Its variants moved, and a format in play encodes them by position.".to_string())
            }
            "renamed_namespace" => Some("Paths that name it need the new name.".to_string()),
            "changed_field_type" | "type_respelled" => from_to("old_type", "new_type"),
            "changed_constant_type" => from_to("old_type", "new_type"),
            "changed_type_alias_target" => from_to("old_target", "new_target"),
            "changed_constant_value" | "changed_wire_annotation" | "changed_annotation" => {
                from_to("old_value", "new_value")
            }
            _ => None,
        }
    };

    Entry { severity, unit, member, subject, what, old, new, sentence }
}

/// Where the unit (or function) an entry is about is documented in
/// `model`, from the site's root: `api.html#greeter-greet` for `ext` `html`.
/// `None` when the version doesn't have it (a removal).
pub fn unit_target(model: &DocModel, e: &Entry, ext: &str) -> Option<String> {
    model.modules.iter().find_map(|module| {
        let unit = module.units.iter().find(|u| u.name() == Some(e.unit.as_str()))?;
        let mut id = anchor(&e.unit);
        if let (Unit::Protocol { functions, .. }, Some(member)) = (unit, &e.member) {
            if functions.iter().any(|f| &f.name == member) {
                id = format!("{id}-{}", anchor(member));
            }
        }
        Some(format!("{}#{id}", page_of(&module.path, ext)))
    })
}

/// What a piece of an IDL line is, for highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Keyword,
    /// A type: a primitive, or a capitalized name.
    Type,
    Name,
    Punct,
    /// A string, f-string, number, `true` or `false`.
    Literal,
    /// `@key`.
    Annotation,
    Comment,
    Space,
}

impl Token {
    /// The CSS class a renderer marks it with.
    pub fn class(self) -> Option<&'static str> {
        Some(match self {
            Token::Keyword => "k",
            Token::Type => "t",
            Token::Name => "n",
            Token::Punct => "p",
            Token::Literal => "l",
            Token::Annotation => "a",
            Token::Comment => "c",
            Token::Space => return None,
        })
    }
}

const KEYWORDS: &[&str] =
    &["struct", "enum", "protocol", "function", "error", "const", "type", "validator", "for", "optional", "message", "union", "use", "as"];
const PRIMITIVES: &[&str] = &[
    "u8", "u16", "u32", "u64", "u128", "s8", "s16", "s32", "s64", "s128", "i8", "i16", "i32", "i64", "f32", "f64",
    "bool", "string", "bytes", "char",
];

/// An IDL line split into highlighted pieces. Lexical only: a lowercase
/// name is a name, a capitalized one or a primitive a type.
pub fn tokens(line: &str) -> Vec<(Token, &str)> {
    let mut out = vec![];
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < line.len() {
        let rest = &line[i..];
        let c = rest.chars().next().unwrap();
        let len = if c.is_whitespace() {
            let n = rest.find(|ch: char| !ch.is_whitespace()).unwrap_or(rest.len());
            out.push((Token::Space, &rest[..n]));
            n
        } else if rest.starts_with("/*") {
            let n = rest.find("*/").map(|n| n + 2).unwrap_or(rest.len());
            out.push((Token::Comment, &rest[..n]));
            n
        } else if c == '"' || (c == 'f' && rest[1..].starts_with('"')) {
            let start = if c == 'f' { 2 } else { 1 };
            let mut n = start;
            let mut escaped = false;
            for ch in rest[start..].chars() {
                n += ch.len_utf8();
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    break;
                }
            }
            out.push((Token::Literal, &rest[..n]));
            n
        } else if c == '@' {
            let n = 1 + rest[1..].find(|ch: char| !(ch.is_alphanumeric() || ch == '_')).unwrap_or(rest.len() - 1);
            out.push((Token::Annotation, &rest[..n]));
            n
        } else if c.is_ascii_digit() || (c == '-' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let n = 1 + rest[1..].find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '_')).unwrap_or(rest.len() - 1);
            out.push((Token::Literal, &rest[..n]));
            n
        } else if c.is_alphabetic() || c == '_' {
            let n = rest.find(|ch: char| !(ch.is_alphanumeric() || ch == '_')).unwrap_or(rest.len());
            let word = &rest[..n];
            let token = if KEYWORDS.contains(&word) {
                Token::Keyword
            } else if word == "true" || word == "false" {
                Token::Literal
            } else if PRIMITIVES.contains(&word) || word.starts_with(char::is_uppercase) {
                Token::Type
            } else {
                Token::Name
            };
            out.push((token, word));
            n
        } else if rest.starts_with("->") || rest.starts_with("::") {
            out.push((Token::Punct, &rest[..2]));
            2
        } else {
            out.push((Token::Punct, &rest[..c.len_utf8()]));
            c.len_utf8()
        };
        i += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_signature_entry() {
        let change = serde_json::json!({
            "kind": "changed_function_signature", "protocol_name": "Greeter", "function_name": "greet",
            "details": "(name: string) -> Greeting → (name: string, formal: bool) -> Greeting",
            "old": "function greet(name: string) -> Greeting;",
            "new": "function greet(name: string, formal: bool) -> Greeting;",
        });
        let e = entry(&change, "major");
        assert_eq!(e.title(), "Greeter.greet: signature changed");
        assert_eq!((e.unit.as_str(), e.member.as_deref()), ("Greeter", Some("greet")));
        assert_eq!(e.sentence, None);
        let docs = entry(&serde_json::json!({"kind": "docstring_changed", "name": "Order.total"}), "patch");
        assert_eq!(docs.title(), "Order.total: documentation changed");
        assert!(docs.sentence.is_some());
    }

    #[test]
    fn highlights_an_idl_line() {
        let classes: Vec<(Option<&str>, &str)> = tokens("function greet(name: string) -> Greeting ! Rude;")
            .into_iter()
            .filter(|(t, _)| *t != Token::Space)
            .map(|(t, s)| (t.class(), s))
            .collect();
        assert_eq!(
            classes,
            [
                (Some("k"), "function"), (Some("n"), "greet"), (Some("p"), "("), (Some("n"), "name"), (Some("p"), ":"),
                (Some("t"), "string"), (Some("p"), ")"), (Some("p"), "->"), (Some("t"), "Greeting"), (Some("p"), "!"),
                (Some("t"), "Rude"), (Some("p"), ";"),
            ]
        );
        let lit: Vec<&str> =
            tokens("@deprecated = \"use \\\"x\\\"\" f\"{a}\" 42").into_iter().filter(|(t, _)| *t == Token::Literal).map(|(_, s)| s).collect();
        assert_eq!(lit, ["\"use \\\"x\\\"\"", "f\"{a}\"", "42"]);
    }

    #[test]
    fn relative_paths() {
        assert_eq!(relative("api.md", "types.md"), "types.md");
        assert_eq!(relative("types/user.md", "api.md"), "../api.md");
        assert_eq!(relative("types/user.md", "types/item.md"), "item.md");
        assert_eq!(relative("api.md", "types/user.md"), "types/user.md");
    }
}
