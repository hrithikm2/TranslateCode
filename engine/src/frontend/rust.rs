use tree_sitter::Node;

use crate::frontend::{common, Frontend};
use crate::typed_ir::{
    Body, BodyKind, ClassDeclaration, ClassKind, ClassMember, CompilationUnit, Declaration,
    EnumDeclaration, FieldDeclaration, ImportDeclaration, TypeAliasDeclaration, TypeReference,
};

pub struct RustFrontend;

impl Frontend for RustFrontend {
    fn parse(&self, source: &str) -> CompilationUnit {
        let language = common::AstLanguage::Rust;
        let tree = match common::syntax_tree(source, language) {
            Ok(tree) => tree,
            Err(unit) => return unit,
        };
        let root = tree.root_node();
        let mut unit = CompilationUnit::default();
        unit.comments = common::collect_comments(root, source);
        common::collect_syntax_errors(root, source, language, &mut unit.diagnostics);
        let nodes = common::direct_named_children(root);
        for node in nodes.iter().copied() {
            match node.kind() {
                "use_declaration" => unit.imports.push(lower_use(node, source)),
                "struct_item" => unit
                    .declarations
                    .push(Declaration::Class(lower_struct(node, source))),
                "trait_item" => unit
                    .declarations
                    .push(Declaration::Class(lower_trait(node, source))),
                "enum_item" => unit
                    .declarations
                    .push(Declaration::Enum(lower_enum(node, source))),
                "type_item" => unit
                    .declarations
                    .push(Declaration::TypeAlias(lower_alias(node, source))),
                "function_item" => {
                    unit.declarations
                        .push(Declaration::Function(common::lower_function(
                            node, source, language,
                        )))
                }
                _ => {}
            }
        }
        for node in nodes {
            if node.kind() == "impl_item" {
                attach_impl(node, source, &mut unit);
            }
        }
        unit
    }
}

fn lower_use(node: Node<'_>, source: &str) -> ImportDeclaration {
    let argument = node
        .child_by_field_name("argument")
        .or_else(|| common::first_named_child(node));
    let raw = argument
        .map(|value| common::text(value, source))
        .unwrap_or_default();
    let uri = raw
        .split("::{")
        .next()
        .unwrap_or(raw)
        .trim_end_matches(';')
        .into();
    let show = common::find_first(node, "use_list")
        .map(|list| {
            common::direct_named_children(list)
                .into_iter()
                .map(|value| common::text(value, source).into())
                .collect()
        })
        .unwrap_or_default();
    ImportDeclaration {
        uri,
        prefix: None,
        show,
        hide: Vec::new(),
        span: common::span(node),
    }
}

fn lower_struct(node: Node<'_>, source: &str) -> ClassDeclaration {
    let mut members = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        for field in common::direct_named_children(body) {
            if field.kind() != "field_declaration" {
                continue;
            }
            if let Some(name) = field.child_by_field_name("name") {
                members.push(ClassMember::Field(FieldDeclaration {
                    name: common::text(name, source).into(),
                    type_ref: field
                        .child_by_field_name("type")
                        .map(|value| common::lower_type(value, source, common::AstLanguage::Rust))
                        .unwrap_or_else(TypeReference::dynamic),
                    span: common::span(field),
                    ..FieldDeclaration::default()
                }));
            }
        }
    }
    ClassDeclaration {
        name: common::field_text(node, "name", source).unwrap_or_default(),
        kind: ClassKind::Class,
        members,
        span: common::span(node),
        ..ClassDeclaration::default()
    }
}

fn lower_trait(node: Node<'_>, source: &str) -> ClassDeclaration {
    let mut declaration = ClassDeclaration {
        name: common::field_text(node, "name", source).unwrap_or_default(),
        kind: ClassKind::Interface,
        span: common::span(node),
        ..ClassDeclaration::default()
    };
    if let Some(body) = node.child_by_field_name("body") {
        for child in common::direct_named_children(body) {
            if child.kind() == "function_signature_item" || child.kind() == "function_item" {
                declaration
                    .members
                    .push(ClassMember::Method(common::lower_function(
                        child,
                        source,
                        common::AstLanguage::Rust,
                    )));
            }
        }
    }
    declaration
}

fn lower_enum(node: Node<'_>, source: &str) -> EnumDeclaration {
    let values = node
        .child_by_field_name("body")
        .map(|body| {
            common::direct_named_children(body)
                .into_iter()
                .filter(|child| child.kind() == "enum_variant")
                .filter_map(|variant| common::field_text(variant, "name", source))
                .collect()
        })
        .unwrap_or_default();
    EnumDeclaration {
        name: common::field_text(node, "name", source).unwrap_or_default(),
        values,
        span: common::span(node),
    }
}

fn lower_alias(node: Node<'_>, source: &str) -> TypeAliasDeclaration {
    TypeAliasDeclaration {
        name: common::field_text(node, "name", source).unwrap_or_default(),
        aliased_type: node
            .child_by_field_name("type")
            .map(|value| common::lower_type(value, source, common::AstLanguage::Rust))
            .unwrap_or_else(TypeReference::dynamic),
        span: common::span(node),
        ..TypeAliasDeclaration::default()
    }
}

fn attach_impl(node: Node<'_>, source: &str, unit: &mut CompilationUnit) {
    let Some(type_node) = node.child_by_field_name("type") else {
        return;
    };
    let type_name = common::text(type_node, source)
        .split('<')
        .next()
        .unwrap_or_default()
        .trim();
    let Some(class) = unit
        .declarations
        .iter_mut()
        .find_map(|declaration| match declaration {
            Declaration::Class(value) if value.name == type_name => Some(value),
            _ => None,
        })
    else {
        return;
    };
    if let Some(trait_node) = node.child_by_field_name("trait") {
        class.implements.push(common::lower_type(
            trait_node,
            source,
            common::AstLanguage::Rust,
        ));
    }
    if let Some(body) = node.child_by_field_name("body") {
        for member in common::direct_named_children(body) {
            match member.kind() {
                "function_item" => class
                    .members
                    .push(ClassMember::Method(common::lower_function(
                        member,
                        source,
                        common::AstLanguage::Rust,
                    ))),
                "const_item" | "static_item" => {
                    if let Some(field) = lower_associated_field(member, source) {
                        class.members.push(ClassMember::Field(field));
                    }
                }
                _ => class.members.push(ClassMember::Unlowered {
                    syntax_kind: member.kind().into(),
                    span: common::span(member),
                }),
            }
        }
    }
}

fn lower_associated_field(node: Node<'_>, source: &str) -> Option<FieldDeclaration> {
    let name = node.child_by_field_name("name")?;
    let initializer = node.child_by_field_name("value").map(|value| Body {
        kind: BodyKind::Expression(common::lower_expression(
            value,
            source,
            common::AstLanguage::Rust,
        )),
        source: common::text(value, source).into(),
        syntax_kind: value.kind().into(),
        span: common::span(value),
    });
    Some(FieldDeclaration {
        name: common::text(name, source).into(),
        type_ref: node
            .child_by_field_name("type")
            .map(|value| common::lower_type(value, source, common::AstLanguage::Rust))
            .unwrap_or_else(TypeReference::dynamic),
        is_static: true,
        is_final: node.kind() == "const_item",
        initializer,
        span: common::span(node),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPREHENSIVE: &str = include_str!("../../tests/fixtures/comprehensive.rs");

    #[test]
    fn preserves_uses_enums_structs_fields_and_impl_members() {
        let unit = RustFrontend.parse(COMPREHENSIVE);
        assert!(unit.diagnostics.is_empty(), "{:#?}", unit.diagnostics);
        assert_eq!(unit.imports[0].uri, "std::collections");
        assert_eq!(unit.imports[0].show, vec!["HashMap", "HashSet"]);
        assert!(unit.declarations.iter().any(
            |value| matches!(value, Declaration::Enum(value) if value.values == ["Fast", "Safe"])
        ));
        assert!(unit.declarations.iter().any(
            |value| matches!(value, Declaration::Class(value) if value.name == "Solvable" && value.kind == ClassKind::Interface)
        ));
        let solver = unit
            .declarations
            .iter()
            .find_map(|value| match value {
                Declaration::Class(value) if value.name == "Solver" => Some(value),
                _ => None,
            })
            .expect("Solver struct missing");
        assert!(solver.members.iter().any(|value| matches!(value, ClassMember::Field(value) if value.name == "values" && value.type_ref.name == "List")));
        assert!(solver.members.iter().any(|value| matches!(value, ClassMember::Field(value) if value.name == "VERSION" && value.is_static)));
        assert!(solver
            .members
            .iter()
            .any(|value| matches!(value, ClassMember::Method(value) if value.name == "new")));
        assert!(solver
            .members
            .iter()
            .any(|value| matches!(value, ClassMember::Method(value) if value.name == "solve")));
        assert_eq!(
            solver.implements.first().map(|value| value.name.as_str()),
            Some("Solvable")
        );
    }
}
