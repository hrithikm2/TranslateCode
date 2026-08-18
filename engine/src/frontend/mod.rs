mod common;

pub mod dart;
pub mod go;
pub mod java;
pub mod javascript;
pub mod python;
pub mod rust;
pub mod swift;

use crate::typed_ir::CompilationUnit;
use crate::Language;

pub trait Frontend {
    fn parse(&self, source: &str) -> CompilationUnit;
}

pub fn parse_source(source: &str, language: Language) -> CompilationUnit {
    match language {
        Language::JavaScript => javascript::JavaScriptFrontend.parse(source),
        Language::Java => java::JavaFrontend.parse(source),
        Language::Dart => dart::DartFrontend.parse(source),
        Language::Swift => swift::SwiftFrontend.parse(source),
        Language::Python => python::PythonFrontend.parse(source),
        Language::Go => go::GoFrontend.parse(source),
        Language::Rust => rust::RustFrontend.parse(source),
    }
}
