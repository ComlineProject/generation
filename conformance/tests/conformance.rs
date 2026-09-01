//! Runs every generator over every fixture and diffs against the checked-in
//! golden output. Regenerate the goldens with:
//!
//! ```text
//! CONFORMANCE_BLESS=1 cargo test -p comline-conformance
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use comline_codegen::{GeneratedFile, GenRequest, Mode, PackageMeta};
use comline_conformance::corpus;
use comline_core::schema::ir::frozen::unit::FrozenUnit;

/// `(generator name, file extension)`.
const LANGS: &[(&str, &str)] = &[("rust", "rs"), ("typescript", "ts")];

fn generate(lang: &str, req: &GenRequest) -> Vec<GeneratedFile> {
    let result = match lang {
        "rust" => comline_codegen_rust::generate_rust(req),
        "typescript" => comline_codegen_typescript::generate_typescript(req),
        other => panic!("unknown language `{other}`"),
    };
    result.unwrap_or_else(|e| panic!("`{lang}` generator failed: {e}"))
}

fn golden_path(fixture: &str, lang: &str, ext: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(fixture)
        .join(format!("{lang}.{ext}"))
}

fn request(schemas: &[(String, Vec<FrozenUnit>)]) -> GenRequest<'_> {
    GenRequest {
        mode: Mode::Code,
        schemas,
        package: PackageMeta {
            name: "conformance".to_string(),
            version: "0.0.0".to_string(),
        },
    }
}

#[test]
fn corpus_matches_golden() {
    let bless = std::env::var_os("CONFORMANCE_BLESS").is_some();
    let mut drift = Vec::new();

    for fx in corpus() {
        let schemas = vec![(fx.namespace.to_string(), fx.units.clone())];
        for &(lang, ext) in LANGS {
            let req = request(&schemas);
            let files = generate(lang, &req);
            assert_eq!(
                files.len(),
                1,
                "fixture `{}` / `{lang}`: expected exactly one generated file, got {}",
                fx.name,
                files.len()
            );
            let got = &files[0].contents;
            let path = golden_path(fx.name, lang, ext);

            if bless {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, got).unwrap();
                continue;
            }

            match fs::read_to_string(&path) {
                Ok(want) if *got == want => {}
                Ok(want) => drift.push(format!(
                    "--- {} / {lang} ---\n{}",
                    fx.name,
                    line_diff(&want, got)
                )),
                Err(_) => drift.push(format!(
                    "{} / {lang}: no golden at {} — run with CONFORMANCE_BLESS=1",
                    fx.name,
                    path.display()
                )),
            }
        }
    }

    assert!(
        drift.is_empty(),
        "conformance drift ({} case(s)):\n\n{}",
        drift.len(),
        drift.join("\n\n")
    );
}

/// A minimal line-oriented diff for the failure message.
fn line_diff(want: &str, got: &str) -> String {
    let mut out = String::new();
    let (w, g): (Vec<_>, Vec<_>) = (want.lines().collect(), got.lines().collect());
    for i in 0..w.len().max(g.len()) {
        match (w.get(i), g.get(i)) {
            (Some(a), Some(b)) if a == b => {}
            (a, b) => {
                if let Some(a) = a {
                    out.push_str(&format!("- {a}\n"));
                }
                if let Some(b) = b {
                    out.push_str(&format!("+ {b}\n"));
                }
            }
        }
    }
    out
}
