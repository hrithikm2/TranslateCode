use tree_sitter::Node;

use crate::frontend::{common, Frontend};
use crate::typed_ir::{
    Body, BodyKind, ClassDeclaration, ClassKind, ClassMember, CompilationUnit,
    ConstructorDeclaration, Declaration, FieldDeclaration, ImportDeclaration, TypeReference,
};

pub struct JavaScriptFrontend;

impl Frontend for JavaScriptFrontend {
    fn parse(&self, source: &str) -> CompilationUnit {
        let language = common::AstLanguage::JavaScript;
        let tree = match common::syntax_tree(source, language) {
            Ok(tree) => tree,
            Err(unit) => return unit,
        };
        let root = tree.root_node();
        let mut unit = CompilationUnit::default();
        unit.comments = common::collect_comments(root, source);
        common::collect_syntax_errors(root, source, language, &mut unit.diagnostics);
        for node in common::direct_named_children(root) {
            lower_top_level(node, source, &mut unit);
        }
        unit
    }
}

fn lower_top_level(node: Node<'_>, source: &str, unit: &mut CompilationUnit) {
    match node.kind() {
        "import_statement" => unit.imports.push(lower_import(node, source)),
        "function_declaration" => unit
            .declarations
            .push(Declaration::Function(lower_function(node, source))),
        "class_declaration" => unit
            .declarations
            .push(Declaration::Class(lower_class(node, source))),
        "export_statement" => {
            if let Some(declaration) = node
                .child_by_field_name("declaration")
                .or_else(|| common::first_named_child(node))
            {
                lower_top_level(declaration, source, unit);
            }
        }
        "lexical_declaration"
        | "variable_declaration"
        | "expression_statement"
        | "if_statement"
        | "for_statement"
        | "for_in_statement"
        | "while_statement" => unit.top_level_statements.push(common::lower_statement(
            node,
            source,
            common::AstLanguage::JavaScript,
        )),
        _ => {}
    }
}

fn lower_import(node: Node<'_>, source: &str) -> ImportDeclaration {
    let uri = node
        .child_by_field_name("source")
        .map(|value| {
            common::text(value, source)
                .trim_matches(['\'', '"'])
                .to_string()
        })
        .unwrap_or_default();
    let mut show = Vec::new();
    let mut prefix = None;
    if let Some(clause) = common::find_first(node, "import_clause") {
        for child in common::direct_named_children(clause) {
            match child.kind() {
                "identifier" => prefix = Some(common::text(child, source).into()),
                "named_imports" => {
                    for specifier in common::direct_named_children(child) {
                        if let Some(name) = specifier.child_by_field_name("name") {
                            show.push(common::text(name, source).into());
                        }
                    }
                }
                "namespace_import" => {
                    prefix = common::find_first(child, "identifier")
                        .map(|value| common::text(value, source).into());
                }
                _ => {}
            }
        }
    }
    ImportDeclaration {
        uri,
        prefix,
        show,
        hide: Vec::new(),
        span: common::span(node),
    }
}

fn lower_class(node: Node<'_>, source: &str) -> ClassDeclaration {
    let name = common::field_text(node, "name", source).unwrap_or_default();
    let extends = common::find_first(node, "class_heritage")
        .and_then(common::first_named_child)
        .map(|value| {
            common::type_from_text(common::text(value, source), common::AstLanguage::JavaScript)
        });
    let mut members = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        for member in common::direct_named_children(body) {
            match member.kind() {
                "field_definition" | "public_field_definition" => {
                    if let Some(field) = lower_field(member, source) {
                        members.push(ClassMember::Field(field));
                    }
                }
                "method_definition" => {
                    let member_name =
                        common::field_text(member, "name", source).unwrap_or_default();
                    if member_name == "constructor" {
                        members.push(ClassMember::Constructor(lower_constructor(
                            member, source, &name,
                        )));
                    } else {
                        let function = lower_function(member, source);
                        let header = common::text(member, source).trim_start();
                        if header.starts_with("get ") {
                            members.push(ClassMember::Getter(function));
                        } else if header.starts_with("set ") {
                            members.push(ClassMember::Setter(function));
                        } else {
                            members.push(ClassMember::Method(function));
                        }
                    }
                }
                _ => members.push(ClassMember::Unlowered {
                    syntax_kind: member.kind().into(),
                    span: common::span(member),
                }),
            }
        }
    }
    ClassDeclaration {
        name,
        kind: ClassKind::Class,
        extends,
        members,
        span: common::span(node),
        ..ClassDeclaration::default()
    }
}

fn lower_field(node: Node<'_>, source: &str) -> Option<FieldDeclaration> {
    let property = node.child_by_field_name("property")?;
    let initializer = node.child_by_field_name("value").map(|value| Body {
        kind: BodyKind::Expression(common::lower_expression(
            value,
            source,
            common::AstLanguage::JavaScript,
        )),
        source: common::text(value, source).into(),
        syntax_kind: value.kind().into(),
        span: common::span(value),
    });
    let header = common::text(node, source).trim_start();
    Some(FieldDeclaration {
        name: common::text(property, source).into(),
        type_ref: TypeReference::dynamic(),
        is_static: header.starts_with("static "),
        is_final: false,
        initializer,
        span: common::span(node),
    })
}

fn lower_constructor(node: Node<'_>, source: &str, class_name: &str) -> ConstructorDeclaration {
    let function = lower_function(node, source);
    ConstructorDeclaration {
        class_name: class_name.into(),
        parameters: function.parameters,
        body: function.body,
        source: common::text(node, source).into(),
        span: common::span(node),
        ..ConstructorDeclaration::default()
    }
}

fn lower_function(node: Node<'_>, source: &str) -> crate::typed_ir::FunctionDeclaration {
    let mut function = common::lower_function(node, source, common::AstLanguage::JavaScript);
    let header = common::text(node, source).trim_start();
    function.is_async = header.starts_with("async ") || header.contains(" async ");
    function.is_static = header.starts_with("static ");
    function
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPREHENSIVE: &str = include_str!("../../tests/fixtures/comprehensive.js");

    #[test]
    fn preserves_modules_classes_fields_constructor_and_methods() {
        let unit = JavaScriptFrontend.parse(COMPREHENSIVE);
        assert!(unit.diagnostics.is_empty(), "{:#?}", unit.diagnostics);
        assert_eq!(unit.imports.len(), 1);
        assert_eq!(unit.imports[0].uri, "node:fs");
        assert_eq!(unit.imports[0].show, vec!["readFile"]);
        let class = unit
            .declarations
            .iter()
            .find_map(|value| match value {
                Declaration::Class(value) => Some(value),
                _ => None,
            })
            .expect("Solver class missing");
        assert_eq!(class.name, "Solver");
        assert_eq!(
            class.extends.as_ref().map(|value| value.name.as_str()),
            Some("BaseSolver")
        );
        assert_eq!(
            class
                .members
                .iter()
                .filter(|value| matches!(value, ClassMember::Field(_)))
                .count(),
            2
        );
        assert!(class
            .members
            .iter()
            .any(|value| matches!(value, ClassMember::Constructor(_))));
        assert!(class.members.iter().any(
            |value| matches!(value, ClassMember::Method(function) if function.name == "solve")
        ));
        assert!(unit.declarations.iter().any(
            |value| matches!(value, Declaration::Function(function) if function.name == "main")
        ));
    }

    #[test]
    fn retains_ordered_script_statements() {
        let unit = JavaScriptFrontend.parse("let count = 0;\ncount++;\nconsole.log(count);\n");
        assert_eq!(unit.top_level_statements.len(), 3, "{unit:#?}");
        assert!(matches!(
            unit.top_level_statements[0].kind,
            crate::typed_ir::StatementKind::Variable { .. }
        ));
    }
}
