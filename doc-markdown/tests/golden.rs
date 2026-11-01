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
    assert_eq!(paths, ["index.md", "api.md", "types.md", "changelog.md", "nav.yml"]);
}

#[test]
fn a_dependency_type_without_a_link_is_plain_text() {
    let files = comline_doc_markdown::render_json(&model(), &RenderOptions::default()).unwrap();
    let api = files.iter().find(|f| f.path.to_str() == Some("api.md")).unwrap();
    assert!(api.contents.contains("| `method` | `HttpMethod` |"), "{}", api.contents);
    assert!(api.contents.contains("[`Customer`](types.md#customer)"));
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
    assert_eq!(out.files.len(), 5);
}
