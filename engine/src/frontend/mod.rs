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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_collection_apis_lower_through_shared_ir_for_all_sources() {
        let fixtures = [
            (
                Language::JavaScript,
                "function hasValue(values, key) { return values.has(key); }",
                None,
            ),
            (
                Language::Java,
                "import java.util.Map; final class Demo { static boolean hasValue(Map<Integer, Integer> values, int key) { return values.containsKey(key); } }",
                Some("java.util.Map"),
            ),
            (
                Language::Dart,
                "import 'dart:collection'; bool hasValue(HashMap<int, int> values, int key) { return values.containsKey(key); }",
                Some("dart:collection"),
            ),
            (
                Language::Swift,
                "import Foundation\nfunc hasValue(_ values: [Int: Int], _ key: Int) -> Bool { return values.keys.contains(key) }",
                Some("Foundation"),
            ),
            (
                Language::Python,
                "from collections import defaultdict\ndef has_value(values: dict[int, int], key: int) -> bool:\n    return key in values\n",
                Some("collections"),
            ),
            (
                Language::Go,
                "package demo\nimport \"container/list\"\nfunc count(values *list.List) int { return values.Len() }",
                Some("container/list"),
            ),
            (
                Language::Rust,
                "use std::collections::HashMap; fn has_value(values: HashMap<i64, i64>, key: i64) -> bool { values.contains_key(&key) }",
                Some("std::collections"),
            ),
        ];

        for (language, source, collection_import) in fixtures {
            let unit = parse_source(source, language);
            assert!(
                unit.diagnostics
                    .iter()
                    .all(|value| value.severity != crate::diagnostic::Severity::Error),
                "{language:?}: {:#?}",
                unit.diagnostics
            );
            if collection_import.is_some() {
                assert!(
                    unit.imports.iter().any(|value| {
                        crate::collection_ir::is_standard_collection_import(&value.uri)
                    }),
                    "{language:?}: {:#?}",
                    unit.imports
                );
            }
            if language != Language::Go {
                let debug = format!("{unit:#?}");
                assert!(
                    debug.contains("IntrinsicCall"),
                    "{language:?} did not lower collection membership:\n{debug}"
                );
            }
        }
    }
}
