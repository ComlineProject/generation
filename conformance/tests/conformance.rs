//! Runs every generator over every fixture and diffs against the checked-in
//! golden output. Regenerate the goldens with:
//!
//! ```text
//! CONFORMANCE_BLESS=1 cargo test -p comline-conformance
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use comline_codegen::{GenRequest, GeneratedFile, PackageMeta};
use comline_conformance::{corpus, Fixture};

const LANGS: &[&str] = &["rust", "typescript"];

fn generate(lang: &str, req: &GenRequest) -> Vec<GeneratedFile> {
    let result = match lang {
        "rust" => comline_codegen_rust::generate_rust(req),
        "typescript" => comline_codegen_typescript::generate_typescript(req),
        other => panic!("unknown language `{other}`"),
    };
    result.unwrap_or_else(|e| panic!("`{lang}` generator failed: {e}"))
}

/// `tests/golden/<fixture>/<lang>/` — one directory per (fixture, language),
/// holding the generated files at the paths the generator itself chose. A
/// fixture is a package now, so a single golden *file* per language can no
/// longer hold the answer.
fn golden_dir(fixture: &str, lang: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(fixture)
        .join(lang)
}

fn request<'a>(fx: &'a Fixture, lang: &str) -> GenRequest<'a> {
    GenRequest {
        mode: fx.mode,
        schemas: &fx.schemas,
        package: PackageMeta {
            name: "conformance".to_string(),
            version: "0.0.0".to_string(),
        },
        // Each generator's own registered version. `Mode::Code` has no
        // manifest to record it in, so no golden depends on it - but a
        // request that named the wrong one would still be a lie about what
        // is being asked for.
        lang_version: match lang {
            "rust" => comline_codegen_rust::LANG_VERSION,
            "typescript" => comline_codegen_typescript::LANG_VERSION,
            other => panic!("unknown language `{other}`"),
        }
        .to_string(),
        default_framing: None,
        external_std: fx.external_std,
    }
}

/// Every file under `dir`, as paths relative to it — so a golden that no
/// longer corresponds to anything the generator emits is reported instead of
/// lingering. Blessing would otherwise leave orphans behind forever.
fn existing_goldens(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, base, out);
            } else if let Ok(rel) = path.strip_prefix(base) {
                out.push(rel.to_path_buf());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn corpus_matches_golden() {
    let bless = std::env::var_os("CONFORMANCE_BLESS").is_some();
    let mut drift = Vec::new();

    for fx in corpus() {
        for &lang in LANGS {
            let req = request(&fx, lang);
            let files = generate(lang, &req);
            let dir = golden_dir(fx.name, lang);

            if bless {
                // Clear first: a file the generator no longer emits must not
                // survive as a golden.
                let _ = fs::remove_dir_all(&dir);
                for file in &files {
                    let path = dir.join(&file.path);
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(&path, &file.contents).unwrap();
                }
                continue;
            }

            for file in &files {
                let path = dir.join(&file.path);
                match fs::read_to_string(&path) {
                    Ok(want) if file.contents == want => {}
                    Ok(want) => drift.push(format!(
                        "--- {} / {lang} / {} ---\n{}",
                        fx.name,
                        file.path.display(),
                        line_diff(&want, &file.contents)
                    )),
                    Err(_) => drift.push(format!(
                        "{} / {lang}: no golden at {} — run with CONFORMANCE_BLESS=1",
                        fx.name,
                        path.display()
                    )),
                }
            }

            let emitted: Vec<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
            for stale in existing_goldens(&dir) {
                if !emitted.contains(&stale) {
                    drift.push(format!(
                        "{} / {lang}: golden `{}` is no longer emitted — run with \
                         CONFORMANCE_BLESS=1",
                        fx.name,
                        stale.display()
                    ));
                }
            }
        }
    }

    // A golden *outside* any language directory. The per-language sweep above
    // can't see these: when fixtures were one schema the layout was
    // `golden/<fixture>/<lang>.<ext>`, and moving to `<lang>/<path>` left the
    // old files tracked and invisible. Anything at the wrong level is
    // reported rather than ignored.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    for fx in corpus() {
        let dir = root.join(fx.name);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let known_lang = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| LANGS.contains(&n));
            if !path.is_dir() || !known_lang {
                drift.push(format!(
                    "{}: `{}` is not a language directory — a stale golden from an \
                     older layout?",
                    fx.name,
                    path.file_name().unwrap_or_default().to_string_lossy()
                ));
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
