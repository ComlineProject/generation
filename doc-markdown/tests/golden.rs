//! Golden tests: `tests/golden/model.json` (a documentation model from
//! core's `doc::model`, with a changelog) renders to `tests/golden/out/`.
//! `COMLINE_BLESS=1 cargo test -p comline-doc-markdown` rewrites the
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
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/model.json")).unwrap()
}

#[test]
fn renders_the_golden_pages() {
    let files = comline_doc_markdown::render_json(&model(), &options()).unwrap();
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
    assert_eq!(paths, ["index.md", "api.md", "types.md", "deps/localdeps/greeting.md", "changelog.md", "nav.yml"]);
}

#[test]
fn dependency_types_link_to_their_docs() {
    let files = comline_doc_markdown::render_json(&model(), &RenderOptions::default()).unwrap();
    let get = |p: &str| &files.iter().find(|f| f.path.to_str() == Some(p)).unwrap().contents;
    let api = get("api.md");
    // `std` without a `[doc] links` entry: its hosted docs.
    assert!(api.contains("[`HttpMethod`](https://comlineproject.github.io/docs/std/http/#httpmethod)"), "{api}");
    // An unlinked dependency: its page here, never an own module's.
    assert!(api.contains("[`Greeting`](deps/localdeps/greeting.md#greeting)"), "{api}");
    assert!(api.contains("[`Customer`](types.md#customer)"));
    assert!(get("deps/localdeps/greeting.md").starts_with("# `localdeps::greeting`"));
    assert!(!files.iter().any(|f| f.path.starts_with("deps/std")));
    let nav = get("nav.yml");
    assert!(nav.contains("  - Dependencies:\n    - \"localdeps\":\n      - \"greeting\": deps/localdeps/greeting.md\n    - \"std\": \"https://comlineproject.github.io/docs/std/\"\n"), "{nav}");

    // A link for it publishes it elsewhere instead.
    let links = BTreeMap::from([("localdeps".to_string(), "https://example.com/localdeps".to_string())]);
    let files = comline_doc_markdown::render_json(&model(), &RenderOptions { links, ..Default::default() }).unwrap();
    assert!(!files.iter().any(|f| f.path.starts_with("deps")));
    let api = &files.iter().find(|f| f.path.to_str() == Some("api.md")).unwrap().contents;
    assert!(api.contains("[`Greeting`](https://example.com/localdeps/greeting/#greeting)"), "{api}");
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
    let files = comline_doc_markdown::render_json(&model.to_string(), &RenderOptions::default()).unwrap();
    let log = &files.iter().find(|f| f.path.to_str() == Some("changelog.md")).unwrap().contents;
    assert!(log.contains("Everything since the last release: [0.1.0 to 0.2.0](#since-0.1.0)."), "{log}");
    assert!(log.contains("<a id=\"since-0.1.0\"></a>\n\n## Since the last release: 0.1.0 to 0.2.0"), "{log}");

    let mut release = model["changelog"][0].clone();
    release["from"] = "0.0.9".into();
    release["span"] = "release".into();
    model["changelog"] = serde_json::json!([model["changelog"][0].clone(), release]);
    let files = comline_doc_markdown::render_json(&model.to_string(), &RenderOptions::default()).unwrap();
    let log = &files.iter().find(|f| f.path.to_str() == Some("changelog.md")).unwrap().contents;
    assert!(log.contains("## 0.0.9 to 0.2.0"), "{log}");
    assert!(!log.contains("## 0.1.0 to 0.2.0"), "{log}");
}

#[test]
fn renders_through_the_protocol() {
    let mut registry = comline_codegen::Registry::new();
    comline_doc_markdown::register(&mut registry);
    let Response::Describe(d) = protocol::respond(&registry, &protocol::encode(&Request::describe()).unwrap()) else {
        panic!()
    };
    assert_eq!(d.renderers.len(), 1);
    assert_eq!(d.renderers[0].id, "markdown");
    let request = Request::render(RenderRequest {
        renderer: "markdown".into(),
        model_version: 1,
        model: model(),
        options: options(),
    });
    let Response::Render(out) = protocol::respond(&registry, &protocol::encode(&request).unwrap()) else { panic!() };
    assert_eq!(out.files.len(), 6);
}

/// The 3-version fixture (`../doc-markdown/tests/golden/versions/`, written by
/// its `make.py`): two releases, a dev build and an unreleased change, each
/// rendered into its own folder, then the site's index, as `comline doc`
/// asks for them. The goldens are `tests/golden/versions-out/`.
pub fn render_versions() -> Vec<(String, String)> {
    use comline_codegen::protocol::{SiteVersion, VersionKind};
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/versions");
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
        for file in comline_doc_markdown::render_json(&model, &options).unwrap() {
            out.push((format!("{id}/{}", file.path.to_string_lossy().replace('\\', "/")), file.contents));
        }
        site.push(SiteVersion { id: id.into(), release: release.into(), kind, model });
    }
    let request = RenderRequest {
        renderer: comline_doc_markdown::RENDERER.id.into(),
        model_version: 1,
        model: site.last().unwrap().model.clone(),
        options: RenderOptions { versions: site, ..options() },
    };
    for file in comline_doc_markdown::render(&request).unwrap() {
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

/// The version in `comline doc`'s cache key carries a content hash of the
/// renderer's sources and assets, so it moves whenever the output can.
#[test]
fn the_version_carries_a_content_hash() {
    let version = comline_doc_markdown::RENDERER.version;
    let (crate_version, hash) = version.split_once('+').expect("crate version + hash");
    assert_eq!(crate_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(hash.len(), 16, "{version}");
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "{version}");
}
