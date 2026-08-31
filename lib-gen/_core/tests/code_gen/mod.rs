pub mod rust;
pub mod typescript;

use comline_codelib_gen::code_gen::{GenRequest, Mode, PackageMeta};
use comline_core::schema::ir::frozen::unit::FrozenUnit;

pub fn code_req(schemas: &[(String, Vec<FrozenUnit>)]) -> GenRequest<'_> {
    GenRequest {
        mode: Mode::Code,
        schemas,
        package: PackageMeta { name: "test".into(), version: "0.1.0".into() },
    }
}

pub fn lib_req(schemas: &[(String, Vec<FrozenUnit>)]) -> GenRequest<'_> {
    GenRequest {
        mode: Mode::Lib,
        schemas,
        package: PackageMeta { name: "chat".into(), version: "0.3.0".into() },
    }
}
