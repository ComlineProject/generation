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
//! **Several versions.** `comline doc` renders each version into its own
//! folder ([`RenderOptions::version`]), then asks for the site's index
//! ([`RenderOptions::versions`]): an `index.md` listing every version, grouped
//! by release, newest first. Switching versions on a published site is left
//! to site tooling (mike, for mkdocs). A version's
//! [`RenderOptions::notes`] (what an old version's documentation lacks) open
//! its index and module pages as a quote.
//!
//! **Links.** Each unit has an anchor, its name in lowercase
//! (`<a id="order"></a>`), and a type links to its unit's anchor, on the same
//! page or another module's. A type from a dependency (or `std`) links to
//! `<base>/<module>/#<anchor>` when [`RenderOptions::links`] gives that
//! dependency a base URL, the directory-URL form mkdocs and zensical publish
//! pages at; otherwise it's plain text.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use comline_codegen::protocol::{RenderOptions, RenderRequest, SiteVersion, VersionKind};
use comline_codegen::{GeneratedFile, Registry, Renderer};
use eyre::{eyre, Result};

use comline_codegen::doc_model::*;

/// The documentation model versions this renderer reads.
pub const MODEL_VERSIONS: &[u32] = &[1];

/// This renderer, as it registers.
pub const RENDERER: Renderer =
    Renderer { id: "markdown", model_versions: MODEL_VERSIONS, version: concat!(env!("CARGO_PKG_VERSION"), "+", env!("COMLINE_RENDERER_HASH")), sites: true, render };

/// Register the Markdown renderer.
pub fn register(registry: &mut Registry) {
    registry.register_renderer(RENDERER);
}

/// [`RENDERER`]'s entry point.
pub fn render(req: &RenderRequest) -> Result<Vec<GeneratedFile>> {
    if !MODEL_VERSIONS.contains(&req.model_version) {
        return Err(eyre!("documentation model {} isn't one this renderer reads ({MODEL_VERSIONS:?})", req.model_version));
    }
    if !req.options.versions.is_empty() {
        return Ok(vec![file("index.md", site_index(&req.options.versions)?)]);
    }
    render_json(&req.model, &req.options)
}

/// A multi-version site's `index.md`: every version, newest first, dev
/// builds under the release they lead to.
fn site_index(versions: &[SiteVersion]) -> Result<String> {
    let mut name = String::new();
    let mut groups: Vec<(&str, Vec<&SiteVersion>)> = vec![];
    for version in versions.iter().rev() {
        if name.is_empty() {
            let model: DocModel =
                serde_json::from_str(&version.model).map_err(|e| eyre!("version `{}`'s model doesn't parse: {e}", version.id))?;
            name = model.package.name;
        }
        match groups.iter_mut().find(|(release, _)| *release == version.release) {
            Some((_, members)) => members.push(version),
            None => groups.push((&version.release, vec![version])),
        }
    }
    let latest = latest(versions).map(|v| v.id.as_str()).unwrap_or("");
    let mut out = format!("# {name}: versions\n\n");
    if !latest.is_empty() {
        out.push_str(&format!("The latest is [{latest}]({latest}/index.md).\n\n"));
    }
    for (release, members) in groups {
        out.push_str(&format!("## {release}\n\n"));
        for version in members {
            let what = match version.kind {
                VersionKind::Release => "release",
                VersionKind::Dev => "dev build",
                VersionKind::Unreleased => "the working tree's changes since the last build",
            };
            out.push_str(&format!("- [{}]({}/index.md): {what}\n", version.id, version.id));
        }
        out.push('\n');
    }
    Ok(finish(out))
}

/// The version a site's index points at: the newest release, else the
/// newest dev build, else the unreleased state.
pub fn latest(versions: &[SiteVersion]) -> Option<&SiteVersion> {
    [VersionKind::Release, VersionKind::Dev, VersionKind::Unreleased]
        .into_iter()
        .find_map(|kind| versions.iter().rev().find(|v| v.kind == kind))
}

/// The pages for the model `json`.
pub fn render_json(json: &str, options: &RenderOptions) -> Result<Vec<GeneratedFile>> {
    let model: DocModel = serde_json::from_str(json).map_err(|e| eyre!("the documentation model doesn't parse: {e}"))?;
    Ok(Pages { model: &model, links: &options.links, notes: &options.notes }.all())
}

struct Pages<'a> {
    model: &'a DocModel,
    links: &'a BTreeMap<String, String>,
    notes: &'a [String],
}

/// A module's page, relative to the output root: `types::user` is
/// `types/user.md`.
fn page_of(module: &str) -> String {
    comline_codegen::doc_model::page_of(module, "md")
}

/// Text for one table cell: no `|` or line break of its own.
fn cell(text: &str) -> String {
    text.trim().replace('|', "\\|").replace('\n', "<br>")
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

    /// This version's notes, as a quote; empty without any.
    fn notes(&self) -> String {
        let mut out = String::new();
        for note in self.notes {
            out.push_str(&format!("> **Note:** {}\n>\n", note.trim()));
        }
        if !out.is_empty() {
            out.truncate(out.len() - 2);
            out.push('\n');
        }
        out
    }

    fn title(&self) -> String {
        match &self.model.package.version {
            Some(version) => format!("{} {version}", self.model.package.name),
            None => self.model.package.name.clone(),
        }
    }

    fn index(&self) -> String {
        let mut out = format!("# {}\n\n", self.title());
        out.push_str(&self.notes());
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
        out.push_str(&self.notes());
        if let Some(docs) = &module.docs {
            out.push_str(&format!("{}\n\n", docs.trim()));
        }
        let units: Vec<&Unit> = module.units.iter().filter(|u| u.name().is_some()).collect();
        if !units.is_empty() {
            out.push_str("| | |\n|---|---|\n");
            for unit in &units {
                let name = unit.name().unwrap_or_default();
                let text = unit.docs().map(|d| summary(&d.text)).unwrap_or("");
                out.push_str(&format!("| {} [`{name}`](#{}) | {} |\n", unit.kind(), anchor(name), cell(text)));
            }
            out.push('\n');
        }
        for unit in units {
            self.unit(&mut out, &page, unit);
        }
        finish(out)
    }

    fn unit(&self, out: &mut String, page: &str, unit: &Unit) {
        let name = unit.name().unwrap_or_default();
        let _ = write!(out, "<a id=\"{}\"></a>\n\n## {} `{name}`\n\n", anchor(name), unit.kind());
        let docs = unit.docs();
        if let Some(docs) = docs {
            if !docs.text.trim().is_empty() {
                let _ = write!(out, "{}\n\n", docs.text.trim());
            }
        }
        let annotations = unit.annotations();
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
        let mut out = String::from("# Changelog\n\n");
        for log in &self.model.changelog {
            let _ = write!(out, "## {} to {}\n\n", log.from, log.to);
            if log.breaking.is_empty() && log.features.is_empty() && log.modifications.is_empty() {
                out.push_str("No changes.\n\n");
                continue;
            }
            let entries = entries(log);
            for (title, severity) in [("Breaking", "major"), ("Added", "minor"), ("Changed", "patch")] {
                let list: Vec<&Entry> = entries.iter().filter(|e| e.severity == severity).collect();
                if list.is_empty() {
                    continue;
                }
                let _ = write!(out, "### {title}\n\n");
                for e in list {
                    entry(&mut out, e, unit_target(self.model, e, "md").as_deref());
                }
            }
        }
        finish(out)
    }
}

/// The protocol's wire summary: the same rows as the language server's hover
/// (`protocol_summary.rs`) and `comline inspect`.
fn wire_summary(out: &mut String, p: &ProtocolSummary) {
    out.push_str("**On the wire**\n\n| | | from |\n|---|---|---|\n");
    for row in p.choices() {
        let mut value = format!("`{}`", row.value);
        if !row.params.is_empty() {
            let params: Vec<String> = row.params.iter().map(|(k, v)| format!("{k} = {v}")).collect();
            let _ = write!(value, " ({})", params.join(", "));
        }
        let from = match &row.source {
            ChoiceSource::Annotation(key) => format!("`@{key}`"),
            ChoiceSource::PackageDefault(key) => format!("config.idp `{key}`"),
            ChoiceSource::CoreDefault => "core's default".to_string(),
            ChoiceSource::Framing => "JSON-RPC's format".to_string(),
            ChoiceSource::Other(other) => other.clone(),
        };
        let _ = writeln!(out, "| {} | {} | {from} |", row.key, cell(&value));
    }

    out.push_str("\n**Guarantees**\n\n| guarantee | | why | accepted |\n|---|---|---|---|\n");
    for row in p.guarantee_rows() {
        let state = if row.held { "held" } else { "**given up**" };
        let accepted = match row.accepted {
            Accepted::ByEachConsumer => "by each consumer".to_string(),
            Accepted::By(by) => format!("by {by}"),
            Accepted::NotAccepted => "**not accepted**".to_string(),
            Accepted::None => String::new(),
        };
        let _ = writeln!(out, "| {} | {state} | {} | {accepted} |", row.guarantee, cell(&row.causes));
    }
    if let Some(reason) = p.unverified_reason() {
        let _ = writeln!(out, "\n`@unverified`: \"{reason}\"");
    }
    out.push('\n');
}

/// One changelog entry: a heading line with its severity and title (the
/// subject linked to `href`), then its old and new IDL lines as a `diff`
/// block, or a sentence.
fn entry(out: &mut String, e: &Entry, href: Option<&str>) {
    let _ = write!(out, "#### {} · ", e.severity.to_uppercase());
    if e.subject.is_empty() {
        out.push_str(&e.title());
    } else {
        match href {
            Some(href) => {
                let _ = write!(out, "[`{}`]({href})", e.subject);
            }
            None => {
                let _ = write!(out, "`{}`", e.subject);
            }
        }
        let _ = write!(out, ": {}", e.what);
    }
    out.push_str("\n\n");
    if e.old.is_some() || e.new.is_some() {
        out.push_str("```diff\n");
        if let Some(old) = &e.old {
            let _ = writeln!(out, "-{old}");
        }
        if let Some(new) = &e.new {
            let _ = writeln!(out, "+{new}");
        }
        out.push_str("```\n\n");
    } else if let Some(sentence) = &e.sentence {
        let _ = write!(out, "{sentence}\n\n");
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
