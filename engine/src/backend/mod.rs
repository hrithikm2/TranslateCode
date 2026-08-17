pub mod java;

use crate::diagnostic::Diagnostic;
use crate::typed_ir::CompilationUnit;

#[derive(Clone, Debug, Default)]
pub struct BackendOutput {
    pub code: String,
    pub diagnostics: Vec<Diagnostic>,
}

pub trait Backend {
    fn emit(&self, unit: &CompilationUnit) -> BackendOutput;
}
