//! The renderer's content hash: `RENDERER.version` is the crate version plus
//! a hash of everything its output depends on (this crate's `src/` and
//! `assets/`, and the shared model helpers in `comline-codegen`), so
//! `comline doc`'s cache sees any change to the output even while the crate
//! version stays put (generation#101).

use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut paths = vec![];
    for dir in ["src", "assets"] {
        println!("cargo:rerun-if-changed={dir}");
        files(&root.join(dir), &mut paths);
    }
    // The shared helpers the pages are built with, when the crate sits in
    // its repo (a git dependency keeps the whole checkout).
    for shared in ["../codegen/src/doc_model.rs", "../codegen/src/protocol.rs"] {
        let path = root.join(shared);
        if path.exists() {
            println!("cargo:rerun-if-changed={shared}");
            paths.push(path);
        }
    }
    paths.sort();
    // FNV-1a, 64-bit: stable across Rust releases, no dependencies.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for path in &paths {
        let name = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        feed(name.as_bytes());
        feed(&[0]);
        // Line endings normalized, so a checkout's settings don't move it.
        let text = std::fs::read(path).unwrap_or_default();
        let text: Vec<u8> = text.into_iter().filter(|b| *b != b'\r').collect();
        feed(&text);
        feed(&[0]);
    }
    println!("cargo:rustc-env=COMLINE_RENDERER_HASH={hash:016x}");
}
