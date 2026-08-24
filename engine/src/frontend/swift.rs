use tree_sitter::Node;

use crate::frontend::{common, Frontend};
use crate::typed_ir::{
    Body, BodyKind, ClassDeclaration, ClassKind, ClassMember, CompilationUnit,
    ConstructorDeclaration, Declaration, EnumDeclaration, FieldDeclaration, ImportDeclaration,
    TypeReference,
};

pub struct SwiftFrontend;

impl Frontend for SwiftFrontend {
    fn parse(&self, source: &str) -> CompilationUnit {
        let language = common::AstLanguage::Swift;
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
                "class_declaration" => {
                    if common::find_first(node, "enum_class_body").is_some() {
                        unit.declarations
                            .push(Declaration::Enum(lower_enum(node, source)));
                    } else {
                        unit.declarations
                            .push(Declaration::Class(lower_class(node, source)));
                    }
                }
                "protocol_declaration" => unit
                    .declarations
                    .push(Declaration::Class(lower_protocol(node, source))),
                "function_declaration" => unit
                    .declarations
                    .push(Declaration::Function(lower_function(node, source))),
                _ => {}
            }
        }
        unit
    }
}

fn lower_import(node: Node<'_>, source: &str) -> ImportDeclaration {
    let uri = common::find_first(node, "simple_identifier")
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

fn lower_enum(node: Node<'_>, source: &str) -> EnumDeclaration {
    let values = common::find_first(node, "enum_class_body")
        .map(|body| {
            common::direct_named_children(body)
                .into_iter()
                .filter(|child| child.kind() == "enum_entry")
                .filter_map(|entry| common::field_text(entry, "name", source))
                .collect()
        })
        .unwrap_or_default();
    EnumDeclaration {
        name: common::field_text(node, "name", source).unwrap_or_default(),
        values,
        span: common::span(node),
    }
}

fn lower_protocol(node: Node<'_>, source: &str) -> ClassDeclaration {
    let mut members = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        for member in common::direct_named_children(body) {
            if member.kind() == "protocol_function_declaration" {
                members.push(ClassMember::Method(lower_function(member, source)));
            }
        }
    }
    ClassDeclaration {
        name: common::field_text(node, "name", source).unwrap_or_default(),
        kind: ClassKind::Interface,
        members,
        span: common::span(node),
        ..ClassDeclaration::default()
    }
}

fn lower_class(node: Node<'_>, source: &str) -> ClassDeclaration {
    let header = common::text(node, source)
        .split('{')
        .next()
        .unwrap_or_default();
    let kind = if header.contains("final class") {
        ClassKind::Final
    } else {
        ClassKind::Class
    };
    let inherited = common::direct_named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "inheritance_specifier")
        .filter_map(|specifier| specifier.child_by_field_name("inherits_from"))
        .map(|value| common::lower_type(value, source, common::AstLanguage::Swift))
        .collect::<Vec<_>>();
    let mut members = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        for member in common::direct_named_children(body) {
            match member.kind() {
                "property_declaration" => {
                    if let Some(field) = lower_field(member, source) {
                        members.push(ClassMember::Field(field));
                    }
                }
                "init_declaration" => members.push(ClassMember::Constructor(lower_constructor(
                    member,
                    source,
                    &common::field_text(node, "name", source).unwrap_or_default(),
                ))),
                "function_declaration" => {
                    members.push(ClassMember::Method(lower_function(member, source)))
                }
                "deinit_declaration" => {}
                _ => members.push(ClassMember::Unlowered {
                    syntax_kind: member.kind().into(),
                    span: common::span(member),
                }),
            }
        }
    }
    ClassDeclaration {
        name: common::field_text(node, "name", source).unwrap_or_default(),
        kind,
        implements: inherited,
        members,
        span: common::span(node),
        ..ClassDeclaration::default()
    }
}

fn lower_field(node: Node<'_>, source: &str) -> Option<FieldDeclaration> {
    let pattern = node.child_by_field_name("name")?;
    let name = pattern
        .child_by_field_name("bound_identifier")
        .or_else(|| common::first_named_child(pattern))?;
    let type_ref = common::find_first(node, "type_annotation")
        .and_then(common::last_named_child)
        .map(|value| common::lower_type(value, source, common::AstLanguage::Swift))
        .unwrap_or_else(TypeReference::dynamic);
    let initializer = node.child_by_field_name("value").map(|value| Body {
        kind: BodyKind::Expression(common::lower_expression(
            value,
            source,
            common::AstLanguage::Swift,
        )),
        source: common::text(value, source).into(),
        syntax_kind: value.kind().into(),
        span: common::span(value),
    });
    let header = common::text(node, source).trim_start();
    Some(FieldDeclaration {
        name: common::text(name, source).into(),
        type_ref,
        is_static: header.starts_with("static ") || header.starts_with("class "),
        is_final: header.contains(" let ")
            || header.starts_with("let ")
            || header.starts_with("static let "),
        initializer,
        span: common::span(node),
    })
}

fn lower_constructor(node: Node<'_>, source: &str, class_name: &str) -> ConstructorDeclaration {
    let body = node.child_by_field_name("body").map(|body| Body {
        kind: BodyKind::Block(common::lower_block(
            body,
            source,
            common::AstLanguage::Swift,
        )),
        source: common::text(body, source).into(),
        syntax_kind: body.kind().into(),
        span: common::span(body),
    });
    ConstructorDeclaration {
        class_name: class_name.into(),
        parameters: common::lower_parameters(node, source, common::AstLanguage::Swift),
        body,
        source: common::text(node, source).into(),
        span: common::span(node),
        ..ConstructorDeclaration::default()
    }
}

fn lower_function(node: Node<'_>, source: &str) -> crate::typed_ir::FunctionDeclaration {
    let mut function = common::lower_function(node, source, common::AstLanguage::Swift);
    let parameters = common::direct_named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "parameter")
        .collect::<Vec<_>>();
    let mut cursor = node.walk();
    for default in node.children_by_field_name("default_value", &mut cursor) {
        if let Some(index) = parameters
            .iter()
            .rposition(|parameter| parameter.start_byte() < default.start_byte())
        {
            if let Some(parameter) = function.parameters.get_mut(index) {
                parameter.default_value = Some(common::lower_expression(
                    default,
                    source,
                    common::AstLanguage::Swift,
                ));
            }
        }
    }
    function.is_async = common::text(node, source).contains(" async ");
    function.is_static = common::text(node, source)
        .trim_start()
        .starts_with("static ");
    function
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPREHENSIVE: &str = include_str!("../../tests/fixtures/comprehensive.swift");

    #[test]
    fn preserves_imports_enums_protocols_classes_fields_initializers_and_methods() {
        let unit = SwiftFrontend.parse(COMPREHENSIVE);
        assert!(unit.diagnostics.is_empty(), "{:#?}", unit.diagnostics);
        assert_eq!(unit.imports[0].uri, "Foundation");
        assert!(unit.declarations.iter().any(
            |value| matches!(value, Declaration::Enum(value) if value.values == ["fast", "safe"])
        ));
        assert!(unit.declarations.iter().any(|value| matches!(value, Declaration::Class(value) if value.kind == ClassKind::Interface && value.name == "Solvable")));
        let solver = unit
            .declarations
            .iter()
            .find_map(|value| match value {
                Declaration::Class(value) if value.name == "Solver" => Some(value),
                _ => None,
            })
            .expect("Solver class missing");
        assert_eq!(solver.kind, ClassKind::Final);
        assert_eq!(
            solver.implements.first().map(|value| value.name.as_str()),
            Some("Solvable")
        );
        assert_eq!(
            solver
                .members
                .iter()
                .filter(|value| matches!(value, ClassMember::Field(_)))
                .count(),
            2
        );
        assert!(solver.members.iter().any(
            |value| matches!(value, ClassMember::Constructor(value) if value.parameters.len() == 1)
        ));
        assert!(solver.members.iter().any(|value| matches!(value, ClassMember::Method(value) if value.name == "solve" && value.parameters[0].default_value.is_some())));
    }
}
