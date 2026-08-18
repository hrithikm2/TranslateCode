use tree_sitter::Node;

use crate::frontend::{common, Frontend};
use crate::typed_ir::{
    ClassDeclaration, ClassKind, ClassMember, CompilationUnit, Declaration, EnumDeclaration,
    FieldDeclaration, ImportDeclaration, TypeAliasDeclaration, TypeReference,
};

pub struct GoFrontend;

impl Frontend for GoFrontend {
    fn parse(&self, source: &str) -> CompilationUnit {
        let language = common::AstLanguage::Go;
        let tree = match common::syntax_tree(source, language) {
            Ok(tree) => tree,
            Err(unit) => return unit,
        };
        let root = tree.root_node();
        let mut unit = CompilationUnit::default();
        common::collect_syntax_errors(root, source, language, &mut unit.diagnostics);
        let nodes = common::direct_named_children(root);
        for node in nodes.iter().copied() {
            match node.kind() {
                "import_declaration" => unit.imports.extend(lower_imports(node, source)),
                "type_declaration" => lower_types(node, source, &mut unit.declarations),
                "const_declaration" => {
                    if let Some(value) = lower_enum(node, source) {
                        unit.declarations.push(Declaration::Enum(value));
                    }
                }
                "function_declaration" => {
                    unit.declarations
                        .push(Declaration::Function(common::lower_function(
                            node, source, language,
                        )))
                }
                _ => {}
            }
        }
        for node in nodes {
            if node.kind() == "method_declaration" {
                attach_method(node, source, &mut unit);
            }
        }
        unit
    }
}

fn lower_imports(node: Node<'_>, source: &str) -> Vec<ImportDeclaration> {
    let specs = if node.child_by_field_name("path").is_some() {
        vec![node]
    } else {
        let found = common::direct_named_children(node)
            .into_iter()
            .filter(|child| child.kind() == "import_spec")
            .collect::<Vec<_>>();
        if found.is_empty() {
            vec![node]
        } else {
            found
        }
    };
    specs
        .into_iter()
        .filter_map(|spec| {
            let path = spec
                .child_by_field_name("path")
                .or_else(|| common::find_first(spec, "interpreted_string_literal"))?;
            let prefix = spec
                .child_by_field_name("name")
                .map(|value| common::text(value, source).into());
            Some(ImportDeclaration {
                uri: common::text(path, source).trim_matches('"').into(),
                prefix,
                show: Vec::new(),
                hide: Vec::new(),
                span: common::span(spec),
            })
        })
        .collect()
}

fn lower_types(node: Node<'_>, source: &str, declarations: &mut Vec<Declaration>) {
    for spec in common::direct_named_children(node) {
        if !matches!(spec.kind(), "type_spec" | "type_alias") {
            continue;
        }
        let name = common::field_text(spec, "name", source).unwrap_or_default();
        let Some(type_node) = spec.child_by_field_name("type") else {
            continue;
        };
        if type_node.kind() == "struct_type" {
            declarations.push(Declaration::Class(lower_struct(
                &name, spec, type_node, source,
            )));
        } else if type_node.kind() == "interface_type" {
            declarations.push(Declaration::Class(lower_interface(
                &name, spec, type_node, source,
            )));
        } else {
            declarations.push(Declaration::TypeAlias(TypeAliasDeclaration {
                name,
                aliased_type: common::lower_type(type_node, source, common::AstLanguage::Go),
                span: common::span(spec),
                ..TypeAliasDeclaration::default()
            }));
        }
    }
}

fn lower_interface(
    name: &str,
    declaration: Node<'_>,
    type_node: Node<'_>,
    source: &str,
) -> ClassDeclaration {
    let mut members = Vec::new();
    if let Some(body) = common::find_first(type_node, "method_spec_list") {
        for method in common::direct_named_children(body) {
            if matches!(method.kind(), "method_elem" | "method_spec") {
                members.push(ClassMember::Method(common::lower_function(
                    method,
                    source,
                    common::AstLanguage::Go,
                )));
            }
        }
    }
    ClassDeclaration {
        name: name.into(),
        kind: ClassKind::Interface,
        members,
        span: common::span(declaration),
        ..ClassDeclaration::default()
    }
}

fn lower_struct(
    name: &str,
    declaration: Node<'_>,
    type_node: Node<'_>,
    source: &str,
) -> ClassDeclaration {
    let mut members = Vec::new();
    if let Some(fields) = common::find_first(type_node, "field_declaration_list") {
        for field in common::direct_named_children(fields) {
            if field.kind() != "field_declaration" {
                continue;
            }
            let type_ref = field
                .child_by_field_name("type")
                .map(|value| common::lower_type(value, source, common::AstLanguage::Go))
                .unwrap_or_else(TypeReference::dynamic);
            let mut cursor = field.walk();
            for field_name in field.children_by_field_name("name", &mut cursor) {
                members.push(ClassMember::Field(FieldDeclaration {
                    name: common::text(field_name, source).into(),
                    type_ref: type_ref.clone(),
                    span: common::span(field_name),
                    ..FieldDeclaration::default()
                }));
            }
        }
    }
    ClassDeclaration {
        name: name.into(),
        kind: ClassKind::Class,
        members,
        span: common::span(declaration),
        ..ClassDeclaration::default()
    }
}

fn attach_method(node: Node<'_>, source: &str, unit: &mut CompilationUnit) {
    let receiver_type = node
        .child_by_field_name("receiver")
        .and_then(common::first_named_child)
        .and_then(|parameter| parameter.child_by_field_name("type"))
        .map(|value| {
            common::text(value, source)
                .trim_start_matches('*')
                .to_string()
        });
    let Some(receiver_type) = receiver_type else {
        return;
    };
    let mut function = common::lower_function(node, source, common::AstLanguage::Go);
    function.is_static = false;
    if let Some(class) = unit
        .declarations
        .iter_mut()
        .find_map(|declaration| match declaration {
            Declaration::Class(value) if value.name == receiver_type => Some(value),
            _ => None,
        })
    {
        class.members.push(ClassMember::Method(function));
    }
}

fn lower_enum(node: Node<'_>, source: &str) -> Option<EnumDeclaration> {
    let specs = common::direct_named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "const_spec")
        .collect::<Vec<_>>();
    let enum_name = specs
        .iter()
        .find_map(|spec| spec.child_by_field_name("type"))
        .map(|value| common::text(value, source).into())?;
    let values = specs
        .into_iter()
        .flat_map(|spec| {
            let mut cursor = spec.walk();
            spec.children_by_field_name("name", &mut cursor)
                .map(|value| common::text(value, source).to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    Some(EnumDeclaration {
        name: enum_name,
        values,
        span: common::span(node),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPREHENSIVE: &str = include_str!("../../tests/fixtures/comprehensive.go");

    #[test]
    fn preserves_imports_aliases_structs_fields_functions_methods_and_enums() {
        let unit = GoFrontend.parse(COMPREHENSIVE);
        assert!(unit.diagnostics.is_empty(), "{:#?}", unit.diagnostics);
        assert_eq!(unit.imports[0].uri, "fmt");
        assert!(unit
            .declarations
            .iter()
            .any(|value| matches!(value, Declaration::TypeAlias(value) if value.name == "Mode")));
        assert!(unit.declarations.iter().any(
            |value| matches!(value, Declaration::Class(value) if value.name == "Solvable" && value.kind == ClassKind::Interface)
        ));
        assert!(unit.declarations.iter().any(|value| matches!(value, Declaration::Enum(value) if value.name == "Mode" && value.values == ["Fast", "Safe"])));
        let solver = unit
            .declarations
            .iter()
            .find_map(|value| match value {
                Declaration::Class(value) if value.name == "Solver" => Some(value),
                _ => None,
            })
            .expect("Solver struct missing");
        assert!(solver.members.iter().any(|value| matches!(value, ClassMember::Field(value) if value.name == "values" && value.type_ref.name == "List")));
        assert!(solver
            .members
            .iter()
            .any(|value| matches!(value, ClassMember::Method(value) if value.name == "Solve")));
        assert!(unit.declarations.iter().any(
            |value| matches!(value, Declaration::Function(value) if value.name == "NewSolver")
        ));
    }
}
