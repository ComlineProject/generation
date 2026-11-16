# Code & Library Generation

## Feature support

What each generator implements, and the conformance fixtures that prove it, is
in the generator's own repository, which checks it with the corpus and harness
here: `FEATURES.md` (also as `features.json`) in
[`comline-rust`](https://github.com/ComlineProject/comline-rust) and
[`comline-typescript`](https://github.com/ComlineProject/comline-typescript).
See [`conformance/README.md`](conformance/README.md).

## `comline.toml`

`comline-codegen`'s `config` module is the one definition of `comline.toml`:
the CLI parses and resolves it, and [`schemas/comline-toml.json`](schemas/comline-toml.json)
is generated from it for editors (published at
`https://comlineproject.github.io/docs/schemas/comline-toml/0.x.json`). A test
fails when the committed schema drifts from the types; regenerate it with
`COMLINE_BLESS=1 cargo test -p comline-codegen --test comline_toml_schema`.

## Resource links
Rust: https://crates.io/crates/abi_stable

C / C++:
- https://crates.io/crates/cbindgen,
- https://doc.rust-lang.org/nomicon/ffi.html,
- https://rust-lang.github.io/rust-bindgen/introduction.html

C#: https://crates.io/crates/csbindgen

Lua/Luau: https://crates.io/crates/mlua

Python: https://crates.io/crates/pyo3



## License

**GNU General Public License v3.0 only** ([LICENSE](LICENSE) or
<https://www.gnu.org/licenses/gpl-3.0.html>) — the `comline-codegen` contract
and the conformance corpus link `comline-core` and are part of Comline's
GPL toolchain. Copyleft covers the generators, not the code they emit
(generated bindings link only `comline-runtime`, MPL-2.0). Contributions are
GPL-3.0-only. See [`design/licensing.md`](https://github.com/ComlineProject/docs).
