use tree_sitter::Node;

use crate::frontend::{common, Frontend};
use crate::typed_ir::{
    Body, BodyKind, ClassDeclaration, ClassKind, ClassMember, CompilationUnit,
    ConstructorDeclaration, Declaration, EnumDeclaration, FieldDeclaration, ImportDeclaration,
    TypeParameter,
};

pub struct JavaFrontend;

impl Frontend for JavaFrontend {
    fn parse(&self, source: &str) -> CompilationUnit {
        let language = common::AstLanguage::Java;
        let tree = match common::syntax_tree(source, language) {
            Ok(tree) => tree,
            Err(unit) => return unit,
        };
        let root = tree.root_node();
        let mut unit = CompilationUnit::default();
        unit.comments = common::collect_comments(root, source);
        common::collect_syntax_errors(root, source, language, &mut unit.diagnostics);
        for node in common::direct_named_children(root) {
            match node.kind() {
                "import_declaration" => unit.imports.push(lower_import(node, source)),
                "class_declaration" | "interface_declaration" | "record_declaration" => unit
                    .declarations
                    .push(Declaration::Class(lower_class(node, source))),
                "enum_declaration" => unit
                    .declarations
                    .push(Declaration::Enum(lower_enum(node, source))),
                _ => {}
            }
        }
        unit
    }
}

fn lower_import(node: Node<'_>, source: &str) -> ImportDeclaration {
    let uri = common::direct_named_children(node)
        .into_iter()
        .find(|child| matches!(child.kind(), "scoped_identifier" | "identifier"))
        .map(|value| common::text(value, source).into())
        .unwrap_or_default();
    ImportDeclaration {
        uri,
        prefix: None,
        show: Vec::new(),
        hide: Vec::new(),
        span: common::span(node),
    }
}

fn lower_class(node: Node<'_>, source: &str) -> ClassDeclaration {
    let name = common::field_text(node, "name", source).unwrap_or_default();
    let header = common::text(node, source)
        .split('{')
        .next()
        .unwrap_or_default();
    let kind = match node.kind() {
        "interface_declaration" => ClassKind::Interface,
        _ if header.contains("abstract ") => ClassKind::Abstract,
        _ if header.contains("final ") => ClassKind::Final,
        _ if header.contains("sealed ") => ClassKind::Sealed,
        _ => ClassKind::Class,
    };
    let type_parameters = common::find_first(node, "type_parameters")
        .map(|parameters| lower_type_parameters(parameters, source))
        .unwrap_or_default();
    let extends = node
        .child_by_field_name("superclass")
        .and_then(common::last_named_child)
        .map(|value| common::lower_type(value, source, common::AstLanguage::Java));
    let implements = node
        .child_by_field_name("interfaces")
        .or_else(|| common::find_first(node, "super_interfaces"))
        .map(|interfaces| {
            let owner = common::find_first(interfaces, "type_list").unwrap_or(interfaces);
            common::direct_named_children(owner)
                .into_iter()
                .map(|value| common::lower_type(value, source, common::AstLanguage::Java))
                .collect()
        })
        .unwrap_or_default();
    let body = node.child_by_field_name("body").or_else(|| {
        common::direct_named_children(node)
            .into_iter()
            .find(|child| {
                matches!(
                    child.kind(),
                    "class_body" | "interface_body" | "record_body"
                )
            })
    });
    let mut members = Vec::new();
    if let Some(body) = body {
        for member in common::direct_named_children(body) {
            match member.kind() {
                "field_declaration" | "constant_declaration" => {
                    members.extend(
                        lower_fields(member, source)
                            .into_iter()
                            .map(ClassMember::Field),
                    );
                }
                "constructor_declaration" | "compact_constructor_declaration" => {
                    members.push(ClassMember::Constructor(lower_constructor(
                        member, source, &name,
                    )));
                }
                "method_declaration" => members.push(ClassMember::Method(common::lower_function(
                    member,
                    source,
                    common::AstLanguage::Java,
                ))),
                _ => members.push(ClassMember::Unlowered {
                    syntax_kind: member.kind().into(),
                    span: common::span(member),
                }),
            }
        }
    }
    ClassDeclaration {
        name,
        kind,
        type_parameters,
        extends,
        implements,
        members,
        span: common::span(node),
        ..ClassDeclaration::default()
    }
}

fn lower_fields(node: Node<'_>, source: &str) -> Vec<FieldDeclaration> {
    let type_ref = node
        .child_by_field_name("type")
        .map(|value| common::lower_type(value, source, common::AstLanguage::Java))
        .unwrap_or_else(crate::typed_ir::TypeReference::dynamic);
    let header = common::text(node, source).trim_start();
    common::direct_named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "variable_declarator")
        .filter_map(|declarator| {
            let name = declarator.child_by_field_name("name")?;
            let initializer = declarator.child_by_field_name("value").map(|value| Body {
                kind: BodyKind::Expression(common::lower_expression(
                    value,
                    source,
                    common::AstLanguage::Java,
                )),
                source: common::text(value, source).into(),
                syntax_kind: value.kind().into(),
                span: common::span(value),
            });
            Some(FieldDeclaration {
                name: common::text(name, source).into(),
                type_ref: type_ref.clone(),
                is_static: header.contains("static "),
                is_final: header.contains("final ") || node.kind() == "constant_declaration",
                initializer,
                span: common::span(declarator),
            })
        })
        .collect()
}

fn lower_constructor(node: Node<'_>, source: &str, class_name: &str) -> ConstructorDeclaration {
    let body = node.child_by_field_name("body").map(|body| Body {
        kind: BodyKind::Block(common::lower_block(body, source, common::AstLanguage::Java)),
        source: common::text(body, source).into(),
        syntax_kind: body.kind().into(),
        span: common::span(body),
    });
    ConstructorDeclaration {
        class_name: class_name.into(),
        parameters: common::lower_parameters(node, source, common::AstLanguage::Java),
        body,
        source: common::text(node, source).into(),
        span: common::span(node),
        ..ConstructorDeclaration::default()
    }
}

fn lower_enum(node: Node<'_>, source: &str) -> EnumDeclaration {
    let mut values = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        for child in common::direct_named_children(body) {
            if child.kind() == "enum_constant" {
                if let Some(name) = common::field_text(child, "name", source) {
                    values.push(name);
                }
            }
        }
    }
    EnumDeclaration {
        name: common::field_text(node, "name", source).unwrap_or_default(),
        values,
        span: common::span(node),
    }
}

fn lower_type_parameters(node: Node<'_>, source: &str) -> Vec<TypeParameter> {
    common::direct_named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "type_parameter")
        .filter_map(|parameter| {
            let name = common::find_first(parameter, "type_identifier")?;
            let bound = parameter
                .child_by_field_name("bounds")
                .and_then(common::first_named_child)
                .map(|value| common::lower_type(value, source, common::AstLanguage::Java));
            Some(TypeParameter {
                name: common::text(name, source).into(),
                bound,
                span: common::span(parameter),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPREHENSIVE: &str = include_str!("../../tests/fixtures/comprehensive.java");

    #[test]
    fn preserves_imports_interfaces_enums_classes_and_members() {
        let unit = JavaFrontend.parse(COMPREHENSIVE);
        assert!(unit.diagnostics.is_empty(), "{:#?}", unit.diagnostics);
        assert_eq!(unit.imports.len(), 3);
        assert!(unit.declarations.iter().any(
            |value| matches!(value, Declaration::Enum(value) if value.values == ["FAST", "SAFE"])
        ));
        let interface = unit
            .declarations
            .iter()
            .find_map(|value| match value {
                Declaration::Class(value) if value.kind == ClassKind::Interface => Some(value),
                _ => None,
            })
            .expect("interface missing");
        assert_eq!(interface.type_parameters.len(), 1);
        let solver = unit
            .declarations
            .iter()
            .find_map(|value| match value {
                Declaration::Class(value) if value.name == "Solver" => Some(value),
                _ => None,
            })
            .expect("Solver missing");
        assert_eq!(solver.kind, ClassKind::Final);
        assert_eq!(
            solver.extends.as_ref().map(|value| value.name.as_str()),
            Some("BaseSolver")
        );
        assert_eq!(solver.implements.len(), 1);
        assert_eq!(
            solver
                .members
                .iter()
                .filter(|value| matches!(value, ClassMember::Field(_)))
                .count(),
            2
        );
        assert!(solver
            .members
            .iter()
            .any(|value| matches!(value, ClassMember::Constructor(_))));
        assert!(solver
            .members
            .iter()
            .any(|value| matches!(value, ClassMember::Method(value) if value.name == "solve")));
    }
}
