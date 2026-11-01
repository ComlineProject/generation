//! The official Markdown documentation renderer (core#145).
//!
//! A package's documentation model (`comline_core::doc`, as JSON) in,
//! Markdown pages out:
//!
//! - `index.md`: the package, its `//!` header and its modules;
//! - one page per module (`api.md`, `types/user.md`) with its header and every
//!   unit in it: docs, fields, arguments, and each protocol's wire summary
//!   (the rows the language server's hover shows);
//! - `changelog.md` when the model carries a changelog;
//! - `nav.yml`: a `nav` for mkdocs or zensical, listing the pages above.
//!
//! **Links.** Each unit has an anchor, its name in lowercase
//! (`<a id="order"></a>`), and a type links to its unit's anchor, on the same
//! page or another module's. A type from a dependency (or `std`) links to
//! `<base>/<module>/#<anchor>` when [`RenderOptions::links`] gives that
//! dependency a base URL, the directory-URL form mkdocs and zensical publish
//! pages at; otherwise it's plain text.

mod model;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use comline_codegen::protocol::{RenderOptions, RenderRequest};
use comline_codegen::{GeneratedFile, Registry, Renderer};
use comline_core::schema::ir::frozen::unit::Guarantee;
use eyre::{eyre, Result};

use model::*;

/// The documentation model versions this renderer reads.
pub const MODEL_VERSIONS: &[u32] = &[1];

/// This renderer, as it registers.
pub const RENDERER: Renderer = Renderer { id: "markdown", model_versions: MODEL_VERSIONS, render };

/// Register the Markdown renderer.
pub fn register(registry: &mut Registry) {
    registry.register_renderer(RENDERER);
}

/// [`RENDERER`]'s entry point.
pub fn render(req: &RenderRequest) -> Result<Vec<GeneratedFile>> {
    if !MODEL_VERSIONS.contains(&req.model_version) {
        return Err(eyre!("documentation model {} isn't one this renderer reads ({MODEL_VERSIONS:?})", req.model_version));
    }
    render_json(&req.model, &req.options)
}

/// The pages for the model `json`.
pub fn render_json(json: &str, options: &RenderOptions) -> Result<Vec<GeneratedFile>> {
    let model: DocModel = serde_json::from_str(json).map_err(|e| eyre!("the documentation model doesn't parse: {e}"))?;
    Ok(Pages { model: &model, links: &options.links }.all())
}

struct Pages<'a> {
    model: &'a DocModel,
    links: &'a BTreeMap<String, String>,
}

/// A module's page, relative to the output root: `types::user` is
/// `types/user.md`.
fn page_of(module: &str) -> String {
    format!("{}.md", module.replace("::", "/"))
}

fn anchor(name: &str) -> String {
    name.to_lowercase()
}

/// `to` (a page path) relative to the page `from`.
fn relative(from: &str, to: &str) -> String {
    let from: Vec<&str> = from.split('/').collect();
    let to: Vec<&str> = to.split('/').collect();
    let from_dir = &from[..from.len() - 1];
    let common = from_dir.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; from_dir.len() - common];
    parts.extend(&to[common..]);
    parts.join("/")
}

/// Text for one table cell: no `|` or line break of its own.
fn cell(text: &str) -> String {
    text.trim().replace('|', "\\|").replace('\n', "<br>")
}

fn unit_name(unit: &Unit) -> Option<&str> {
    match unit {
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

fn kind_of(unit: &Unit) -> &'static str {
    match unit {
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

fn summary(docs: &str) -> &str {
    docs.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("")
}

fn annotation(a: &Annotation) -> String {
    match &a.value {
        Some(value) => format!("`@{} = {}`", a.key, value),
        None => format!("`@{}`", a.key),
    }
}

impl Pages<'_> {
    fn all(&self) -> Vec<GeneratedFile> {
        let mut files = vec![file("index.md", self.index())];
        for module in &self.model.modules {
            files.push(file(&page_of(&module.path), self.module(module)));
        }
        if !self.model.changelog.is_empty() {
            files.push(file("changelog.md", self.changelog()));
        }
        files.push(file("nav.yml", self.nav()));
        files
    }

    fn title(&self) -> String {
        match &self.model.package.version {
            Some(version) => format!("{} {version}", self.model.package.name),
            None => self.model.package.name.clone(),
        }
    }

    fn index(&self) -> String {
        let mut out = format!("# {}\n\n", self.title());
        if let Some(docs) = &self.model.package.docs {
            out.push_str(&format!("{}\n\n", docs.trim()));
        }
        if !self.model.modules.is_empty() {
            out.push_str("## Modules\n\n| Module | |\n|---|---|\n");
            for module in &self.model.modules {
                let text = module.docs.as_deref().map(summary).unwrap_or("");
                out.push_str(&format!("| [`{}`]({}) | {} |\n", module.path, page_of(&module.path), cell(text)));
            }
            out.push('\n');
        }
        if !self.model.changelog.is_empty() {
            out.push_str("See the [changelog](changelog.md).\n");
        }
        finish(out)
    }

    fn nav(&self) -> String {
        let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
        let mut out = String::from("# The pages, as an mkdocs / zensical `nav`.\nnav:\n");
        out.push_str(&format!("  - {}: index.md\n", quote(&self.model.package.name)));
        for module in &self.model.modules {
            out.push_str(&format!("  - {}: {}\n", quote(&module.path), page_of(&module.path)));
        }
        if !self.model.changelog.is_empty() {
            out.push_str("  - Changelog: changelog.md\n");
        }
        out
    }

    fn module(&self, module: &Module) -> String {
        let page = page_of(&module.path);
        let mut out = format!("# `{}`\n\n", module.path);
        if let Some(docs) = &module.docs {
            out.push_str(&format!("{}\n\n", docs.trim()));
        }
        let units: Vec<&Unit> = module.units.iter().filter(|u| unit_name(u).is_some()).collect();
        if !units.is_empty() {
            out.push_str("| | |\n|---|---|\n");
            for unit in &units {
                let name = unit_name(unit).unwrap_or_default();
                let text = docs_of(unit).map(|d| summary(&d.text)).unwrap_or("");
                out.push_str(&format!("| {} [`{name}`](#{}) | {} |\n", kind_of(unit), anchor(name), cell(text)));
            }
            out.push('\n');
        }
        for unit in units {
            self.unit(&mut out, &page, unit);
        }
        finish(out)
    }

    fn unit(&self, out: &mut String, page: &str, unit: &Unit) {
        let name = unit_name(unit).unwrap_or_default();
        let _ = write!(out, "<a id=\"{}\"></a>\n\n## {} `{name}`\n\n", anchor(name), kind_of(unit));
        let docs = docs_of(unit);
        if let Some(docs) = docs {
            if !docs.text.trim().is_empty() {
                let _ = write!(out, "{}\n\n", docs.text.trim());
            }
        }
        let annotations = match unit {
            Unit::Struct { annotations, .. }
            | Unit::Protocol { annotations, .. }
            | Unit::Error { annotations, .. }
            | Unit::Validator { annotations, .. }
            | Unit::TypeAlias { annotations, .. } => annotations.as_slice(),
            _ => &[],
        };
        if !annotations.is_empty() {
            let list: Vec<String> = annotations.iter().map(annotation).collect();
            let _ = write!(out, "Annotations: {}\n\n", list.join(", "));
        }
        match unit {
            Unit::Struct { fields, .. } => self.fields(out, page, "Field", fields, docs),
            Unit::Enum { variants, .. } => {
                out.push_str("Variants:\n\n");
                for variant in variants {
                    let _ = writeln!(out, "- `{variant}`");
                }
                out.push('\n');
            }
            Unit::Error { ordinal, message, fields, .. } => {
                let _ = write!(out, "Ordinal `{ordinal}`. Message: `{message}`\n\n");
                self.fields(out, page, "Field", fields, docs);
            }
            Unit::Validator { targets, params, asserts, .. } => {
                let targets: Vec<String> = targets.iter().map(|t| self.ty(page, t)).collect();
                let _ = write!(out, "Applies to: {}\n\n", targets.join(", "));
                self.fields(out, page, "Parameter", params, docs);
                if !asserts.is_empty() {
                    out.push_str("| Assert | Message |\n|---|---|\n");
                    for a in asserts {
                        let _ = writeln!(out, "| `{}` | {} |", cell(&a.condition), cell(&a.message));
                    }
                    out.push('\n');
                }
            }
            Unit::Constant { ty, value, .. } => {
                let _ = write!(out, "Type: {}", self.ty(page, ty));
                if let Some(value) = value {
                    let _ = write!(out, ". Value: `{value}`");
                }
                out.push_str("\n\n");
            }
            Unit::TypeAlias { target, .. } => {
                let _ = write!(out, "Alias of {}\n\n", self.ty(page, target));
            }
            Unit::Settings { values, .. } => {
                if !values.is_empty() {
                    out.push_str("| Key | Value |\n|---|---|\n");
                    for v in values {
                        let _ = writeln!(out, "| `{}` | `{}` |", cell(&v.key), cell(&v.value));
                    }
                    out.push('\n');
                }
            }
            Unit::Protocol { functions, wire, .. } => {
                for function in functions {
                    self.function(out, page, name, function);
                }
                if let Some(wire) = wire {
                    wire_summary(out, wire);
                }
            }
            Unit::Unknown => {}
        }
    }

    fn fields(&self, out: &mut String, page: &str, what: &str, fields: &[Field], parent: Option<&Doc>) {
        if fields.is_empty() {
            return;
        }
        let _ = write!(out, "| {what} | Type | |\n|---|---|---|\n");
        for f in fields {
            let mut ty = self.ty(page, &f.ty);
            if f.optional {
                ty.push_str(" (optional)");
            }
            if let Some(default) = &f.default {
                let _ = write!(ty, " = `{default}`");
            }
            let mut text = param_doc(f.docs.as_ref(), parent, &f.name);
            for a in &f.annotations {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(&annotation(a));
            }
            let _ = writeln!(out, "| `{}` | {ty} | {} |", f.name, cell(&text));
        }
        out.push('\n');
    }

    fn function(&self, out: &mut String, page: &str, protocol: &str, f: &Function) {
        let args: Vec<String> = f.arguments.iter().map(|a| format!("{}: {}", a.name, self.ty(page, &a.ty))).collect();
        let _ = write!(
            out,
            "<a id=\"{}-{}\"></a>\n\n### `{}`\n\n",
            anchor(protocol),
            anchor(&f.name),
            f.name
        );
        let mut signature = format!("`{}`({})", f.name, args.join(", "));
        if let Some(returns) = &f.returns {
            let _ = write!(signature, " -> {}", self.ty(page, returns));
        }
        if !f.throws.is_empty() {
            let throws: Vec<String> = f.throws.iter().map(|t| self.ty(page, t)).collect();
            let _ = write!(signature, " ! {}", throws.join(", "));
        }
        let _ = write!(out, "{signature}\n\n");
        if let Some(docs) = &f.docs {
            if !docs.text.trim().is_empty() {
                let _ = write!(out, "{}\n\n", docs.text.trim());
            }
        }
        if !f.annotations.is_empty() {
            let list: Vec<String> = f.annotations.iter().map(annotation).collect();
            let _ = write!(out, "Annotations: {}\n\n", list.join(", "));
        }
        if !f.arguments.is_empty() {
            out.push_str("| Argument | Type | |\n|---|---|---|\n");
            for a in &f.arguments {
                let text = param_doc(None, f.docs.as_ref(), &a.name);
                let _ = writeln!(out, "| `{}` | {} | {} |", a.name, self.ty(page, &a.ty), cell(&text));
            }
            out.push('\n');
        }
    }

    /// A type as Markdown, each name linked where it can be.
    fn ty(&self, page: &str, ty: &Type) -> String {
        match ty {
            Type::Primitive { name } => format!("`{name}`"),
            Type::Unit => "`()`".to_string(),
            Type::Named { name, target } => match target.as_ref().and_then(|t| self.link(page, t)) {
                Some(url) => format!("[`{name}`]({url})"),
                None => format!("`{name}`"),
            },
            Type::Array { element, length } => match length {
                Some(n) => format!("{}`[{n}]`", self.ty(page, element)),
                None => format!("{}`[]`", self.ty(page, element)),
            },
            Type::Union { members } => {
                let members: Vec<String> = members.iter().map(|m| self.ty(page, m)).collect();
                format!("union({})", members.join(" "))
            }
        }
    }

    fn link(&self, page: &str, target: &UnitRef) -> Option<String> {
        if target.package == self.model.package.name {
            let to = page_of(&target.schema);
            let to = if to == page { String::new() } else { relative(page, &to) };
            return Some(format!("{to}#{}", anchor(&target.name)));
        }
        let base = self.links.get(&target.package)?;
        Some(format!(
            "{}/{}/#{}",
            base.trim_end_matches('/'),
            target.schema.replace("::", "/"),
            anchor(&target.name)
        ))
    }

    fn changelog(&self) -> String {
        let mut out = format!("# Changelog: {}\n\n", self.model.package.name);
        for log in &self.model.changelog {
            let _ = write!(out, "## {} to {}\n\n", log.from, log.to);
            if log.breaking.is_empty() && log.features.is_empty() && log.modifications.is_empty() {
                out.push_str("No changes.\n\n");
                continue;
            }
            for (title, changes) in
                [("Breaking", &log.breaking), ("Added", &log.features), ("Changed", &log.modifications)]
            {
                if changes.is_empty() {
                    continue;
                }
                let _ = write!(out, "### {title}\n\n");
                for change in changes {
                    let _ = writeln!(out, "- {}", change_line(change));
                }
                out.push('\n');
            }
        }
        finish(out)
    }
}

/// The docs of a field or argument: its own, else its parent's `@name:` line.
fn param_doc(own: Option<&Doc>, parent: Option<&Doc>, name: &str) -> String {
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

fn docs_of(unit: &Unit) -> Option<&Doc> {
    match unit {
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

/// The protocol's wire summary: the same rows as the language server's hover
/// (`protocol_summary.rs`) and `comline inspect`.
fn wire_summary(out: &mut String, p: &ProtocolSummary) {
    out.push_str("**On the wire**\n\n| | | from |\n|---|---|---|\n");
    for (key, choice) in [("format", &p.format), ("framing", &p.framing), ("handshake", &p.handshake)] {
        let mut value = format!("`{}`", choice.value);
        if !choice.params.is_empty() {
            let params: Vec<String> = choice.params.iter().map(|(k, v)| format!("{k} = {v}")).collect();
            let _ = write!(value, " ({})", params.join(", "));
        }
        let from = match choice.source.as_str() {
            "annotation" => format!("`@{key}`"),
            "package_default" if key == "handshake" => "config.idp `handshake`".to_string(),
            "package_default" => format!("config.idp `wire.{key}`"),
            "core_default" => "core's default".to_string(),
            "framing" => "JSON-RPC's format".to_string(),
            other => other.to_string(),
        };
        let _ = writeln!(out, "| {key} | {} | {from} |", cell(&value));
    }

    out.push_str("\n**Guarantees**\n\n| guarantee | | why | accepted |\n|---|---|---|---|\n");
    for row in &p.guarantees {
        let state = if row.held { "held" } else { "**given up**" };
        let accepted = match row.acceptance.state.as_str() {
            "unknown" => "by each consumer".to_string(),
            "accepted" => format!("by {}", row.acceptance.by.as_deref().unwrap_or("")),
            "not_accepted" => "**not accepted**".to_string(),
            _ => String::new(),
        };
        let guarantee = Guarantee::from_name(&row.guarantee).map(|g| g.describe()).unwrap_or(&row.guarantee);
        let _ = writeln!(out, "| {guarantee} | {state} | {} | {accepted} |", cell(&row.causes.join("; ")));
    }
    if let Some(reason) = p.guarantees.iter().find_map(|g| g.reason.as_deref()) {
        let _ = writeln!(out, "\n`@unverified`: \"{reason}\"");
    }
    out.push('\n');
}

/// One change, as a line: its kind in words, then its fields.
fn change_line(change: &serde_json::Value) -> String {
    let Some(object) = change.as_object() else { return format!("`{change}`") };
    let kind = object.get("kind").and_then(|k| k.as_str()).unwrap_or("change");
    let mut words = kind.replace('_', " ");
    if let Some(first) = words.get(..1) {
        words = first.to_uppercase() + &words[1..];
    }
    let details: Vec<String> = object
        .iter()
        .filter(|(k, _)| k.as_str() != "kind")
        .map(|(k, v)| {
            let v = match v {
                serde_json::Value::String(s) => format!("`{s}`"),
                other => format!("`{other}`"),
            };
            format!("{}: {v}", k.replace('_', " "))
        })
        .collect();
    if details.is_empty() {
        words
    } else {
        format!("{words}: {}", details.join(", "))
    }
}

fn file(path: &str, contents: String) -> GeneratedFile {
    GeneratedFile { path: PathBuf::from(path), contents }
}

/// One trailing newline.
fn finish(mut out: String) -> String {
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
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
