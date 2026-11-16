//! The generator-agnostic half of the conformance suite: what runs a
//! generator over the [`corpus`](crate::corpus) and checks it.
//!
//! The corpus and this harness live here, in `ComlineProject/generation`,
//! because they need only `comline-codegen` and core. A generator does not
//! live here: it is a repository of its own that depends on this crate and
//! runs the harness over its own output, against goldens it holds itself.
//! That keeps the dependency one way - a generator depends on generation,
//! never the other - so a change to core is pinned in generation, then in
//! each generator, and nowhere does it come back.
//!
//! A generator describes itself as a [`Generator`], and its conformance test
//! is three calls:
//!
//! ```ignore
//! use comline_conformance::harness::{self, Generator};
//!
//! const GENERATOR: Generator = Generator {
//!     language: "rust",
//!     generate: comline_codegen_rust::generate_rust,
//!     register: comline_codegen_rust::register,
//!     support: comline_codegen_rust::SUPPORT,
//!     versions: comline_codegen_rust::VERSIONS,
//! };
//!
//! #[test]
//! fn corpus_matches_golden() {
//!     harness::corpus_matches_golden(&GENERATOR, &golden_root);
//! }
//! ```
//!
//! `CONFORMANCE_BLESS=1` regenerates what these check: the goldens, and the
//! generator's `FEATURES.md` and `features.json`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use comline_codegen::connections;
use comline_codegen::features::{self, Column, Feature, Options, State, SupportTable};
use comline_codegen::versions::{LangVersion, Versions};
use comline_codegen::{protocol, GenerateRequest, GeneratedFile, Registry};
use comline_core::schema::idl::annotations::DEFAULT_FORMAT;
use comline_core::schema::ir::frozen::cas::blob::schema_ir_hash;
use comline_core::schema::ir::frozen::unit::FrozenUnit;

use crate::{corpus, Fixture};

/// One generator, as the harness sees it: what it generates, what it claims
/// and which versions of its language it handles.
#[derive(Clone, Copy)]
pub struct Generator {
    /// `rust`, `typescript`: the language a request targets, and the name of
    /// the generator's goldens directories.
    pub language: &'static str,
    /// The generator's entry point.
    pub generate: fn(&GenerateRequest) -> eyre::Result<Vec<GeneratedFile>>,
    /// Registers the generator with a [`Registry`], as its binary does.
    pub register: fn(&mut Registry),
    /// What the generator claims to support.
    pub support: SupportTable,
    /// Which versions of its language it handles.
    pub versions: Versions,
}

impl Generator {
    /// Every version the generator's output or support can change at, the
    /// floor first: testing at each covers every version.
    fn boundaries(&self) -> Vec<&'static str> {
        self.versions.boundaries()
    }

    fn floor(&self, version: &str) -> bool {
        version == self.versions.floor
    }
}

fn parsed(version: &str) -> LangVersion {
    LangVersion::parse(version).unwrap_or_else(|| panic!("`{version}` isn't a version"))
}

fn blessing() -> bool {
    std::env::var_os("CONFORMANCE_BLESS").is_some()
}

/// The request knobs `fx` sets, as the features detector reads them.
fn options(fx: &Fixture) -> Options {
    Options {
        mode: Some(fx.mode),
        external_std: fx.external_std,
        validate_on_decode: fx.validate_on_decode,
        wire_format: fx.wire_format.map(str::to_string),
        async_api: fx.async_api,
        ..Default::default()
    }
}

/// Whether `fx` uses a feature `g` claims not to support at `version` - in
/// which case it must refuse, and has no golden there.
fn refused(g: &Generator, fx: &Fixture, version: &str) -> bool {
    let uses = features::used(&fx.schemas, &options(fx));
    !features::unsupported_uses(&uses, g.support, &parsed(version)).is_empty()
}

/// `<root>/<fixture>/<lang>/` for the floor, and
/// `<fixture>/<lang>@<boundary>/` for a later boundary whose output differs
/// from the one before it - one directory per (fixture, language, version),
/// holding the generated files at the paths the generator itself chose.
fn golden_dir(root: &Path, g: &Generator, fixture: &str, version: &str) -> PathBuf {
    let name = if g.floor(version) {
        g.language.to_string()
    } else {
        format!("{}@{version}", g.language)
    };
    root.join(fixture).join(name)
}

fn request(g: &Generator, fx: &Fixture, version: &str) -> GenerateRequest {
    request_for(g, &fx.schemas, &fx.dependencies, &options(fx), version)
}

/// A request for `schemas` under `options` at `version` - a fixture's own,
/// or the same with a feature stripped out. `dependencies` are the names the
/// package gives its dependencies, whose schemas are among `schemas`.
fn request_for(
    g: &Generator,
    schemas: &[(String, Vec<FrozenUnit>)],
    dependencies: &[&str],
    options: &Options,
    version: &str,
) -> GenerateRequest {
    let wire_format = options
        .wire_format
        .clone()
        .unwrap_or_else(|| DEFAULT_FORMAT.to_string());
    let mut request = GenerateRequest {
        mode: options.mode.expect("a fixture states its mode"),
        schemas: schemas.to_vec(),
        package: comline_codegen::PackageMeta {
            name: "conformance".to_string(),
            version: "0.0.0".to_string(),
        },
        // One of the generator's own boundaries: what a target declaring it
        // gets, and every version up to the next boundary gets the same.
        target: comline_codegen::TargetLanguage {
            language: g.language.to_string(),
            version: version.to_string(),
        },
        wire_format: wire_format.clone(),
        external_std: options.external_std,
        validate_on_decode: options.validate_on_decode,
        async_api: options.async_api,
        encodings: comline_codegen::encodings::official_for(g.language),
        facts: None,
        dependencies: dependencies.iter().map(|d| d.to_string()).collect(),
    };
    // Only a package that needs them gets facts, as from `comline generation`:
    // the package's own, a dependency's protocols having their own package's.
    if comline_codegen::facts::needed_in(&request) {
        let own: Vec<_> = schemas
            .iter()
            .filter(|(namespace, _)| !connections::is_dependency(&request.dependencies, namespace))
            .cloned()
            .collect();
        request.facts = Some(crate::facts_for(&own, &wire_format));
    }
    request
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

/// The generator protocol in process: for every fixture, at every boundary,
/// a `Generate` request through [`protocol::handle`] answers exactly the
/// files the generator returns when called directly, and a refusal comes
/// back as an error.
pub fn the_protocol_generates_what_the_generator_does(g: &Generator) {
    let mut registry = Registry::new();
    (g.register)(&mut registry);
    for fx in corpus() {
        for version in g.boundaries() {
            let req = request(g, &fx, version);
            let bytes = protocol::encode(&protocol::Request::generate(req.clone())).unwrap();
            let response: protocol::Response =
                protocol::decode(&protocol::handle(&registry, &bytes)).unwrap();
            let at = format!("{} / {}#{version}", fx.name, g.language);
            match ((g.generate)(&req), response) {
                (Ok(files), protocol::Response::Generate(generated)) => {
                    let direct: Vec<protocol::FileOut> =
                        files.into_iter().map(Into::into).collect();
                    assert_eq!(generated.files, direct, "{at}");
                }
                (Err(e), protocol::Response::Error { message }) => {
                    assert_eq!(message, format!("{e:#}"), "{at}")
                }
                (direct, response) => panic!(
                    "{at}: directly {:?}, through the protocol {response:?}",
                    direct.map(|f| f.len())
                ),
            }
        }
    }
}

/// Runs `g` over every fixture, at each of its version boundaries, and diffs
/// against the checked-in golden output under `root`
/// (`<root>/<fixture>/<lang>[@<boundary>]/<path>`). `CONFORMANCE_BLESS=1`
/// regenerates them.
pub fn corpus_matches_golden(g: &Generator, root: &Path) {
    let bless = blessing();
    let mut drift = Vec::new();

    for fx in corpus() {
        // What the boundary before generated, which a boundary whose
        // output is the same doesn't store again.
        let mut previous: Option<Vec<GeneratedFile>> = None;
        for version in g.boundaries() {
            let at = format!("{} / {}#{version}", fx.name, g.language);
            let dir = golden_dir(root, g, fx.name, version);
            let floor = g.floor(version);

            // A fixture using a feature this generator claims not to
            // support here must be refused - and so has nothing to
            // compare.
            if refused(g, &fx, version) {
                if bless {
                    let _ = fs::remove_dir_all(&dir);
                } else if !existing_goldens(&dir).is_empty() {
                    drift.push(format!(
                        "{at}: refused, but has goldens - run with CONFORMANCE_BLESS=1"
                    ));
                }
                previous = None;
                continue;
            }

            let files = (g.generate)(&request(g, &fx, version))
                .unwrap_or_else(|e| panic!("{at}: the generator failed: {e}"));
            // A later boundary stores its own goldens only where its
            // output differs from the boundary before.
            let carried = !floor
                && previous
                    .as_ref()
                    .is_some_and(|p| same_files(p, &files, &[]));

            if bless {
                // Clear first: a file the generator no longer emits must
                // not survive as a golden.
                let _ = fs::remove_dir_all(&dir);
                if !carried {
                    for file in &files {
                        let path = dir.join(&file.path);
                        fs::create_dir_all(path.parent().unwrap()).unwrap();
                        fs::write(&path, &file.contents).unwrap();
                    }
                }
                previous = Some(files);
                continue;
            }

            if carried {
                if !existing_goldens(&dir).is_empty() {
                    drift.push(format!(
                        "{at}: generates the same as the boundary before, but has goldens of its own - \
                         run with CONFORMANCE_BLESS=1"
                    ));
                }
                previous = Some(files);
                continue;
            }

            for file in &files {
                let path = dir.join(&file.path);
                match fs::read_to_string(&path) {
                    Ok(want) if file.contents == want => {}
                    Ok(want) => drift.push(format!(
                        "--- {at} / {} ---\n{}",
                        file.path.display(),
                        line_diff(&want, &file.contents)
                    )),
                    Err(_) => drift.push(format!(
                        "{at}: no golden at {} — run with CONFORMANCE_BLESS=1",
                        path.display()
                    )),
                }
            }

            let emitted: Vec<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
            for stale in existing_goldens(&dir) {
                if !emitted.contains(&stale) {
                    drift.push(format!(
                        "{at}: golden `{}` is no longer emitted — run with CONFORMANCE_BLESS=1",
                        stale.display()
                    ));
                }
            }
            previous = Some(files);
        }
    }

    // A golden *outside* any (language, boundary) directory. The sweep above
    // can't see these: when fixtures were one schema the layout was
    // `golden/<fixture>/<lang>.<ext>`, and moving to `<lang>/<path>` left the
    // old files tracked and invisible; a boundary a generator dropped leaves
    // `<lang>@<version>/` behind. Anything at the wrong level is reported
    // rather than ignored.
    let known: Vec<String> = g
        .boundaries()
        .into_iter()
        .map(|v| {
            if g.floor(v) {
                g.language.to_string()
            } else {
                format!("{}@{v}", g.language)
            }
        })
        .collect();
    for fx in corpus() {
        let dir = root.join(fx.name);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            if !path.is_dir() || !known.contains(&name) {
                drift.push(format!(
                    "{}: `{name}` is not a (language, boundary) directory — a stale golden from an \
                     older layout, or a boundary that's gone?",
                    fx.name,
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

/// The proof behind `g`'s `SUPPORT` claims, against the corpus.
/// For each feature:
/// - the table states it, once;
/// - **supported / partial**: some fixture uses it and generates (its golden
///   is checked by [`corpus_matches_golden`]); partial says what's missing;
/// - **ignored**: some fixture uses it and generates, and every fixture
///   using it generates exactly what it would with the feature stripped;
/// - **unsupported**: some fixture uses it, and every fixture using it is
///   refused with an error naming the feature.
///
/// It also writes the report - `FEATURES.md` and `features.json` in
/// `reports` - from what it proved; `CONFORMANCE_BLESS=1` regenerates them,
/// and a stale one fails here like a stale golden.
pub fn support_is_proven(g: &Generator, reports: &Path) {
    let corpus = corpus();
    let mut failures = Vec::new();
    let mut evidence: BTreeMap<(String, Feature), Vec<String>> = BTreeMap::new();

    let lang = g.language;
    let table = g.support;
    for problem in g.versions.problems(table) {
        failures.push(format!("{lang}: {problem}"));
    }

    for &feature in Feature::ALL {
        let entries = table.iter().filter(|(f, _)| *f == feature).count();
        if entries != 1 {
            failures.push(format!(
                "{lang}: `{}` is stated {entries} times, not once",
                feature.id()
            ));
            continue;
        }
    }

    for &feature in Feature::ALL {
        let claim = features::support_for(table, feature);
        let using: Vec<&Fixture> = corpus
            .iter()
            .filter(|fx| {
                features::used(&fx.schemas, &options(fx))
                    .iter()
                    .any(|u| u.feature == feature)
            })
            .collect();
        if using.is_empty() {
            failures.push(format!(
                "{lang}: `{}` is claimed {}, but no fixture uses it",
                feature.id(),
                claim.state.id()
            ));
            continue;
        }

        let mut all_proven: Vec<String> = Vec::new();
        for version in g.boundaries() {
            let mut proven = Vec::new();
            let claim = features::support_at(table, feature, &parsed(version));
            let failed_before = failures.len();
            match claim.state {
                State::Supported | State::Partial => {
                    if claim.state == State::Partial && claim.note.is_none() {
                        failures.push(format!(
                            "{lang}#{version}: `{}` is partial without saying what's missing",
                            feature.id()
                        ));
                    }
                    // A feature that can be taken out must make a difference
                    // somewhere: generating the same without it is ignoring it.
                    let mut differs = false;
                    for fx in &using {
                        if refused(g, fx, version) {
                            continue;
                        }
                        match (g.generate)(&request(g, fx, version)) {
                            Ok(with) => {
                                proven.push(fx.name.to_string());
                                if let Some((schemas, stripped)) =
                                    features::strip(feature, &fx.schemas, &options(fx))
                                {
                                    let hashes = ir_hashes(&fx.schemas, &schemas);
                                    match (g.generate)(&request_for(
                                        g,
                                        &schemas,
                                        &fx.dependencies,
                                        &stripped,
                                        version,
                                    )) {
                                        Ok(without) => {
                                            differs |= !same_files(&with, &without, &hashes)
                                        }
                                        Err(_) => differs = true,
                                    }
                                } else {
                                    differs = true;
                                }
                            }
                            Err(e) => failures.push(format!(
                                "{lang}#{version}: `{}` fails `{}`: {e}",
                                fx.name,
                                feature.id()
                            )),
                        }
                    }
                    if !proven.is_empty() && !differs {
                        failures.push(format!(
                            "{lang}#{version}: `{}` is claimed {}, but every fixture using it generates the same without it \
                             - it's ignored",
                            feature.id(),
                            claim.state.id()
                        ));
                    }
                }
                State::Ignored => {
                    for fx in &using {
                        if refused(g, fx, version) {
                            continue;
                        }
                        let Some((schemas, stripped)) =
                            features::strip(feature, &fx.schemas, &options(fx))
                        else {
                            failures.push(format!(
                                "{lang}#{version}: `{}` is claimed ignored, but it can't be stripped to prove it",
                                feature.id()
                            ));
                            break;
                        };
                        let with = (g.generate)(&request(g, fx, version));
                        let without = (g.generate)(&request_for(
                            g,
                            &schemas,
                            &fx.dependencies,
                            &stripped,
                            version,
                        ));
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
                                "{lang}#{version}: `{}` is claimed ignored, but `{}` generates differently without it",
                                feature.id(),
                                fx.name
                            )),
                            (Err(e), _) | (_, Err(e)) => {
                                failures.push(format!("{lang}#{version}: `{}` fails `{}`: {e}", fx.name, feature.id()))
                            }
                        }
                    }
                }
                State::Unsupported => {
                    for fx in &using {
                        match (g.generate)(&request(g, fx, version)) {
                            Err(e) if e.to_string().contains(&format!("({})", feature.id())) => {
                                proven.push(fx.name.to_string())
                            }
                            Err(e) => failures.push(format!(
                                "{lang}#{version}: `{}` is refused without naming `{}`: {e}",
                                fx.name,
                                feature.id()
                            )),
                            Ok(_) => failures.push(format!(
                                "{lang}#{version}: `{}` is claimed unsupported, but `{}` generates anyway",
                                feature.id(),
                                fx.name
                            )),
                        }
                    }
                }
            }
            if proven.is_empty() && failures.len() == failed_before {
                failures.push(format!(
                    "{lang}#{version}: `{}` is claimed {}, but every fixture using it is refused for another feature",
                    feature.id(),
                    claim.state.id()
                ));
            }
            for name in proven {
                if !all_proven.contains(&name) {
                    all_proven.push(name);
                }
            }
        }
        evidence.insert((lang.to_string(), feature), all_proven);
    }

    // The report, from what was proven.
    let columns = [Column {
        generator: g.language,
        versions: g.versions,
        support: g.support,
    }];
    let files = [
        (
            reports.join("FEATURES.md"),
            format!(
                "# Generator features\n\n\
                 <!-- Generated by `comline-conformance` (`support_is_proven`) - don't edit by hand. \
                 Regenerate with `CONFORMANCE_BLESS=1 cargo test`. -->\n\n\
                 What the {lang} generator implements, and the conformance fixtures that prove it. \
                 A claim is checked, not trusted: a supported feature has a fixture that generates \
                 and matches its golden, an ignored one generates exactly what it would without the \
                 feature, and an unsupported one is refused with an error naming every use.\n\n{}",
                features::markdown(&columns, &evidence)
            ),
        ),
        (
            reports.join("features.json"),
            serde_json::to_string_pretty(&features::json(&columns, &evidence)).unwrap() + "\n",
        ),
    ];
    for (path, contents) in &files {
        if blessing() {
            fs::write(path, contents).unwrap();
        } else if fs::read_to_string(path).ok().as_deref() != Some(contents.as_str()) {
            failures.push(format!(
                "`{}` is out of date - run with CONFORMANCE_BLESS=1",
                path.display()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "unproven feature support ({}):\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// Every schema's IR hash in `a` and `b`, and each package's connection
/// hash, to mask when comparing their output.
fn ir_hashes(a: &[(String, Vec<FrozenUnit>)], b: &[(String, Vec<FrozenUnit>)]) -> Vec<u64> {
    let mut hashes: Vec<u64> = a
        .iter()
        .chain(b)
        .map(|(_, units)| schema_ir_hash(units))
        .collect();
    hashes.extend([
        connections::connection_ir_hash(a),
        connections::connection_ir_hash(b),
    ]);
    hashes
}

/// The same files with the same contents, in any order, once each of
/// `hashes` is masked out.
fn same_files(a: &[GeneratedFile], b: &[GeneratedFile], hashes: &[u64]) -> bool {
    let mask = |contents: &str| {
        hashes.iter().fold(contents.to_string(), |text, h| {
            text.replace(&format!("{h:016x}"), "<ir-hash>")
                .replace(&format!("{h:x}"), "<ir-hash>")
        })
    };
    let key = |files: &[GeneratedFile]| {
        let mut v: Vec<(PathBuf, String)> = files
            .iter()
            .map(|f| (f.path.clone(), mask(&f.contents)))
            .collect();
        v.sort();
        v
    };
    key(a) == key(b)
}
