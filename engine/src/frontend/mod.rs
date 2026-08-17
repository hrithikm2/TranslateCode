pub mod dart;

use crate::typed_ir::CompilationUnit;

pub trait Frontend {
    fn parse(&self, source: &str) -> CompilationUnit;
}
