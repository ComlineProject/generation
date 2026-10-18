//! Runs every generator over every fixture and diffs against the checked-in
//! golden output. Regenerate the goldens with:
//!
//! ```text
//! CONFORMANCE_BLESS=1 cargo test -p comline-conformance
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use std::collections::BTreeMap;

use comline_codegen::features::{self, Column, Feature, Options, State, SupportTable};
use comline_codegen::{GenRequest, GeneratedFile, PackageMeta};
use comline_conformance::{corpus, Fixture};
use comline_core::schema::ir::frozen::cas::blob::schema_ir_hash;
use comline_core::schema::ir::frozen::unit::FrozenUnit;

const LANGS: &[&str] = &["rust", "typescript"];

fn generate(lang: &str, req: &GenRequest) -> eyre::Result<Vec<GeneratedFile>> {
    match lang {
        "rust" => comline_codegen_rust::generate_rust(req),
        "typescript" => comline_codegen_typescript::generate_typescript(req),
        other => panic!("unknown language `{other}`"),
    }
}

/// What `lang`'s generator claims to support.
fn support(lang: &str) -> SupportTable {
    match lang {
        "rust" => comline_codegen_rust::SUPPORT,
        "typescript" => comline_codegen_typescript::SUPPORT,
        other => panic!("unknown language `{other}`"),
    }
}

fn lang_version(lang: &str) -> &'static str {
    match lang {
        "rust" => comline_codegen_rust::LANG_VERSION,
        "typescript" => comline_codegen_typescript::LANG_VERSION,
        other => panic!("unknown language `{other}`"),
    }
}

/// The request knobs `fx` sets, as the features detector reads them.
fn options(fx: &Fixture) -> Options {
    Options {
        mode: Some(fx.mode),
        external_std: fx.external_std,
        validate_on_decode: fx.validate_on_decode,
        default_framing: None,
    }
}

/// Whether `fx` uses a feature `lang`'s generator claims not to support - in
/// which case it must refuse, and has no golden.
fn refused(fx: &Fixture, lang: &str) -> bool {
    let uses = features::used(&fx.schemas, &options(fx));
    !features::unsupported_uses(&uses, support(lang)).is_empty()
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
    request_for(&fx.schemas, &options(fx), lang)
}

/// A request for `schemas` under `options` - a fixture's own, or the same
/// with a feature stripped out.
fn request_for<'a>(schemas: &'a [(String, Vec<FrozenUnit>)], options: &Options, lang: &str) -> GenRequest<'a> {
    GenRequest {
        mode: options.mode.expect("a fixture states its mode"),
        schemas,
        package: PackageMeta {
            name: "conformance".to_string(),
            version: "0.0.0".to_string(),
        },
        // Each generator's own registered version. `Mode::Code` has no
        // manifest to record it in, so no golden depends on it - but a
        // request that named the wrong one would still be a lie about what
        // is being asked for.
        lang_version: lang_version(lang).to_string(),
        default_framing: options.default_framing.clone(),
        external_std: options.external_std,
        validate_on_decode: options.validate_on_decode,
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
            let dir = golden_dir(fx.name, lang);

            // A fixture using a feature this generator claims not to support
            // must be refused - and so has nothing to compare.
            if refused(&fx, lang) {
                if bless {
                    let _ = fs::remove_dir_all(&dir);
                } else if !existing_goldens(&dir).is_empty() {
                    drift.push(format!(
                        "{} / {lang}: refused, but has goldens - run with CONFORMANCE_BLESS=1",
                        fx.name
                    ));
                }
                continue;
            }

            let files = generate(lang, &req)
                .unwrap_or_else(|e| panic!("{} / {lang}: the generator failed: {e}", fx.name));

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

/// The proof behind every generator's `SUPPORT` claims, against the corpus.
/// For each generator and feature:
/// - the table states it, once;
/// - **supported / partial**: some fixture uses it and generates (its golden
///   is checked by [`corpus_matches_golden`]); partial says what's missing;
/// - **ignored**: some fixture uses it and generates, and every fixture
///   using it generates exactly what it would with the feature stripped;
/// - **unsupported**: some fixture uses it, and every fixture using it is
///   refused with an error naming the feature.
///
/// It also writes the report - `FEATURES.md` and `features.json` at the
/// repository root - from what it proved; `CONFORMANCE_BLESS=1` regenerates
/// them, and a stale one fails here like a stale golden.
#[test]
fn support_is_proven() {
    let corpus = corpus();
    let mut failures = Vec::new();
    let mut evidence: BTreeMap<(String, Feature), Vec<String>> = BTreeMap::new();

    for &lang in LANGS {
        let table = support(lang);

        for &feature in Feature::ALL {
            let entries = table.iter().filter(|(f, _)| *f == feature).count();
            if entries != 1 {
                failures.push(format!("{lang}: `{}` is stated {entries} times, not once", feature.id()));
                continue;
            }
        }

        for &feature in Feature::ALL {
            let claim = features::support_for(table, feature);
            let using: Vec<&Fixture> = corpus
                .iter()
                .filter(|fx| features::used(&fx.schemas, &options(fx)).iter().any(|u| u.feature == feature))
                .collect();
            if using.is_empty() {
                failures.push(format!(
                    "{lang}: `{}` is claimed {}, but no fixture uses it",
                    feature.id(),
                    claim.state.id()
                ));
                continue;
            }

            let mut proven = Vec::new();
            let failed_before = failures.len();
            match claim.state {
                State::Supported | State::Partial => {
                    if claim.state == State::Partial && claim.note.is_none() {
                        failures.push(format!("{lang}: `{}` is partial without saying what's missing", feature.id()));
                    }
                    // A feature that can be taken out must make a difference
                    // somewhere: generating the same without it is ignoring it.
                    let mut differs = false;
                    for fx in &using {
                        if refused(fx, lang) {
                            continue;
                        }
                        match generate(lang, &request(fx, lang)) {
                            Ok(with) => {
                                proven.push(fx.name.to_string());
                                if let Some((schemas, stripped)) = features::strip(feature, &fx.schemas, &options(fx)) {
                                    let hashes = ir_hashes(&fx.schemas, &schemas);
                                    match generate(lang, &request_for(&schemas, &stripped, lang)) {
                                        Ok(without) => differs |= !same_files(&with, &without, &hashes),
                                        Err(_) => differs = true,
                                    }
                                } else {
                                    differs = true;
                                }
                            }
                            Err(e) => failures.push(format!("{lang}: `{}` fails `{}`: {e}", fx.name, feature.id())),
                        }
                    }
                    if !proven.is_empty() && !differs {
                        failures.push(format!(
                            "{lang}: `{}` is claimed {}, but every fixture using it generates the same without it \
                             - it's ignored",
                            feature.id(),
                            claim.state.id()
                        ));
                    }
                }
                State::Ignored => {
                    for fx in &using {
                        if refused(fx, lang) {
                            continue;
                        }
                        let Some((schemas, stripped)) = features::strip(feature, &fx.schemas, &options(fx)) else {
                            failures.push(format!(
                                "{lang}: `{}` is claimed ignored, but it can't be stripped to prove it",
                                feature.id()
                            ));
                            break;
                        };
                        let with = generate(lang, &request(fx, lang));
                        let without = generate(lang, &request_for(&schemas, &stripped, lang));
                        // The IR hash names the schema itself, so it differs
                        // with the feature taken out - rightly: a peer that
                        // does check must not handshake as the same schema.
                        // Everything else must be identical.
                        let hashes = ir_hashes(&fx.schemas, &schemas);
                        match (with, without) {
                            (Ok(with), Ok(without)) if same_files(&with, &without, &hashes) => {
                                proven.push(fx.name.to_string())
                            }
                            (Ok(_), Ok(_)) => failures.push(format!(
                                "{lang}: `{}` is claimed ignored, but `{}` generates differently without it",
                                feature.id(),
                                fx.name
                            )),
                            (Err(e), _) | (_, Err(e)) => {
                                failures.push(format!("{lang}: `{}` fails `{}`: {e}", fx.name, feature.id()))
                            }
                        }
                    }
                }
                State::Unsupported => {
                    for fx in &using {
                        match generate(lang, &request(fx, lang)) {
                            Err(e) if e.to_string().contains(&format!("({})", feature.id())) => {
                                proven.push(fx.name.to_string())
                            }
                            Err(e) => failures.push(format!(
                                "{lang}: `{}` is refused without naming `{}`: {e}",
                                fx.name,
                                feature.id()
                            )),
                            Ok(_) => failures.push(format!(
                                "{lang}: `{}` is claimed unsupported, but `{}` generates anyway",
                                feature.id(),
                                fx.name
                            )),
                        }
                    }
                }
            }
            if proven.is_empty() && failures.len() == failed_before {
                failures.push(format!(
                    "{lang}: `{}` is claimed {}, but every fixture using it is refused for another feature",
                    feature.id(),
                    claim.state.id()
                ));
            }
            evidence.insert((lang.to_string(), feature), proven);
        }
    }

    // The report, from what was proven.
    let columns: Vec<Column> = LANGS
        .iter()
        .map(|&lang| Column { generator: lang, version: lang_version(lang), support: support(lang) })
        .collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let reports = [
        (
            root.join("FEATURES.md"),
            format!(
                "# Generator features\n\n\
                 <!-- Generated by `comline-conformance` (`support_is_proven`) - don't edit by hand. \
                 Regenerate with `CONFORMANCE_BLESS=1 cargo test -p comline-conformance`. -->\n\n\
                 What each generator implements, and the conformance fixtures that prove it. \
                 A claim is checked, not trusted: a supported feature has a fixture that generates \
                 and matches its golden, an ignored one generates exactly what it would without the \
                 feature, and an unsupported one is refused with an error naming every use.\n\n{}",
                features::markdown(&columns, &evidence)
            ),
        ),
        (
            root.join("features.json"),
            serde_json::to_string_pretty(&features::json(&columns, &evidence)).unwrap() + "\n",
        ),
    ];
    let bless = std::env::var_os("CONFORMANCE_BLESS").is_some();
    for (path, contents) in &reports {
        if bless {
            fs::write(path, contents).unwrap();
        } else if fs::read_to_string(path).ok().as_deref() != Some(contents.as_str()) {
            failures.push(format!("`{}` is out of date - run with CONFORMANCE_BLESS=1", path.display()));
        }
    }

    assert!(failures.is_empty(), "unproven feature support ({}):\n  {}", failures.len(), failures.join("\n  "));
}

/// Every schema's IR hash in `a` and `b`, to mask when comparing their output.
fn ir_hashes(a: &[(String, Vec<FrozenUnit>)], b: &[(String, Vec<FrozenUnit>)]) -> Vec<u64> {
    a.iter().chain(b).map(|(_, units)| schema_ir_hash(units)).collect()
}

/// The same files with the same contents, in any order, once each of
/// `hashes` is masked out.
fn same_files(a: &[GeneratedFile], b: &[GeneratedFile], hashes: &[u64]) -> bool {
    let mask = |contents: &str| {
        hashes.iter().fold(contents.to_string(), |text, h| {
            text.replace(&format!("{h:016x}"), "<ir-hash>").replace(&format!("{h:x}"), "<ir-hash>")
        })
    };
    let key = |files: &[GeneratedFile]| {
        let mut v: Vec<(PathBuf, String)> = files.iter().map(|f| (f.path.clone(), mask(&f.contents))).collect();
        v.sort();
        v
    };
    key(a) == key(b)
}
