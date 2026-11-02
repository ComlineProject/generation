//! Golden tests: the Markdown renderer's `tests/golden/model.json` (a
//! documentation model from core's `doc::model`, with a changelog) renders
//! to `tests/golden/out/`, a static site.
//! `COMLINE_BLESS=1 cargo test -p comline-doc-html` rewrites the
//! expected pages.

use std::collections::BTreeMap;
use std::path::Path;

use comline_codegen::protocol::{self, RenderOptions, RenderRequest, Request, Response};

fn options() -> RenderOptions {
    RenderOptions {
        links: BTreeMap::from([("std".to_string(), "https://comlineproject.github.io/docs/std/".to_string())]),
        ..Default::default()
    }
}

fn model() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../doc-markdown/tests/golden/model.json")).unwrap()
}

#[test]
fn renders_the_golden_pages() {
    let files = comline_doc_html::render_json(&model(), &options()).unwrap();
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/out");
    let bless = std::env::var_os("COMLINE_BLESS").is_some();
    if bless {
        let _ = std::fs::remove_dir_all(&out);
    }
    let mut paths = vec![];
    for file in &files {
        let path = out.join(&file.path);
        paths.push(file.path.to_string_lossy().replace('\\', "/"));
        if bless {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &file.contents).unwrap();
        } else {
            let expected = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("no golden {}; run with COMLINE_BLESS=1", path.display()));
            assert_eq!(file.contents, expected, "{} drifted; run with COMLINE_BLESS=1", path.display());
        }
    }
    assert_eq!(paths, ["index.html", "api.html", "types.html", "deps/localdeps/greeting.html", "changelog.html", "style.css", "search.js"]);
}

#[test]
fn dependency_types_link_to_their_docs() {
    let files = comline_doc_html::render_json(&model(), &RenderOptions::default()).unwrap();
    let get = |p: &str| &files.iter().find(|f| f.path.to_str() == Some(p)).unwrap().contents;
    let api = get("api.html");
    assert!(api.contains("<a href=\"https://comlineproject.github.io/docs/std/http/#httpmethod\"><code>HttpMethod</code></a>"), "{api}");
    assert!(api.contains("<a href=\"deps/localdeps/greeting.html#greeting\"><code>Greeting</code></a>"), "{api}");
    assert!(api.contains("<a href=\"types.html#customer\"><code>Customer</code></a>"));
    // The sidebar: own modules, then a collapsed Dependencies section.
    let sidebar = api.split("<nav class=\"sidebar\"").nth(1).unwrap().split("</nav>").next().unwrap();
    let (own, deps) = sidebar.split_once("<li class=\"dependencies\"><details><summary>Dependencies</summary>").unwrap();
    assert!(own.contains("<code>api</code>") && own.contains("<code>types</code>"));
    assert!(!own.contains("greeting") && !own.contains("http"));
    assert!(deps.contains("<code>localdeps</code>") && deps.contains("<code>std</code>"));
    // On a dependency's page the section is open.
    let greeting = get("deps/localdeps/greeting.html");
    assert!(greeting.contains("<li class=\"dependencies\"><details open>"), "{greeting}");
    assert!(greeting.contains("<a href=\"../../style.css\">") || greeting.contains("href=\"../../style.css\""));
    // Only the package's name in the brand.
    assert!(api.contains("<a class=\"brand\" href=\"index.html\">shop</a>"), "{api}");
}

/// A dev build's page: its step, and a link to everything since the last
/// release; a release's: its changes since the previous release only.
#[test]
fn changelog_spans() {
    let mut model: serde_json::Value = serde_json::from_str(&model()).unwrap();
    let step = model["changelog"][0].clone();
    let mut since = step.clone();
    since["from"] = "0.1.0".into();
    since["span"] = "since_release".into();
    model["changelog"] = serde_json::json!([step, since]);
    let files = comline_doc_html::render_json(&model.to_string(), &RenderOptions::default()).unwrap();
    let log = &files.iter().find(|f| f.path.to_str() == Some("changelog.html")).unwrap().contents;
    assert!(log.contains("Everything since the last release: <a href=\"#since-0.1.0\">0.1.0 to 0.2.0</a>."), "{log}");
    assert!(log.contains("<details class=\"since-release\" id=\"since-0.1.0\">"), "{log}");
}

#[test]
fn loads_nothing_from_the_network() {
    let files = comline_doc_html::render_json(&model(), &options()).unwrap();
    for file in &files {
        for needle in ["src=\"http", "src=\"//", "href=\"//", "@import", "fetch(", "XMLHttpRequest"] {
            assert!(!file.contents.contains(needle), "{} has {needle}", file.path.display());
        }
        if file.path.extension().is_some_and(|e| e == "html") && file.path.parent() == Some(Path::new("")) {
            assert!(file.contents.contains("<link rel=\"stylesheet\" href=\"style.css\">"));
            assert!(file.contents.contains("<script src=\"search.js\" data-root=\"\"></script>"));
        }
    }
    let search = files.iter().find(|f| f.path.to_str() == Some("search.js")).unwrap();
    assert!(search.contents.contains(r#"{"k":"struct","m":"api","n":"Order","s":"One order.","u":"api.html#order"}"#), "{}", search.contents);
}

#[test]
fn renders_through_the_protocol() {
    let mut registry = comline_codegen::Registry::new();
    comline_doc_html::register(&mut registry);
    let Response::Describe(d) = protocol::respond(&registry, &protocol::encode(&Request::describe()).unwrap()) else {
        panic!()
    };
    assert_eq!(d.renderers.len(), 1);
    assert_eq!(d.renderers[0].id, "html");
    let request = Request::render(RenderRequest {
        renderer: "html".into(),
        model_version: 1,
        model: model(),
        options: options(),
    });
    let Response::Render(out) = protocol::respond(&registry, &protocol::encode(&request).unwrap()) else { panic!() };
    assert_eq!(out.files.len(), 7);
}

/// The 3-version fixture (`../doc-markdown/tests/golden/versions/`, written by
/// its `make.py`): two releases, a dev build and an unreleased change, each
/// rendered into its own folder, then the site's index, as `comline doc`
/// asks for them. The goldens are `tests/golden/versions-out/`.
pub fn render_versions() -> Vec<(String, String)> {
    use comline_codegen::protocol::{SiteVersion, VersionKind};
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../doc-markdown/tests/golden/versions");
    let versions = [
        ("0.1.0", "0.1.0", VersionKind::Release),
        ("0.2.0-dev.1", "0.2.0", VersionKind::Dev),
        ("0.2.0", "0.2.0", VersionKind::Release),
        ("unreleased", "unreleased", VersionKind::Unreleased),
    ];
    let wire = "Built versions don't record their wire tables yet (core#153).".to_string();
    let mut out = vec![];
    let mut site = vec![];
    for (id, release, kind) in versions {
        let model = std::fs::read_to_string(dir.join(format!("{id}.json"))).unwrap();
        let notes = match id {
            "0.1.0" => vec!["Built before headers were frozen (core#152): it has no package or module headers.".to_string(), wire.clone()],
            "unreleased" => vec![],
            _ => vec![wire.clone()],
        };
        let options = RenderOptions { version: Some(id.to_string()), notes, ..options() };
        for file in comline_doc_html::render_json(&model, &options).unwrap() {
            out.push((format!("{id}/{}", file.path.to_string_lossy().replace('\\', "/")), file.contents));
        }
        site.push(SiteVersion { id: id.into(), release: release.into(), kind, model });
    }
    let request = RenderRequest {
        renderer: comline_doc_html::RENDERER.id.into(),
        model_version: 1,
        model: site.last().unwrap().model.clone(),
        options: RenderOptions { versions: site, ..options() },
    };
    for file in comline_doc_html::render(&request).unwrap() {
        out.push((file.path.to_string_lossy().replace('\\', "/"), file.contents));
    }
    out
}

#[test]
fn renders_every_version_and_the_site_index() {
    let files = render_versions();
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/versions-out");
    let bless = std::env::var_os("COMLINE_BLESS").is_some();
    if bless {
        let _ = std::fs::remove_dir_all(&out);
    }
    for (path, contents) in &files {
        let path = out.join(path);
        if bless {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
        } else {
            let expected = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("no golden {}; run with COMLINE_BLESS=1", path.display()));
            assert_eq!(contents, &expected, "{} drifted; run with COMLINE_BLESS=1", path.display());
        }
    }
}

/// The multi-version site holds to `file://` too: no network, relative
/// links, data inlined in scripts; each version's pages reach the picker's
/// data at `../versions.js`, and the root points at the latest release.
#[test]
fn a_multi_version_site_loads_nothing_from_the_network() {
    let files = render_versions();
    for (path, contents) in &files {
        for needle in ["src=\"http", "src=\"//", "href=\"//", "href=\"/", "@import", "fetch(", "XMLHttpRequest"] {
            assert!(!contents.contains(needle), "{path} has {needle}");
        }
        if path.ends_with(".html") && path.contains('/') {
            let depth = path.matches('/').count();
            let versions = format!("<script src=\"{}versions.js\"></script>", "../".repeat(depth));
            assert!(contents.contains(&versions), "{path}");
            assert!(contents.contains("<select id=\"version-picker\">"), "{path}");
        }
    }
    let get = |p: &str| &files.iter().find(|(path, _)| path == p).unwrap().1;
    assert!(get("index.html").contains("url=0.2.0/index.html"));
    assert!(get("versions.js").contains("var LATEST = \"0.2.0\";"));
    assert!(get("0.1.0/index.html").contains("Built before headers were frozen"));
    assert!(!get("unreleased/api.html").contains("class=\"note\" role=\"note\""));
    let compare = get("compare.js");
    assert!(compare.contains(r#""severity":"major","title":"Payment.Cash: enum variant removed","unit":"Payment""#), "{compare}");
    // Each entry comes rendered, its old and new lines highlighted.
    assert!(compare.contains(r#"<span class=\"del\"><span class=\"sign\" aria-label=\"before\">−</span> <span class=\"t\">Cash</span></span>"#), "{compare}");
    // Every page has the theme toggle, applied before first paint.
    for (path, contents) in &files {
        if path.ends_with(".html") {
            assert!(contents.contains("id=\"theme-toggle\""), "{path}");
            let head = contents.split("</head>").next().unwrap();
            assert!(head.contains("localStorage.getItem(\"comline-doc-theme\")"), "{path}");
        }
    }
    assert!(compare.contains(r#""api::Orders.cancel":"cancel(order: Order) -> bool""#), "{compare}");
}

/// The version in `comline doc`'s cache key carries a content hash of the
/// renderer's sources and assets, so it moves whenever the output can.
#[test]
fn the_version_carries_a_content_hash() {
    let version = comline_doc_html::RENDERER.version;
    let (crate_version, hash) = version.split_once('+').expect("crate version + hash");
    assert_eq!(crate_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(hash.len(), 16, "{version}");
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "{version}");
}
