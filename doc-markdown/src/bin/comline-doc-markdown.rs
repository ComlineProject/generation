//! The Markdown renderer as its own process: one CBOR `Request` on stdin,
//! one CBOR `Response` on stdout, behind the generator protocol. The same
//! renderer the CLI links, so it also builds for `wasm32-wasip1`.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut registry = comline_codegen::Registry::new();
    comline_doc_markdown::register(&mut registry);
    comline_codegen::protocol::serve_stdio(&registry)
}
