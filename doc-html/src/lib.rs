//! The official HTML documentation renderer (generation#97, after core#145).
//!
//! A package's documentation model (`comline_core::doc`, as JSON) in, a
//! self-contained static site out, with the same content as the Markdown
//! renderer:
//!
//! - `index.html`: the package, its `//!` header and its modules;
//! - one page per module (`api.html`, `types/user.html`) with its header and
//!   every unit in it: docs, fields, arguments, function signatures, and each
//!   protocol's wire summary (the rows the language server's hover shows);
//! - `changelog.html` when the model carries a changelog;
//! - `style.css`: the fixed theme, light and dark from `prefers-color-scheme`;
//! - `search.js`: the search index, inlined as data, and the search box's
//!   script. Inlined because a browser blocks `fetch` under `file://`.
//!
//! Nothing loads from the network, and every link is relative, so
//! `index.html` opens straight from disk or from any static host.
//!
//! **Several versions** (generation#101). With [`RenderOptions::version`]
//! set, the pages are one version's folder of a site: the header carries a
//! version picker and a link to the site's comparison page, both fed by
//! `../versions.js`, so a version's pages don't change when versions are
//! added and `comline doc` can keep them. [`RenderOptions::notes`] open every
//! page. The site's index render ([`RenderOptions::versions`]) writes the
//! root: `index.html` (pointing at the latest version), `versions.js` (the
//! version list, each version's pages and anchors, and the picker's script),
//! and `compare.html` with `compare.js`: choose two versions for what changed
//! between them, more for a grid of every unit and member per version. The
//! change labels come from each version's changelog, computed by core from
//! the version before it; the page only collects the steps between the two
//! chosen versions and never judges a change itself.
//!
//! **Links.** Each unit's section has an anchor, its name in lowercase
//! (`id="order"`), and a type links to its unit's anchor, on the same page or
//! another module's. A type from a dependency (or `std`) links to
//! `<base>/<module>/#<anchor>` when [`RenderOptions::links`] gives that
//! dependency a base URL; otherwise it's plain text.
//!
//! Doc text is Markdown, rendered with `pulldown-cmark`; raw HTML in it is
//! escaped, not passed through. Everything else is escaped.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use comline_codegen::doc_model::*;
use comline_codegen::protocol::{RenderOptions, RenderRequest, SiteVersion, VersionKind};
use comline_codegen::{GeneratedFile, Registry, Renderer};
use eyre::{eyre, Result};

/// The documentation model versions this renderer reads.
pub const MODEL_VERSIONS: &[u32] = &[1];

/// This renderer, as it registers.
pub const RENDERER: Renderer =
    Renderer { id: "html", model_versions: MODEL_VERSIONS, version: concat!(env!("CARGO_PKG_VERSION"), "+", env!("COMLINE_RENDERER_HASH")), sites: true, render };

const STYLE: &str = include_str!("../assets/style.css");
const SEARCH: &str = include_str!("../assets/search.js");
const PICKER: &str = include_str!("../assets/picker.js");
const COMPARE: &str = include_str!("../assets/compare.js");

/// Register the HTML renderer.
pub fn register(registry: &mut Registry) {
    registry.register_renderer(RENDERER);
}

/// [`RENDERER`]'s entry point.
pub fn render(req: &RenderRequest) -> Result<Vec<GeneratedFile>> {
    if !MODEL_VERSIONS.contains(&req.model_version) {
        return Err(eyre!("documentation model {} isn't one this renderer reads ({MODEL_VERSIONS:?})", req.model_version));
    }
    if !req.options.versions.is_empty() {
        return site_index(&req.options.versions);
    }
    render_json(&req.model, &req.options)
}

/// The site for the model `json`.
pub fn render_json(json: &str, options: &RenderOptions) -> Result<Vec<GeneratedFile>> {
    let model: DocModel = serde_json::from_str(json).map_err(|e| eyre!("the documentation model doesn't parse: {e}"))?;
    Ok(Site { model: &model, links: &options.links, version: options.version.as_deref(), notes: &options.notes }.all())
}

struct Site<'a> {
    model: &'a DocModel,
    links: &'a BTreeMap<String, String>,
    /// The version folder this site is, in a multi-version site.
    version: Option<&'a str>,
    notes: &'a [String],
}

fn page_of(module: &str) -> String {
    comline_codegen::doc_model::page_of(module, "html")
}

/// `text`, escaped for HTML text and attribute values.
fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn code(text: &str) -> String {
    format!("<code>{}</code>", esc(text))
}

/// Doc text (Markdown) as HTML. Raw HTML in it is shown as text.
fn markdown(text: &str) -> String {
    use pulldown_cmark::{Event, Options, Parser};
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let events = Parser::new_ext(text.trim(), options).map(|event| match event {
        Event::Html(html) | Event::InlineHtml(html) => Event::Text(html),
        other => other,
    });
    let mut out = String::new();
    pulldown_cmark::html::push_html(&mut out, events);
    out
}

/// Doc text kept to one line, for a table cell: inline Markdown only.
fn markdown_inline(text: &str) -> String {
    let html = markdown(text);
    let trimmed = html.trim_end();
    match trimmed.strip_prefix("<p>").and_then(|t| t.strip_suffix("</p>")) {
        Some(inner) if !inner.contains("<p>") => inner.to_string(),
        _ => trimmed.to_string(),
    }
}

fn annotation(a: &Annotation) -> String {
    match &a.value {
        Some(value) => code(&format!("@{} = {}", a.key, value)),
        None => code(&format!("@{}", a.key)),
    }
}

/// A table: a header row and body rows of ready HTML cells, in a wrapper
/// that scrolls sideways on its own, never the page.
fn table(out: &mut String, head: &[&str], rows: &[Vec<String>]) {
    out.push_str("<div class=\"table\"><table>\n<thead><tr>");
    for h in head {
        let _ = write!(out, "<th>{h}</th>");
    }
    out.push_str("</tr></thead>\n<tbody>\n");
    for row in rows {
        out.push_str("<tr>");
        for c in row {
            let _ = write!(out, "<td>{c}</td>");
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</tbody>\n</table></div>\n");
}

impl Site<'_> {
    fn all(&self) -> Vec<GeneratedFile> {
        let mut files = vec![file("index.html", self.index())];
        for module in &self.model.modules {
            files.push(file(&page_of(&module.path), self.module(module)));
        }
        if !self.model.changelog.is_empty() {
            files.push(file("changelog.html", self.changelog()));
        }
        files.push(file("style.css", STYLE.to_string()));
        files.push(file("search.js", self.search()));
        files
    }

    fn title(&self) -> String {
        match &self.model.package.version {
            Some(version) => format!("{} {version}", self.model.package.name),
            None => self.model.package.name.clone(),
        }
    }

    fn units(module: &Module) -> impl Iterator<Item = &Unit> {
        module.units.iter().filter(|u| u.name().is_some())
    }

    /// A whole page: `page` is its path from the root, `current` the module
    /// it shows, if any.
    fn page(&self, page: &str, title: &str, current: Option<&str>, body: &str) -> String {
        let root = relative(page, "x");
        let root = &root[..root.len() - 1];
        let mut out = String::new();
        let body_attrs = match self.version {
            Some(version) => format!(" data-page=\"{}\" data-version=\"{}\" data-root=\"{root}\"", esc(page), esc(version)),
            None => String::new(),
        };
        let picker = match self.version {
            Some(version) => format!(
                "<form class=\"picker\" id=\"version-form\">\n<label for=\"version-picker\">Version</label>\n\
                 <select id=\"version-picker\"><option value=\"{v}\" selected>{v}</option></select>\n\
                 <button type=\"submit\">Go</button>\n<a href=\"{root}../compare.html\">Compare</a>\n</form>\n",
                v = esc(version)
            ),
            None => String::new(),
        };
        let _ = write!(
            out,
            "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
             <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
             <title>{}</title>\n<link rel=\"stylesheet\" href=\"{root}style.css\">\n</head>\n<body{body_attrs}>\n\
             <header class=\"top\">\n<a class=\"brand\" href=\"{root}index.html\">{}</a>\n{picker}\
             <div class=\"search\" role=\"search\">\n<input id=\"search\" type=\"search\" placeholder=\"Search (press /)\" \
             aria-label=\"Search the documentation\" autocomplete=\"off\" spellcheck=\"false\">\n\
             <ul id=\"results\" class=\"results\" hidden></ul>\n</div>\n</header>\n<div class=\"layout\">\n",
            esc(title),
            esc(&self.title()),
        );
        self.sidebar(&mut out, page, current);
        let mut notes = String::new();
        for note in self.notes {
            let _ = writeln!(notes, "<p class=\"note\" role=\"note\">{}</p>", esc(note.trim()));
        }
        if self.version.is_some() && page == "index.html" {
            notes.push_str("<p class=\"note\" id=\"version-note\" role=\"status\" hidden></p>\n");
        }
        let versions = match self.version {
            Some(_) => format!("<script src=\"{root}../versions.js\"></script>\n"),
            None => String::new(),
        };
        let _ = write!(
            out,
            "<main>\n{notes}{body}</main>\n</div>\n{versions}<script src=\"{root}search.js\" data-root=\"{root}\"></script>\n</body>\n</html>\n"
        );
        out
    }

    fn sidebar(&self, out: &mut String, page: &str, current: Option<&str>) {
        out.push_str("<nav class=\"sidebar\" aria-label=\"Modules\">\n<ul>\n");
        let here = |to: &str| if to == page { " aria-current=\"page\"" } else { "" };
        let _ = writeln!(out, "<li><a href=\"{}\"{}>Overview</a></li>", relative(page, "index.html"), here("index.html"));
        for module in &self.model.modules {
            let to = page_of(&module.path);
            let href = relative(page, &to);
            let open = if current == Some(module.path.as_str()) { " open" } else { "" };
            let _ = write!(
                out,
                "<li><details{open}><summary><a href=\"{href}\"{}>{}</a></summary>\n<ul>\n",
                here(&to),
                code(&module.path)
            );
            for unit in Self::units(module) {
                let name = unit.name().unwrap_or_default();
                let _ = writeln!(
                    out,
                    "<li><a href=\"{href}#{}\"><span class=\"kind\">{}</span> {}</a></li>",
                    esc(&anchor(name)),
                    unit.kind(),
                    esc(name)
                );
            }
            out.push_str("</ul></details></li>\n");
        }
        if !self.model.changelog.is_empty() {
            let _ = writeln!(
                out,
                "<li><a href=\"{}\"{}>Changelog</a></li>",
                relative(page, "changelog.html"),
                here("changelog.html")
            );
        }
        out.push_str("</ul>\n</nav>\n");
    }

    fn index(&self) -> String {
        let mut body = format!("<h1>{}</h1>\n", esc(&self.title()));
        if let Some(docs) = &self.model.package.docs {
            body.push_str(&markdown(docs));
        }
        if !self.model.modules.is_empty() {
            body.push_str("<h2>Modules</h2>\n");
            let rows: Vec<Vec<String>> = self
                .model
                .modules
                .iter()
                .map(|m| {
                    let text = m.docs.as_deref().map(summary).unwrap_or("");
                    vec![format!("<a href=\"{}\">{}</a>", page_of(&m.path), code(&m.path)), markdown_inline(text)]
                })
                .collect();
            table(&mut body, &["Module", ""], &rows);
        }
        if !self.model.changelog.is_empty() {
            body.push_str("<p>See the <a href=\"changelog.html\">changelog</a>.</p>\n");
        }
        self.page("index.html", &self.title(), None, &body)
    }

    fn module(&self, module: &Module) -> String {
        let page = page_of(&module.path);
        let mut body = format!("<h1>{}</h1>\n", code(&module.path));
        if let Some(docs) = &module.docs {
            body.push_str(&markdown(docs));
        }
        let rows: Vec<Vec<String>> = Self::units(module)
            .map(|unit| {
                let name = unit.name().unwrap_or_default();
                let text = unit.docs().map(|d| summary(&d.text)).unwrap_or("");
                vec![
                    format!("{} <a href=\"#{}\">{}</a>", unit.kind(), esc(&anchor(name)), code(name)),
                    markdown_inline(text),
                ]
            })
            .collect();
        if !rows.is_empty() {
            table(&mut body, &["", ""], &rows);
        }
        for unit in Self::units(module) {
            self.unit(&mut body, &page, unit);
        }
        self.page(&page, &format!("{} · {}", module.path, self.title()), Some(&module.path), &body)
    }

    fn unit(&self, out: &mut String, page: &str, unit: &Unit) {
        let name = unit.name().unwrap_or_default();
        let id = esc(&anchor(name));
        let _ = write!(
            out,
            "<section class=\"unit\" id=\"{id}\">\n<h2><a class=\"self\" href=\"#{id}\">{} {}</a></h2>\n",
            unit.kind(),
            code(name)
        );
        let docs = unit.docs();
        if let Some(docs) = docs {
            if !docs.text.trim().is_empty() {
                out.push_str(&markdown(&docs.text));
            }
        }
        let annotations = unit.annotations();
        if !annotations.is_empty() {
            let list: Vec<String> = annotations.iter().map(annotation).collect();
            let _ = writeln!(out, "<p>Annotations: {}</p>", list.join(", "));
        }
        match unit {
            Unit::Struct { fields, .. } => self.fields(out, page, "Field", fields, docs),
            Unit::Enum { variants, .. } => {
                out.push_str("<p>Variants:</p>\n<ul>\n");
                for variant in variants {
                    let _ = writeln!(out, "<li>{}</li>", code(variant));
                }
                out.push_str("</ul>\n");
            }
            Unit::Error { ordinal, message, fields, .. } => {
                let _ = writeln!(out, "<p>Ordinal {}. Message: {}</p>", code(&ordinal.to_string()), code(message));
                self.fields(out, page, "Field", fields, docs);
            }
            Unit::Validator { targets, params, asserts, .. } => {
                let targets: Vec<String> = targets.iter().map(|t| self.ty(page, t)).collect();
                let _ = writeln!(out, "<p>Applies to: {}</p>", targets.join(", "));
                self.fields(out, page, "Parameter", params, docs);
                if !asserts.is_empty() {
                    let rows: Vec<Vec<String>> =
                        asserts.iter().map(|a| vec![code(&a.condition), esc(&a.message)]).collect();
                    table(out, &["Assert", "Message"], &rows);
                }
            }
            Unit::Constant { ty, value, .. } => {
                let _ = write!(out, "<p>Type: {}", self.ty(page, ty));
                if let Some(value) = value {
                    let _ = write!(out, ". Value: {}", code(value));
                }
                out.push_str("</p>\n");
            }
            Unit::TypeAlias { target, .. } => {
                let _ = writeln!(out, "<p>Alias of {}</p>", self.ty(page, target));
            }
            Unit::Settings { values, .. } => {
                if !values.is_empty() {
                    let rows: Vec<Vec<String>> = values.iter().map(|v| vec![code(&v.key), code(&v.value)]).collect();
                    table(out, &["Key", "Value"], &rows);
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
        out.push_str("</section>\n");
    }

    fn fields(&self, out: &mut String, page: &str, what: &str, fields: &[Field], parent: Option<&Doc>) {
        if fields.is_empty() {
            return;
        }
        let rows: Vec<Vec<String>> = fields
            .iter()
            .map(|f| {
                let mut ty = self.ty(page, &f.ty);
                if f.optional {
                    ty.push_str(" (optional)");
                }
                if let Some(default) = &f.default {
                    let _ = write!(ty, " = {}", code(default));
                }
                let mut text = markdown_inline(&param_doc(f.docs.as_ref(), parent, &f.name));
                for a in &f.annotations {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(&annotation(a));
                }
                vec![code(&f.name), ty, text]
            })
            .collect();
        table(out, &[what, "Type", ""], &rows);
    }

    fn function(&self, out: &mut String, page: &str, protocol: &str, f: &Function) {
        let id = esc(&format!("{}-{}", anchor(protocol), anchor(&f.name)));
        let _ = write!(
            out,
            "<div class=\"function\" id=\"{id}\">\n<h3><a class=\"self\" href=\"#{id}\">{}</a></h3>\n",
            code(&f.name)
        );
        let args: Vec<String> =
            f.arguments.iter().map(|a| format!("{}: {}", esc(&a.name), self.ty(page, &a.ty))).collect();
        let mut signature = format!("{}({})", code(&f.name), args.join(", "));
        if let Some(returns) = &f.returns {
            let _ = write!(signature, " -&gt; {}", self.ty(page, returns));
        }
        if !f.throws.is_empty() {
            let throws: Vec<String> = f.throws.iter().map(|t| self.ty(page, t)).collect();
            let _ = write!(signature, " ! {}", throws.join(", "));
        }
        let _ = writeln!(out, "<p class=\"signature\">{signature}</p>");
        if let Some(docs) = &f.docs {
            if !docs.text.trim().is_empty() {
                out.push_str(&markdown(&docs.text));
            }
        }
        if !f.annotations.is_empty() {
            let list: Vec<String> = f.annotations.iter().map(annotation).collect();
            let _ = writeln!(out, "<p>Annotations: {}</p>", list.join(", "));
        }
        if !f.arguments.is_empty() {
            let rows: Vec<Vec<String>> = f
                .arguments
                .iter()
                .map(|a| {
                    vec![
                        code(&a.name),
                        self.ty(page, &a.ty),
                        markdown_inline(&param_doc(None, f.docs.as_ref(), &a.name)),
                    ]
                })
                .collect();
            table(out, &["Argument", "Type", ""], &rows);
        }
        out.push_str("</div>\n");
    }

    /// A type as HTML, each name linked where it can be.
    fn ty(&self, page: &str, ty: &Type) -> String {
        match ty {
            Type::Primitive { name } => code(name),
            Type::Unit => code("()"),
            Type::Named { name, target } => match target.as_ref().and_then(|t| self.link(page, t)) {
                Some(url) => format!("<a href=\"{}\">{}</a>", esc(&url), code(name)),
                None => code(name),
            },
            Type::Array { element, length } => match length {
                Some(n) => format!("{}{}", self.ty(page, element), code(&format!("[{n}]"))),
                None => format!("{}{}", self.ty(page, element), code("[]")),
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
        let mut body = format!("<h1>Changelog: {}</h1>\n", esc(&self.model.package.name));
        for log in &self.model.changelog {
            let _ = writeln!(body, "<h2>{} to {}</h2>", esc(&log.from), esc(&log.to));
            if log.breaking.is_empty() && log.features.is_empty() && log.modifications.is_empty() {
                body.push_str("<p>No changes.</p>\n");
                continue;
            }
            for (title, changes) in
                [("Breaking", &log.breaking), ("Added", &log.features), ("Changed", &log.modifications)]
            {
                if changes.is_empty() {
                    continue;
                }
                let _ = write!(body, "<h3>{title}</h3>\n<ul>\n");
                for change in changes {
                    let _ = writeln!(body, "<li>{}</li>", change_line(change));
                }
                body.push_str("</ul>\n");
            }
        }
        self.page("changelog.html", &format!("Changelog · {}", self.title()), None, &body)
    }

    /// `search.js`: the index as data, then the search box's script.
    fn search(&self) -> String {
        let mut entries = vec![];
        for module in &self.model.modules {
            let page = page_of(&module.path);
            for unit in Self::units(module) {
                let name = unit.name().unwrap_or_default();
                entries.push(serde_json::json!({
                    "n": name,
                    "k": unit.kind(),
                    "m": module.path,
                    "s": unit.docs().map(|d| summary(&d.text)).unwrap_or(""),
                    "u": format!("{page}#{}", anchor(name)),
                }));
            }
        }
        let mut out = String::from("/* The search index: generated by comline-doc-html. */\nvar INDEX = [\n");
        for (i, entry) in entries.iter().enumerate() {
            let sep = if i + 1 == entries.len() { "" } else { "," };
            let _ = writeln!(out, "  {entry}{sep}");
        }
        out.push_str("];\n");
        out.push_str(SEARCH);
        out
    }
}

/// The latest version, which a site's `index.html` points at: the newest
/// release, else the newest dev build, else the unreleased state.
pub fn latest(versions: &[SiteVersion]) -> Option<&SiteVersion> {
    [VersionKind::Release, VersionKind::Dev, VersionKind::Unreleased]
        .into_iter()
        .find_map(|kind| versions.iter().rev().find(|v| v.kind == kind))
}

/// A multi-version site's root: `index.html`, `versions.js`, `compare.html`,
/// `compare.js` and the shared `style.css`.
pub fn site_index(versions: &[SiteVersion]) -> Result<Vec<GeneratedFile>> {
    let mut models = vec![];
    for version in versions {
        let model: DocModel = serde_json::from_str(&version.model)
            .map_err(|e| eyre!("version `{}`'s model doesn't parse: {e}", version.id))?;
        models.push(model);
    }
    let name = models.last().map(|m| m.package.name.clone()).unwrap_or_default();
    let latest = latest(versions).map(|v| v.id.clone()).unwrap_or_default();

    // `versions.js`: each version's pages and the anchors on them, so the
    // picker knows whether the page it's on exists in another version.
    let mut list = vec![];
    for (version, model) in versions.iter().zip(&models) {
        let mut pages = serde_json::Map::new();
        pages.insert("index.html".into(), serde_json::json!([]));
        for module in &model.modules {
            let mut anchors = vec![];
            for unit in Site::units(module) {
                let name = unit.name().unwrap_or_default();
                anchors.push(anchor(name));
                if let Unit::Protocol { functions, .. } = unit {
                    for f in functions {
                        anchors.push(format!("{}-{}", anchor(name), anchor(&f.name)));
                    }
                }
            }
            pages.insert(page_of(&module.path), serde_json::json!(anchors));
        }
        if !model.changelog.is_empty() {
            pages.insert("changelog.html".into(), serde_json::json!([]));
        }
        list.push(serde_json::json!({
            "id": version.id, "release": version.release, "kind": version.kind.as_str(), "pages": pages,
        }));
    }
    let mut versions_js = String::from("/* The site's versions: generated by comline-doc-html. */\nvar VERSIONS = [\n");
    for (i, entry) in list.iter().enumerate() {
        let sep = if i + 1 == list.len() { "" } else { "," };
        let _ = writeln!(versions_js, "  {entry}{sep}");
    }
    let _ = writeln!(versions_js, "];\nvar LATEST = {};", serde_json::json!(latest));
    versions_js.push_str(PICKER);

    // `compare.js`: per version, its structure and the changes since the
    // version before it, labelled by the changelog list core put them in.
    let mut compare = vec![];
    for (version, model) in versions.iter().zip(&models) {
        let mut changes = vec![];
        for log in &model.changelog {
            for (severity, list) in SEVERITIES.iter().zip([&log.breaking, &log.features, &log.modifications]) {
                for change in list {
                    changes.push(serde_json::json!({
                        "unit": change_unit(change), "severity": severity, "text": change_text(change),
                    }));
                }
            }
        }
        compare.push(serde_json::json!({
            "id": version.id, "release": version.release, "kind": version.kind.as_str(),
            "rows": structure(model), "changes": changes,
        }));
    }
    let mut compare_js = String::from("/* Each version's structure and changes: generated by comline-doc-html. */\nvar COMPARE = [\n");
    for (i, entry) in compare.iter().enumerate() {
        let sep = if i + 1 == compare.len() { "" } else { "," };
        let _ = writeln!(compare_js, "  {entry}{sep}");
    }
    compare_js.push_str("];\n");
    compare_js.push_str(COMPARE);

    let head = |title: &str, extra: &str| {
        format!(
            "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
             <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n{extra}\
             <title>{}</title>\n<link rel=\"stylesheet\" href=\"style.css\">\n</head>\n<body>\n\
             <header class=\"top\">\n<a class=\"brand\" href=\"index.html\">{}</a>\n\
             <nav class=\"site-links\" aria-label=\"Site\"><a href=\"compare.html\">Compare versions</a></nav>\n</header>\n",
            esc(title),
            esc(&name)
        )
    };

    let mut index = head(
        &format!("{name}: versions"),
        &if latest.is_empty() {
            String::new()
        } else {
            format!("<meta http-equiv=\"refresh\" content=\"0; url={}/index.html\">\n", esc(&latest))
        },
    );
    index.push_str("<main class=\"wide\">\n");
    let _ = writeln!(index, "<h1>{}: versions</h1>", esc(&name));
    if !latest.is_empty() {
        let _ = writeln!(index, "<p>The latest is <a href=\"{0}/index.html\">{0}</a>.</p>", esc(&latest));
    }
    let mut groups: Vec<(&str, Vec<&SiteVersion>)> = vec![];
    for version in versions.iter().rev() {
        match groups.iter_mut().find(|(release, _)| *release == version.release) {
            Some((_, members)) => members.push(version),
            None => groups.push((&version.release, vec![version])),
        }
    }
    for (release, members) in groups {
        let _ = write!(index, "<h2>{}</h2>\n<ul>\n", esc(release));
        for version in members {
            let what = match version.kind {
                VersionKind::Release => "release",
                VersionKind::Dev => "dev build",
                VersionKind::Unreleased => "the working tree&#39;s changes since the last build",
            };
            let _ = writeln!(index, "<li><a href=\"{0}/index.html\">{0}</a>: {what}</li>", esc(&version.id));
        }
        index.push_str("</ul>\n");
    }
    index.push_str("</main>\n</body>\n</html>\n");

    let mut compare_html = head(&format!("Compare versions · {name}"), "");
    compare_html.push_str(
        "<main class=\"wide\">\n<h1>Compare versions</h1>\n\
         <p>Choose two versions to see what changed between them, or more for every unit and member side by side. \
         The labels are the changelogs core computed between each version and the one before it.</p>\n\
         <fieldset class=\"choose\">\n<legend>Versions</legend>\n<div id=\"choices\"></div>\n</fieldset>\n\
         <div id=\"comparison\" aria-live=\"polite\"><p>This page needs JavaScript.</p></div>\n</main>\n\
         <script src=\"compare.js\"></script>\n</body>\n</html>\n",
    );

    Ok(vec![
        file("index.html", index),
        file("compare.html", compare_html),
        file("versions.js", versions_js),
        file("compare.js", compare_js),
        file("style.css", STYLE.to_string()),
    ])
}

/// The protocol's wire summary: the same rows as the Markdown renderer, the
/// language server's hover (`protocol_summary.rs`) and `comline inspect`.
fn wire_summary(out: &mut String, p: &ProtocolSummary) {
    out.push_str("<h3>On the wire</h3>\n");
    let rows: Vec<Vec<String>> = p
        .choices()
        .iter()
        .map(|row| {
            let mut value = code(row.value);
            if !row.params.is_empty() {
                let params: Vec<String> = row.params.iter().map(|(k, v)| format!("{k} = {v}")).collect();
                let _ = write!(value, " ({})", esc(&params.join(", ")));
            }
            let from = match &row.source {
                ChoiceSource::Annotation(key) => code(&format!("@{key}")),
                ChoiceSource::PackageDefault(key) => format!("config.idp {}", code(key)),
                ChoiceSource::CoreDefault => "core&#39;s default".to_string(),
                ChoiceSource::Framing => "JSON-RPC&#39;s format".to_string(),
                ChoiceSource::Other(other) => esc(other),
            };
            vec![row.key.to_string(), value, from]
        })
        .collect();
    table(out, &["", "", "from"], &rows);

    out.push_str("<h3>Guarantees</h3>\n");
    let rows: Vec<Vec<String>> = p
        .guarantee_rows()
        .iter()
        .map(|row| {
            let state = if row.held { "held" } else { "<span class=\"given-up\">given up</span>" };
            let accepted = match row.accepted {
                Accepted::ByEachConsumer => "by each consumer".to_string(),
                Accepted::By(by) => format!("by {}", esc(by)),
                Accepted::NotAccepted => "<span class=\"not-accepted\">not accepted</span>".to_string(),
                Accepted::None => String::new(),
            };
            vec![esc(row.guarantee), state.to_string(), esc(&row.causes), accepted]
        })
        .collect();
    table(out, &["guarantee", "", "why", "accepted"], &rows);
    if let Some(reason) = p.unverified_reason() {
        let _ = writeln!(out, "<p>{}: &quot;{}&quot;</p>", code("@unverified"), esc(reason));
    }
}

/// One change, as a line: its kind in words, then its fields.
fn change_line(change: &serde_json::Value) -> String {
    let Some((words, details)) = change_parts(change) else { return code(&change.to_string()) };
    let details: Vec<String> = details.iter().map(|(k, v)| format!("{}: {}", esc(k), code(v))).collect();
    if details.is_empty() {
        esc(&words)
    } else {
        format!("{}: {}", esc(&words), details.join(", "))
    }
}

fn file(path: &str, contents: String) -> GeneratedFile {
    GeneratedFile { path: PathBuf::from(path), contents }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_and_renders_markdown() {
        assert_eq!(esc("<a href=\"x\">&'"), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
        assert_eq!(markdown("It sells **things**."), "<p>It sells <strong>things</strong>.</p>\n");
        assert_eq!(markdown("<script>x</script>"), "&lt;script&gt;x&lt;/script&gt;");
        assert_eq!(markdown_inline("A `b`."), "A <code>b</code>.");
    }
}
