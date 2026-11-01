//! `schemas/comline-toml.json` is generated from `comline_codegen::config`.
//! This test fails when the committed file drifts from the types; regenerate
//! it with `COMLINE_BLESS=1 cargo test -p comline-codegen --test comline_toml_schema`.
//!
//! It also checks the schema against `comline.toml` documents: every one in
//! this repo, extra directories named by `COMLINE_TOML_CHECK_DIRS`
//! (`:`-separated, e.g. a CLI checkout's fixtures), and the mistakes an
//! editor should flag.

use std::path::{Path, PathBuf};

use comline_codegen::config::{self, ComlineToml};

fn schema_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../schemas/comline-toml.json")
}

#[test]
fn the_committed_schema_matches_the_types() {
    let generated = serde_json::to_string_pretty(&config::json_schema()).unwrap() + "\n";
    let path = schema_path();
    if std::env::var_os("COMLINE_BLESS").is_some() {
        std::fs::write(&path, &generated).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == generated,
        "{} is out of date with comline_codegen::config - regenerate it with \
         `COMLINE_BLESS=1 cargo test -p comline-codegen --test comline_toml_schema`",
        path.display()
    );
}

fn validator() -> jsonschema::Validator {
    jsonschema::draft7::new(&config::json_schema()).expect("the schema is valid draft-07")
}

/// A TOML document as the JSON an editor validates.
fn as_json(text: &str) -> serde_json::Value {
    let value: toml::Value = toml::from_str(text).unwrap();
    serde_json::to_value(value).unwrap()
}

/// Every key, in both places it can be written.
const EVERY_KEY: &str = r#"
#:schema https://comlineproject.github.io/docs/schemas/comline-toml/0.x.json
[generate]
only = ["rust"]
out = "generated"
layout = "{{language}}/{{namespace}}.{{ext}}"
mode = "code"
external_std = false
package_versions = "latest"
validate_on_decode = "off"
async = false
encodings = { "com.acme.lines@1" = "acme::LinesFraming", "jsonrpc@1" = { type = "Rpc", package = "rpc", version = "^1" } }

[[generate.target]]
language = "rust"
generator = "gen.wasm"
timeout = 30
lang_version = "1.88.0"
out = "src/generated"
layout = "{{namespace}}.{{ext}}"
mode = "lib"
package_versions = ["0.3.0", "0.4.0"]
external_std = true
validate_on_decode = "collect"
async = true
encodings = { "com.acme.lines@1" = "mine::Lines" }
flatten = true

[doc]
out = "docs/api"
formats = ["html", "markdown", "json"]
versions = { last = 5, since = "0.3.0", releases_only = true }
renderer = "render.wasm"
timeout = 30
links = { std = "https://comlineproject.github.io/docs/std/" }
changelog_from = "0.3.0"
"#;

fn toml_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && path
                .file_name()
                .is_some_and(|n| n != "target" && n != ".git")
        {
            toml_files(&path, found);
        } else if path.file_name().is_some_and(|n| n == "comline.toml") {
            found.push(path);
        }
    }
}

#[test]
fn every_comline_toml_the_parser_accepts_is_valid() {
    let validator = validator();
    let mut documents: Vec<(String, String)> = [
        ("every key", EVERY_KEY),
        ("empty", ""),
        (
            "a bare target",
            "[[generate.target]]\nlanguage = \"rust\"\n",
        ),
        ("a section only", "[generate]\nout = \"gen\"\n"),
        ("every version", "[doc]\nversions = \"all\"\n"),
    ]
    .into_iter()
    .map(|(name, text)| (name.to_string(), text.to_string()))
    .collect();
    let mut files = Vec::new();
    toml_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(".."),
        &mut files,
    );
    for dir in std::env::var("COMLINE_TOML_CHECK_DIRS")
        .unwrap_or_default()
        .split(':')
    {
        if !dir.is_empty() {
            toml_files(Path::new(dir), &mut files);
        }
    }
    for file in files {
        documents.push((
            file.display().to_string(),
            std::fs::read_to_string(&file).unwrap(),
        ));
    }
    for (name, text) in &documents {
        // The parser is the reference: what it accepts, the schema accepts.
        let Ok(parsed) = ComlineToml::parse(text) else {
            continue;
        };
        // Except the retired key, which parses only to be refused by name.
        if parsed.generate.default_framing.is_some()
            || parsed
                .generate
                .targets
                .iter()
                .any(|t| t.default_framing.is_some())
        {
            continue;
        }
        let errors: Vec<String> = validator
            .iter_errors(&as_json(text))
            .map(|e| format!("{e} at {}", e.instance_path()))
            .collect();
        assert!(
            errors.is_empty(),
            "{name} is valid comline.toml, but the schema says: {errors:#?}"
        );
        eprintln!("valid: {name}");
    }
}

#[test]
fn the_schema_rejects_what_the_parser_or_resolver_rejects() {
    let validator = validator();
    for bad in [
        // A typo'd key, at each level.
        "[generat]\n",
        "[generate]\noutt = \"x\"\n",
        "[[generate.target]]\nlanguage = \"rust\"\nlang_verison = \"1.85\"\n",
        "[generate]\nencodings = { \"x@1\" = { type = \"T\", pakage = \"p\" } }\n",
        // A wrong enum value.
        "[generate]\nmode = \"dylib\"\n",
        "[[generate.target]]\nmode = \"library\"\n",
        "[generate]\nvalidate_on_decode = \"always\"\n",
        // The wrong type.
        "[generate]\nasync = \"yes\"\n",
        "[[generate.target]]\ntimeout = 0\n",
        // The retired key.
        "[generate]\ndefault_framing = \"datagram\"\n",
    ] {
        assert!(
            !validator.is_valid(&as_json(bad)),
            "the schema accepts {bad:?}"
        );
    }
}

/// `#:schema` is a TOML comment: the parser never sees it.
#[test]
fn the_schema_directive_is_a_comment() {
    let parsed = ComlineToml::parse(
        "#:schema https://comlineproject.github.io/docs/schemas/comline-toml/0.x.json\n\
         [generate]\nout = \"x\"\n",
    )
    .unwrap();
    assert_eq!(parsed.generate.out.as_deref(), Some("x"));
}
