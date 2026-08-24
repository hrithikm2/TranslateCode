use tree_sitter::Node;

use crate::frontend::{common, Frontend};
use crate::typed_ir::{
    Body, BodyKind, ClassDeclaration, ClassKind, ClassMember, CompilationUnit,
    ConstructorDeclaration, Declaration, FieldDeclaration, ImportDeclaration, TypeAliasDeclaration,
    TypeReference,
};

pub struct PythonFrontend;

impl Frontend for PythonFrontend {
    fn parse(&self, source: &str) -> CompilationUnit {
        let language = common::AstLanguage::Python;
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
        "import_statement" | "import_from_statement" => {
            unit.imports.push(lower_import(node, source))
        }
        "function_definition" => unit
            .declarations
            .push(Declaration::Function(lower_function(node, source, false))),
        "class_definition" => unit
            .declarations
            .push(Declaration::Class(lower_class(node, source))),
        "decorated_definition" => {
            if let Some(definition) = common::direct_named_children(node)
                .into_iter()
                .find(|child| matches!(child.kind(), "function_definition" | "class_definition"))
            {
                lower_top_level(definition, source, unit);
            }
        }
        "expression_statement" => {
            if let Some(alias) = lower_type_alias(node, source) {
                unit.declarations.push(Declaration::TypeAlias(alias));
            } else {
                unit.top_level_statements.push(common::lower_statement(
                    node,
                    source,
                    common::AstLanguage::Python,
                ));
            }
        }
        "if_statement" | "for_statement" | "while_statement" | "try_statement" => {
            unit.top_level_statements.push(common::lower_statement(
                node,
                source,
                common::AstLanguage::Python,
            ))
        }
        _ => {}
    }
}

fn lower_import(node: Node<'_>, source: &str) -> ImportDeclaration {
    let uri = if node.kind() == "import_from_statement" {
        node.child_by_field_name("module_name")
            .map(|value| common::text(value, source).into())
            .unwrap_or_default()
    } else {
        common::direct_named_children(node)
            .into_iter()
            .find(|child| matches!(child.kind(), "dotted_name" | "aliased_import"))
            .map(|value| common::text(value, source).into())
            .unwrap_or_default()
    };
    let mut show = Vec::new();
    let mut prefix = None;
    if node.kind() == "import_from_statement" {
        let mut cursor = node.walk();
        for name in node.children_by_field_name("name", &mut cursor) {
            show.push(common::text(name, source).into());
        }
    } else if let Some(alias) = common::find_first(node, "aliased_import") {
        prefix = alias
            .child_by_field_name("alias")
            .map(|value| common::text(value, source).into());
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
    let bases = node
        .child_by_field_name("superclasses")
        .map(common::direct_named_children)
        .unwrap_or_default()
        .into_iter()
        .map(|value| {
            common::type_from_text(common::text(value, source), common::AstLanguage::Python)
        })
        .collect::<Vec<_>>();
    let extends = bases.first().cloned();
    let mixins = bases.into_iter().skip(1).collect();
    let mut members = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        for member in common::direct_named_children(body) {
            lower_class_member(member, source, &name, &mut members);
        }
    }
    ClassDeclaration {
        name,
        kind: ClassKind::Class,
        extends,
        mixins,
        members,
        span: common::span(node),
        ..ClassDeclaration::default()
    }
}

fn lower_class_member(
    node: Node<'_>,
    source: &str,
    class_name: &str,
    members: &mut Vec<ClassMember>,
) {
    match node.kind() {
        "function_definition" => {
            let function = lower_function(node, source, true);
            if function.name == "__init__" {
                let mut parameters = function.parameters;
                for parameter in &mut parameters {
                    if parameter.default_value.is_some() {
                        parameter.kind = crate::typed_ir::ParameterKind::Named;
                        parameter.is_required = false;
                    }
                }
                members.push(ClassMember::Constructor(ConstructorDeclaration {
                    class_name: class_name.into(),
                    parameters,
                    body: function.body,
                    source: common::text(node, source).into(),
                    span: common::span(node),
                    ..ConstructorDeclaration::default()
                }));
            } else {
                members.push(ClassMember::Method(function));
            }
        }
        "decorated_definition" => {
            if let Some(definition) = common::find_first(node, "function_definition") {
                lower_class_member(definition, source, class_name, members);
                if common::text(node, source).contains("@staticmethod") {
                    if let Some(ClassMember::Method(function)) = members.last_mut() {
                        function.is_static = true;
                    }
                }
            }
        }
        "expression_statement" => {
            if let Some(field) = lower_field(node, source) {
                members.push(ClassMember::Field(field));
            }
        }
        "pass_statement" => {}
        _ => members.push(ClassMember::Unlowered {
            syntax_kind: node.kind().into(),
            span: common::span(node),
        }),
    }
}

fn lower_function(
    node: Node<'_>,
    source: &str,
    method: bool,
) -> crate::typed_ir::FunctionDeclaration {
    let mut function = common::lower_function(node, source, common::AstLanguage::Python);
    if method && function.parameters.first().map(|value| value.name.as_str()) == Some("self") {
        function.parameters.remove(0);
    }
    function.is_async = common::text(node, source)
        .trim_start()
        .starts_with("async ");
    function
}

fn lower_field(node: Node<'_>, source: &str) -> Option<FieldDeclaration> {
    let assignment = common::find_first(node, "assignment")?;
    let left = assignment.child_by_field_name("left")?;
    if left.kind() != "identifier" {
        return None;
    }
    let type_ref = assignment
        .child_by_field_name("type")
        .map(|value| common::lower_type(value, source, common::AstLanguage::Python))
        .unwrap_or_else(TypeReference::dynamic);
    let initializer = assignment.child_by_field_name("right").map(|value| Body {
        kind: BodyKind::Expression(common::lower_expression(
            value,
            source,
            common::AstLanguage::Python,
        )),
        source: common::text(value, source).into(),
        syntax_kind: value.kind().into(),
        span: common::span(value),
    });
    Some(FieldDeclaration {
        name: common::text(left, source).into(),
        type_ref,
        is_static: true,
        is_final: false,
        initializer,
        span: common::span(node),
    })
}

fn lower_type_alias(node: Node<'_>, source: &str) -> Option<TypeAliasDeclaration> {
    let assignment = common::find_first(node, "assignment")?;
    let annotation = assignment.child_by_field_name("type")?;
    if common::text(annotation, source).trim() != "TypeAlias" {
        return None;
    }
    let name = assignment.child_by_field_name("left")?;
    let value = assignment.child_by_field_name("right")?;
    Some(TypeAliasDeclaration {
        name: common::text(name, source).into(),
        aliased_type: common::type_from_text(
            common::text(value, source),
            common::AstLanguage::Python,
        ),
        span: common::span(node),
        ..TypeAliasDeclaration::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPREHENSIVE: &str = include_str!("../../tests/fixtures/comprehensive.py");

    #[test]
    fn preserves_imports_aliases_classes_fields_constructor_and_methods() {
        let unit = PythonFrontend.parse(COMPREHENSIVE);
        assert!(unit.diagnostics.is_empty(), "{:#?}", unit.diagnostics);
        assert_eq!(unit.imports.len(), 2);
        assert!(unit.declarations.iter().any(|value| matches!(value, Declaration::TypeAlias(value) if value.name == "NumberList" && value.aliased_type.name == "List")));
        let solver = unit
            .declarations
            .iter()
            .find_map(|value| match value {
                Declaration::Class(value) if value.name == "Solver" => Some(value),
                _ => None,
            })
            .expect("Solver missing");
        assert_eq!(
            solver.extends.as_ref().map(|value| value.name.as_str()),
            Some("BaseSolver")
        );
        assert!(solver.members.iter().any(
            |value| matches!(value, ClassMember::Constructor(value) if value.parameters.len() == 1)
        ));
        assert!(solver.members.iter().any(|value| matches!(value, ClassMember::Method(value) if value.name == "solve" && value.parameters.len() == 1)));
        assert!(unit
            .declarations
            .iter()
            .any(|value| matches!(value, Declaration::Function(value) if value.name == "main")));
    }

    #[test]
    fn retains_ordered_module_statements() {
        let unit = PythonFrontend.parse("count = 0\ncount += 1\nprint(count)\n");
        assert_eq!(unit.top_level_statements.len(), 3, "{unit:#?}");
        assert!(matches!(
            unit.top_level_statements[0].kind,
            crate::typed_ir::StatementKind::Variable { .. }
        ));
    }
}
