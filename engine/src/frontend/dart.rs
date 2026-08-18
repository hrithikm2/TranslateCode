use tree_sitter::{Node, Parser};

use crate::diagnostic::{Diagnostic, Severity, SourcePosition, SourceSpan};
use crate::frontend::Frontend;
use crate::typed_ir::{
    Argument, Body, BodyKind, CatchClause, ClassDeclaration, ClassKind, ClassMember,
    CollectionElement, CompilationUnit, ConstructorDeclaration, Declaration, EnumDeclaration,
    Expression, ExpressionKind, ExtensionDeclaration, FieldDeclaration, FunctionDeclaration,
    ImportDeclaration, Literal, Parameter, ParameterKind, Pattern, PatternField, PatternKind, Statement,
    StatementKind, StringPart, SwitchCase, SwitchExpressionCase, TypeAliasDeclaration,
    TypeParameter, TypeReference,
};

pub struct DartFrontend;

impl Frontend for DartFrontend {
    fn parse(&self, source: &str) -> CompilationUnit {
        let mut parser = Parser::new();
        if parser
            .set_language(&tree_sitter_dart::LANGUAGE.into())
            .is_err()
        {
            return CompilationUnit {
                imports: Vec::new(),
                declarations: Vec::new(),
                diagnostics: vec![Diagnostic {
                    code: "DART0001",
                    severity: Severity::Error,
                    message: "Unable to initialize the Dart parser".into(),
                    span: SourceSpan::default(),
                }],
            };
        }
        let Some(tree) = parser.parse(source, None) else {
            return CompilationUnit {
                imports: Vec::new(),
                declarations: Vec::new(),
                diagnostics: vec![Diagnostic {
                    code: "DART0002",
                    severity: Severity::Error,
                    message: "The Dart parser did not produce a syntax tree".into(),
                    span: SourceSpan::default(),
                }],
            };
        };
        let root = tree.root_node();
        let mut unit = CompilationUnit::default();
        collect_syntax_errors(root, source, &mut unit.diagnostics);
        let mut cursor = root.walk();
        for node in root.named_children(&mut cursor) {
            if node.kind() == "import_or_export" {
                if node_text(node, source).trim_start().starts_with("import ") {
                    let import = lower_import(node, source);
                    if import.uri.starts_with("package:") {
                        unit.diagnostics.push(Diagnostic {
                            code: "DART2001",
                            severity: Severity::Warning,
                            message: format!(
                                "External package `{}` needs a target-language adapter",
                                import.uri
                            ),
                            span: import.span,
                        });
                    }
                    unit.imports.push(import);
                }
                continue;
            }
            if let Some(declaration) = lower_top_level_declaration(node, source) {
                unit.declarations.push(declaration);
            }
        }
        unit
    }
}

fn lower_import(node: Node<'_>, source: &str) -> ImportDeclaration {
    let uri = node
        .child_by_field_name("uri")
        .or_else(|| find_first(node, "uri"))
        .map(|value| node_text(value, source).trim_matches(['\'', '"']).to_string())
        .unwrap_or_default();
    let text = node_text(node, source);
    let prefix = text
        .split_whitespace()
        .position(|word| word == "as")
        .and_then(|index| text.split_whitespace().nth(index + 1))
        .map(|value| value.trim_end_matches(';').to_string());
    let (show, hide) = parse_combinators(&text);
    ImportDeclaration {
        uri,
        prefix,
        show,
        hide,
        span: span(node),
    }
}

fn parse_combinators(text: &str) -> (Vec<String>, Vec<String>) {
    let mut show = Vec::new();
    let mut hide = Vec::new();
    for (kind, values) in text
        .split(" show ")
        .nth(1)
        .map(|value| ("show", value))
        .into_iter()
        .chain(text.split(" hide ").nth(1).map(|value| ("hide", value)))
    {
        let names = values
            .split(';')
            .next()
            .unwrap_or(values)
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if kind == "show" {
            show.extend(names);
        } else {
            hide.extend(names);
        }
    }
    (show, hide)
}

fn lower_top_level_declaration(node: Node<'_>, source: &str) -> Option<Declaration> {
    match node.kind() {
        "class_declaration" => Some(Declaration::Class(lower_class(node, source, false))),
        "mixin_declaration" => Some(Declaration::Mixin(lower_class(node, source, true))),
        "enum_declaration" => Some(Declaration::Enum(lower_enum(node, source))),
        "extension_declaration" => Some(Declaration::Extension(lower_extension(node, source))),
        "type_alias" => Some(Declaration::TypeAlias(lower_alias(node, source))),
        "function_declaration" => Some(Declaration::Function(lower_function(node, source))),
        _ => None,
    }
}

fn lower_class(node: Node<'_>, source: &str, is_mixin: bool) -> ClassDeclaration {
    let text = node_text(node, source);
    let header = text.split('{').next().unwrap_or(text);
    let kind = if is_mixin {
        ClassKind::Mixin
    } else if header.contains("abstract interface class") {
        ClassKind::AbstractInterface
    } else if header.contains("abstract class") {
        ClassKind::Abstract
    } else if header.contains("interface class") {
        ClassKind::Interface
    } else if header.contains("base class") {
        ClassKind::Base
    } else if header.contains("final class") {
        ClassKind::Final
    } else if header.contains("sealed class") {
        ClassKind::Sealed
    } else {
        ClassKind::Class
    };
    let name = field_text(node, "name", source).unwrap_or_default();
    let type_parameters = node
        .child_by_field_name("type_parameters")
        .map(|child| lower_type_parameters(child, source))
        .unwrap_or_default();
    let superclass = node
        .child_by_field_name("superclass")
        .map(|child| node_text(child, source).to_string())
        .unwrap_or_default();
    let interfaces = node
        .child_by_field_name("interfaces")
        .map(|child| node_text(child, source).to_string())
        .unwrap_or_default();
    let extends =
        clause_after(&superclass, "extends", "with").and_then(|value| parse_type_reference(&value));
    let mixins = clause_from(&superclass, "with", "implements")
        .map(|value| parse_type_list(&value))
        .unwrap_or_default();
    let implements = interfaces
        .strip_prefix("implements")
        .map(parse_type_list)
        .unwrap_or_default();
    let mut members = node
        .child_by_field_name("body")
        .map(|body| lower_members(body, source, &name))
        .unwrap_or_default();
    resolve_constructor_parameter_types(&mut members);
    ClassDeclaration {
        name,
        kind,
        type_parameters,
        extends,
        mixins,
        implements,
        members,
        span: span(node),
    }
}

fn lower_enum(node: Node<'_>, source: &str) -> EnumDeclaration {
    let name = field_text(node, "name", source).unwrap_or_default();
    let mut values = Vec::new();
    walk_named(node, &mut |child| {
        if child.kind() == "enum_constant" {
            if let Some(value) = field_text(child, "name", source) {
                values.push(value);
            }
        }
    });
    EnumDeclaration {
        name,
        values,
        span: span(node),
    }
}

fn lower_extension(node: Node<'_>, source: &str) -> ExtensionDeclaration {
    let name = field_text(node, "name", source).unwrap_or_default();
    let on_type = node
        .child_by_field_name("class")
        .and_then(|child| parse_type_reference(node_text(child, source)))
        .unwrap_or_else(TypeReference::dynamic);
    let members = node
        .child_by_field_name("body")
        .map(|body| lower_members(body, source, ""))
        .unwrap_or_default();
    ExtensionDeclaration {
        name,
        on_type,
        members,
        span: span(node),
    }
}

fn lower_members(body: Node<'_>, source: &str, class_name: &str) -> Vec<ClassMember> {
    let mut members = Vec::new();
    let mut cursor = body.walk();
    for class_member in body.named_children(&mut cursor) {
        if class_member.kind() != "class_member" {
            continue;
        }
        if let Some(method) = find_first(class_member, "method_declaration") {
            if let Some(member) = lower_method_member(method, source, class_name) {
                members.push(member);
            }
            continue;
        }
        let Some(declaration) = find_first(class_member, "declaration") else {
            continue;
        };
        if let Some(signature) = ["constructor_signature", "constant_constructor_signature"]
            .iter()
            .find_map(|kind| find_first(declaration, kind))
        {
            members.push(ClassMember::Constructor(lower_constructor(
                signature,
                declaration,
                source,
                class_name,
                false,
            )));
            continue;
        }
        if let Some(signature) = find_first(declaration, "function_signature") {
            members.push(ClassMember::Method(lower_callable(
                signature,
                declaration,
                source,
            )));
            continue;
        }
        members.extend(
            lower_fields(declaration, source)
                .into_iter()
                .map(ClassMember::Field),
        );
    }
    members
}

fn lower_method_member(node: Node<'_>, source: &str, class_name: &str) -> Option<ClassMember> {
    let signature = find_first(node, "method_signature")?;
    if let Some(factory) = find_first(signature, "factory_constructor_signature") {
        return Some(ClassMember::Constructor(lower_constructor(
            factory, node, source, class_name, true,
        )));
    }
    if let Some(function) = find_first(signature, "function_signature") {
        return Some(ClassMember::Method(lower_callable(function, node, source)));
    }
    if let Some(getter) = find_first(signature, "getter_signature") {
        return Some(ClassMember::Getter(lower_callable(getter, node, source)));
    }
    if let Some(setter) = find_first(signature, "setter_signature") {
        return Some(ClassMember::Setter(lower_callable(setter, node, source)));
    }
    if let Some(operator) = find_first(signature, "operator_signature") {
        return Some(ClassMember::Operator(lower_callable(
            operator, node, source,
        )));
    }
    Some(ClassMember::Unlowered {
        syntax_kind: signature.kind().to_string(),
        span: span(node),
    })
}

fn lower_callable(signature: Node<'_>, declaration: Node<'_>, source: &str) -> FunctionDeclaration {
    let name = field_text(signature, "name", source)
        .or_else(|| {
            field_text(signature, "operator", source).map(|value| format!("operator {}", value))
        })
        .unwrap_or_default();
    let return_type = signature
        .child_by_field_name("return_type")
        .and_then(|node| parse_type_reference(node_text(node, source)))
        .unwrap_or_else(TypeReference::dynamic);
    let body =
        find_first(declaration, "function_body").map(|node| lower_function_body(node, source));
    let declaration_text = node_text(declaration, source);
    let type_parameters = find_first(signature, "type_parameters")
        .map(|node| lower_type_parameters(node, source))
        .unwrap_or_default();
    FunctionDeclaration {
        name,
        return_type,
        type_parameters,
        parameters: direct_child_of_kind(signature, "formal_parameter_list")
            .map(|node| lower_parameters(node, source))
            .unwrap_or_default(),
        is_async: body
            .as_ref()
            .map(|value| value.source.contains("async"))
            .unwrap_or(false),
        is_static: declaration_text.trim_start().starts_with("static "),
        body,
        span: span(declaration),
    }
}

fn lower_constructor(
    signature: Node<'_>,
    declaration: Node<'_>,
    source: &str,
    class_name: &str,
    is_factory: bool,
) -> ConstructorDeclaration {
    let signature_text = node_text(signature, source);
    let before_parameters = signature_text
        .split('(')
        .next()
        .unwrap_or(signature_text)
        .trim();
    let constructor_token = before_parameters
        .split_whitespace()
        .last()
        .unwrap_or(class_name);
    let mut names = constructor_token.split('.');
    let parsed_class = names.next().unwrap_or(class_name);
    let named = names.next().map(str::to_string);
    let body =
        find_first(declaration, "function_body").map(|node| lower_function_body(node, source));
    ConstructorDeclaration {
        class_name: if parsed_class.is_empty() {
            class_name.into()
        } else {
            parsed_class.into()
        },
        named,
        parameters: direct_child_of_kind(signature, "formal_parameter_list")
            .map(|node| lower_parameters(node, source))
            .unwrap_or_default(),
        is_const: signature.kind() == "constant_constructor_signature",
        is_factory,
        body,
        source: node_text(declaration, source).to_string(),
        span: span(declaration),
    }
}

fn lower_fields(declaration: Node<'_>, source: &str) -> Vec<FieldDeclaration> {
    let declaration_text = node_text(declaration, source);
    let type_ref = find_first(declaration, "type")
        .and_then(|node| parse_type_reference(node_text(node, source)))
        .unwrap_or_else(TypeReference::dynamic);
    let is_static = declaration_text.trim_start().starts_with("static ")
        || declaration_text.contains(" static ");
    let is_final = declaration_text.contains("final ") || declaration_text.contains("const ");
    let mut fields = Vec::new();
    walk_named(declaration, &mut |node| {
        if !matches!(
            node.kind(),
            "initialized_identifier" | "static_final_declaration"
        ) {
            return;
        }
        let Some(name) = field_text(node, "name", source) else {
            return;
        };
        let initializer = node.child_by_field_name("value").map(|value| Body {
            kind: BodyKind::Expression(lower_expression(value, source)),
            source: node_text(value, source).to_string(),
            syntax_kind: value.kind().to_string(),
            span: span(value),
        });
        fields.push(FieldDeclaration {
            name,
            type_ref: type_ref.clone(),
            is_static,
            is_final,
            initializer,
            span: span(node),
        });
    });
    fields
}

fn lower_alias(node: Node<'_>, source: &str) -> TypeAliasDeclaration {
    let mut name = String::new();
    let mut aliased_type = TypeReference::dynamic();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "type_identifier" && name.is_empty() {
            name = node_text(child, source).to_string();
        }
        if child.kind() == "type" {
            aliased_type = parse_type_reference(node_text(child, source))
                .unwrap_or_else(TypeReference::dynamic);
        }
    }
    let type_parameters = find_first(node, "type_parameters")
        .map(|child| lower_type_parameters(child, source))
        .unwrap_or_default();
    TypeAliasDeclaration {
        name,
        type_parameters,
        aliased_type,
        span: span(node),
    }
}

fn lower_function(node: Node<'_>, source: &str) -> FunctionDeclaration {
    let signature = find_first(node, "function_signature");
    let name = signature
        .and_then(|value| field_text(value, "name", source))
        .unwrap_or_default();
    let return_type = signature
        .and_then(|value| value.child_by_field_name("return_type"))
        .and_then(|value| parse_type_reference(node_text(value, source)))
        .unwrap_or_else(TypeReference::dynamic);
    let body = find_first(node, "function_body").map(|value| lower_function_body(value, source));
    let is_async = body
        .as_ref()
        .map(|value| value.source.contains("async"))
        .unwrap_or(false);
    let type_parameters = signature
        .and_then(|value| value.child_by_field_name("type_parameters"))
        .map(|value| lower_type_parameters(value, source))
        .unwrap_or_default();
    let parameters = signature
        .and_then(|value| direct_child_of_kind(value, "formal_parameter_list"))
        .map(|value| lower_parameters(value, source))
        .unwrap_or_default();
    FunctionDeclaration {
        name,
        return_type,
        type_parameters,
        parameters,
        is_async,
        is_static: false,
        body,
        span: span(node),
    }
}

fn lower_function_body(node: Node<'_>, source: &str) -> Body {
    let source_text = node_text(node, source).to_string();
    let kind = if let Some(block) = direct_child_of_kind(node, "block") {
        BodyKind::Block(lower_block(block, source))
    } else if let Some(expression) = first_named_child(node) {
        BodyKind::Expression(lower_expression(expression, source))
    } else {
        BodyKind::Empty
    };
    Body {
        kind,
        source: source_text,
        syntax_kind: node.kind().to_string(),
        span: span(node),
    }
}

fn lower_block(node: Node<'_>, source: &str) -> Vec<Statement> {
    let mut statements = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        statements.push(lower_statement(child, source));
    }
    statements
}

fn lower_statement(node: Node<'_>, source: &str) -> Statement {
    let source_text = node_text(node, source).to_string();
    let kind = match node.kind() {
        "block" => StatementKind::Block(lower_block(node, source)),
        "local_variable_declaration" => lower_local_variable(node, source),
        "expression_statement" => first_named_child(node)
            .map(|value| StatementKind::Expression(lower_expression(value, source)))
            .unwrap_or_else(|| StatementKind::Unlowered {
                syntax_kind: node.kind().into(),
            }),
        "if_statement" => {
            let condition = node
                .child_by_field_name("condition")
                .or_else(|| first_named_child(node));
            let consequence = node.child_by_field_name("consequence");
            let alternative = node.child_by_field_name("alternative");
            match (condition, consequence) {
                (Some(condition), Some(consequence)) => StatementKind::If {
                    condition: lower_expression(unwrap_parenthesized(condition), source),
                    then_branch: Box::new(lower_statement(consequence, source)),
                    else_branch: alternative.map(|value| Box::new(lower_statement(value, source))),
                },
                _ => StatementKind::Unlowered {
                    syntax_kind: node.kind().into(),
                },
            }
        }
        "for_statement" => {
            let variable = field_text(node, "name", source).unwrap_or_default();
            let iterable = node
                .child_by_field_name("value")
                .map(|value| lower_expression(value, source));
            let body = node
                .child_by_field_name("body")
                .map(|value| lower_statement(value, source));
            match (iterable, body) {
                (Some(iterable), Some(body)) => StatementKind::ForEach {
                    variable,
                    iterable,
                    body: Box::new(body),
                },
                _ => StatementKind::Unlowered {
                    syntax_kind: node.kind().into(),
                },
            }
        }
        "while_statement" => lower_loop(node, source, false),
        "do_statement" => lower_loop(node, source, true),
        "switch_statement" => lower_switch_statement(node, source),
        "try_statement" => lower_try_statement(node, source),
        "return_statement" => StatementKind::Return(
            first_named_child(node).map(|value| lower_expression(value, source)),
        ),
        "assert_statement" => {
            let assertion = find_first(node, "assertion")
                .and_then(first_named_child)
                .unwrap_or(node);
            StatementKind::Assert(lower_expression(assertion, source))
        }
        "break_statement" => StatementKind::Break,
        "continue_statement" => StatementKind::Continue,
        _ if is_expression_kind(node.kind()) => {
            StatementKind::Expression(lower_expression(node, source))
        }
        _ => StatementKind::Unlowered {
            syntax_kind: node.kind().into(),
        },
    };
    Statement {
        kind,
        source: source_text,
        span: span(node),
    }
}

fn lower_local_variable(node: Node<'_>, source: &str) -> StatementKind {
    let Some(definition) = find_first(node, "initialized_variable_definition") else {
        return StatementKind::Unlowered {
            syntax_kind: node.kind().into(),
        };
    };
    let name = field_text(definition, "name", source).unwrap_or_default();
    let type_ref = definition
        .child_by_field_name("type")
        .or_else(|| direct_child_of_kind(definition, "type"))
        .and_then(|value| parse_type_reference(node_text(value, source)))
        .unwrap_or_else(TypeReference::dynamic);
    let mut value_cursor = definition.walk();
    let values = definition
        .children_by_field_name("value", &mut value_cursor)
        .collect::<Vec<_>>();
    let initializer = values.first().map(|value| {
        let target = lower_expression(*value, source);
        if values.len() > 1 {
            Expression {
                kind: ExpressionKind::Cascade {
                    target: Box::new(target),
                    sections: values
                        .iter()
                        .skip(1)
                        .map(|value| raw_expression(*value, source))
                        .collect(),
                },
                source: node_text(*value, source).to_string(),
                span: span(definition),
            }
        } else {
            target
        }
    });
    let text = node_text(node, source).trim_start();
    StatementKind::Variable {
        name,
        type_ref,
        is_final: text.starts_with("final ") || text.starts_with("const "),
        initializer,
    }
}

fn lower_loop(node: Node<'_>, source: &str, is_do_while: bool) -> StatementKind {
    let condition = node
        .child_by_field_name("condition")
        .map(|value| lower_expression(unwrap_parenthesized(value), source));
    let body = node
        .child_by_field_name("body")
        .map(|value| lower_statement(value, source));
    match (condition, body, is_do_while) {
        (Some(condition), Some(body), false) => StatementKind::While {
            condition,
            body: Box::new(body),
        },
        (Some(condition), Some(body), true) => StatementKind::DoWhile {
            body: Box::new(body),
            condition,
        },
        _ => StatementKind::Unlowered {
            syntax_kind: node.kind().into(),
        },
    }
}

fn lower_switch_statement(node: Node<'_>, source: &str) -> StatementKind {
    let expression = node
        .child_by_field_name("condition")
        .map(|value| lower_expression(unwrap_parenthesized(value), source))
        .unwrap_or_else(|| raw_expression(node, source));
    let mut cases = Vec::new();
    if let Some(block) = find_first(node, "switch_block") {
        let mut cursor = block.walk();
        for case in block.named_children(&mut cursor) {
            if case.kind() != "switch_statement_case" {
                continue;
            }
            let pattern = first_named_child(case)
                .map(|value| lower_pattern(value, source))
                .unwrap_or_else(|| Pattern {
                    kind: PatternKind::Default,
                    source: "default".into(),
                    span: span(case),
                });
            let mut statements = Vec::new();
            let mut case_cursor = case.walk();
            let mut skipped_pattern = false;
            for child in case.named_children(&mut case_cursor) {
                if !skipped_pattern {
                    skipped_pattern = true;
                    continue;
                }
                statements.push(lower_statement(child, source));
            }
            cases.push(SwitchCase {
                pattern,
                statements,
                span: span(case),
            });
        }
    }
    StatementKind::Switch { expression, cases }
}

fn lower_try_statement(node: Node<'_>, source: &str) -> StatementKind {
    let body = direct_child_of_kind(node, "block")
        .map(|value| lower_statement(value, source))
        .unwrap_or_else(|| Statement {
            kind: StatementKind::Block(Vec::new()),
            source: String::new(),
            span: span(node),
        });
    let mut catches = Vec::new();
    let mut finally_body = None;
    let children = direct_named_children(node);
    for (index, child) in children.iter().copied().enumerate() {
        match child.kind() {
            "catch_clause" => {
                let exception_name = field_text(child, "exception", source);
                let stack_name = field_text(child, "stack_trace", source);
                let catch_body = direct_child_of_kind(child, "block")
                    .or_else(|| {
                        children
                            .get(index + 1)
                            .copied()
                            .filter(|value| value.kind() == "block")
                    })
                    .map(|value| lower_statement(value, source))
                    .unwrap_or_else(|| Statement {
                        kind: StatementKind::Block(Vec::new()),
                        source: String::new(),
                        span: span(child),
                    });
                catches.push(CatchClause {
                    exception_type: children
                        .get(index.wrapping_sub(1))
                        .copied()
                        .filter(|value| value.kind() == "type")
                        .and_then(|value| parse_type_reference(node_text(value, source))),
                    exception_name,
                    stack_name,
                    body: Box::new(catch_body),
                    span: span(child),
                });
            }
            "finally_clause" => {
                finally_body = direct_child_of_kind(child, "block")
                    .map(|value| Box::new(lower_statement(value, source)));
            }
            _ => {}
        }
    }
    StatementKind::Try {
        body: Box::new(body),
        catches,
        finally_body,
    }
}

fn lower_expression(node: Node<'_>, source: &str) -> Expression {
    let source_text = node_text(node, source).to_string();
    let kind = match node.kind() {
        "identifier" | "type_identifier" => ExpressionKind::Identifier(source_text.clone()),
        "decimal_integer_literal" | "hex_integer_literal" => {
            ExpressionKind::Literal(Literal::Integer(source_text.clone()))
        }
        "decimal_floating_point_literal" => {
            ExpressionKind::Literal(Literal::Float(source_text.clone()))
        }
        "true" => ExpressionKind::Literal(Literal::Bool(true)),
        "false" => ExpressionKind::Literal(Literal::Bool(false)),
        "null_literal" => ExpressionKind::Literal(Literal::Null),
        "string_literal" => lower_string_literal(node, source),
        "parenthesized_expression" | "null_assertion_expression" => first_named_child(node)
            .map(|value| lower_expression(value, source).kind)
            .unwrap_or_else(|| ExpressionKind::Raw {
                syntax_kind: node.kind().into(),
            }),
        "assignable_expression" => {
            match (
                node.child_by_field_name("object"),
                field_text(node, "property", source),
            ) {
                (Some(object), Some(property)) => ExpressionKind::Member {
                    object: Box::new(lower_expression(object, source)),
                    property,
                    null_aware: false,
                },
                _ => first_named_child(node)
                    .map(|value| lower_expression(value, source).kind)
                    .unwrap_or_else(|| ExpressionKind::Raw {
                        syntax_kind: node.kind().into(),
                    }),
            }
        }
        "if_null_expression" => {
            let children = direct_named_children(node);
            if children.len() >= 2 {
                ExpressionKind::IfNull {
                    left: Box::new(lower_expression(children[0], source)),
                    right: Box::new(lower_expression(*children.last().unwrap(), source)),
                }
            } else {
                ExpressionKind::Raw {
                    syntax_kind: node.kind().into(),
                }
            }
        }
        "assignment_expression" => {
            let left = node.child_by_field_name("left");
            let right = node.child_by_field_name("right");
            match (left, right) {
                (Some(left), Some(right)) => ExpressionKind::Assignment {
                    target: Box::new(lower_expression(left, source)),
                    operator: operator_between(left, right, source),
                    value: Box::new(lower_expression(right, source)),
                },
                _ => ExpressionKind::Raw {
                    syntax_kind: node.kind().into(),
                },
            }
        }
        "member_expression" | "null_aware_member_expression" => {
            let object = node.child_by_field_name("object");
            let property = field_text(node, "property", source).unwrap_or_default();
            match object {
                Some(object) => ExpressionKind::Member {
                    object: Box::new(lower_expression(object, source)),
                    property,
                    null_aware: node.kind().starts_with("null_aware"),
                },
                None => ExpressionKind::Raw {
                    syntax_kind: node.kind().into(),
                },
            }
        }
        "index_expression" | "null_aware_index_expression" => {
            let object = node.child_by_field_name("object");
            let index = node.child_by_field_name("index");
            match (object, index) {
                (Some(object), Some(index)) => ExpressionKind::Index {
                    object: Box::new(lower_expression(object, source)),
                    index: Box::new(lower_expression(index, source)),
                    null_aware: node.kind().starts_with("null_aware"),
                },
                _ => ExpressionKind::Raw {
                    syntax_kind: node.kind().into(),
                },
            }
        }
        "call_expression" => lower_call_expression(node, source),
        "const_object_expression" | "new_expression" => lower_object_creation(node, source),
        "list_literal" => lower_list_literal(node, source),
        "set_or_map_literal" => lower_map_literal(node, source),
        "function_expression" => lower_closure(node, source),
        "await_expression" => first_named_child(node)
            .map(|value| ExpressionKind::Await(Box::new(lower_expression(value, source))))
            .unwrap_or_else(|| ExpressionKind::Raw {
                syntax_kind: node.kind().into(),
            }),
        "type_cast_expression" => {
            let expression = first_named_child(node);
            let type_node = find_first(node, "type");
            match (expression, type_node) {
                (Some(expression), Some(type_node)) => ExpressionKind::Cast {
                    expression: Box::new(lower_expression(expression, source)),
                    type_ref: parse_type_reference(node_text(type_node, source))
                        .unwrap_or_else(TypeReference::dynamic),
                },
                _ => ExpressionKind::Raw {
                    syntax_kind: node.kind().into(),
                },
            }
        }
        "type_test_expression" => {
            let expression = first_named_child(node);
            let type_node = find_first(node, "type");
            match (expression, type_node) {
                (Some(expression), Some(type_node)) => ExpressionKind::TypeTest {
                    expression: Box::new(lower_expression(expression, source)),
                    type_ref: parse_type_reference(node_text(type_node, source))
                        .unwrap_or_else(TypeReference::dynamic),
                    negated: source_text.contains(" is!"),
                },
                _ => ExpressionKind::Raw {
                    syntax_kind: node.kind().into(),
                },
            }
        }
        "unary_expression" | "postfix_expression" => {
            let operand = first_named_child(node);
            match operand {
                Some(operand) => ExpressionKind::Unary {
                    operator: source_text
                        .replace(node_text(operand, source), "")
                        .trim()
                        .to_string(),
                    operand: Box::new(lower_expression(operand, source)),
                },
                None => ExpressionKind::Raw {
                    syntax_kind: node.kind().into(),
                },
            }
        }
        "switch_expression" => lower_switch_expression(node, source),
        kind if is_binary_kind(kind) => lower_binary_expression(node, source, ""),
        _ => ExpressionKind::Raw {
            syntax_kind: node.kind().into(),
        },
    };
    Expression {
        kind,
        source: source_text,
        span: span(node),
    }
}

fn lower_binary_expression(
    node: Node<'_>,
    source: &str,
    fallback_operator: &str,
) -> ExpressionKind {
    let children = direct_named_children(node);
    if children.len() < 2 {
        return ExpressionKind::Raw {
            syntax_kind: node.kind().into(),
        };
    }
    let left = children[0];
    let right = *children.last().unwrap();
    ExpressionKind::Binary {
        operator: if fallback_operator.is_empty() {
            operator_between(left, right, source)
        } else {
            fallback_operator.into()
        },
        left: Box::new(lower_expression(left, source)),
        right: Box::new(lower_expression(right, source)),
    }
}

fn lower_call_expression(node: Node<'_>, source: &str) -> ExpressionKind {
    let callee = node
        .child_by_field_name("function")
        .or_else(|| first_named_child(node));
    // A call's callee may itself contain a nested call (`Solution().twoSum(...)`).
    // A recursive search finds the inner `Solution()` argument list first and silently
    // drops the outer arguments. Always select the argument list owned by this call.
    let arguments_node = node
        .child_by_field_name("arguments")
        .or_else(|| direct_child_of_kind(node, "arguments"));
    match callee {
        Some(callee) => ExpressionKind::Call {
            callee: Box::new(lower_expression(callee, source)),
            arguments: arguments_node
                .map(|value| lower_arguments(value, source))
                .unwrap_or_default(),
            type_arguments: Vec::new(),
        },
        None => ExpressionKind::Raw {
            syntax_kind: node.kind().into(),
        },
    }
}

fn lower_object_creation(node: Node<'_>, source: &str) -> ExpressionKind {
    let raw = node_text(node, source).trim();
    let clean = raw.trim_start_matches("const ").trim_start_matches("new ");
    let header = clean.split('(').next().unwrap_or(clean).trim();
    let generic_depth_end = header.rfind('>').map(|index| index + 1).unwrap_or(0);
    let constructor_separator = header[generic_depth_end..]
        .rfind('.')
        .map(|index| generic_depth_end + index);
    let (type_name, constructor) = constructor_separator
        .map(|index| (&header[..index], Some(header[index + 1..].to_string())))
        .unwrap_or((header, None));
    ExpressionKind::ObjectCreation {
        type_ref: parse_type_reference(type_name).unwrap_or_else(TypeReference::dynamic),
        constructor,
        arguments: node
            .child_by_field_name("arguments")
            .or_else(|| direct_child_of_kind(node, "arguments"))
            .map(|value| lower_arguments(value, source))
            .unwrap_or_default(),
        is_const: raw.starts_with("const "),
    }
}

fn lower_arguments(node: Node<'_>, source: &str) -> Vec<Argument> {
    let mut arguments = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "named_argument" {
            let name = find_first(child, "label")
                .and_then(|label| find_first(label, "identifier"))
                .map(|value| node_text(value, source).to_string());
            let value = direct_named_children(child)
                .last()
                .copied()
                .map(|value| lower_expression(value, source))
                .unwrap_or_else(|| raw_expression(child, source));
            arguments.push(Argument { name, value });
        } else {
            arguments.push(Argument {
                name: None,
                value: lower_expression(child, source),
            });
        }
    }
    arguments
}

fn lower_list_literal(node: Node<'_>, source: &str) -> ExpressionKind {
    let element_type = find_first(node, "type_arguments")
        .and_then(|arguments| find_first(arguments, "type"))
        .and_then(|value| parse_type_reference(node_text(value, source)));
    let mut elements = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "type_arguments" {
            continue;
        }
        if child.kind() == "spread_element" {
            let expression = child
                .child_by_field_name("value")
                .or_else(|| first_named_child(child))
                .map(|value| lower_expression(value, source))
                .unwrap_or_else(|| raw_expression(child, source));
            elements.push(CollectionElement::Spread {
                expression,
                null_aware: node_text(child, source).starts_with("...?"),
            });
        } else {
            elements.push(CollectionElement::Expression(lower_expression(
                child, source,
            )));
        }
    }
    ExpressionKind::ListLiteral {
        element_type,
        elements,
    }
}

fn lower_map_literal(node: Node<'_>, source: &str) -> ExpressionKind {
    let types = find_first(node, "type_arguments")
        .map(|arguments| {
            direct_named_children(arguments)
                .into_iter()
                .filter(|child| child.kind() == "type")
                .filter_map(|child| parse_type_reference(node_text(child, source)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut entries = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() != "pair" {
            continue;
        }
        if let (Some(key), Some(value)) = (
            child.child_by_field_name("key"),
            child.child_by_field_name("value"),
        ) {
            entries.push((
                lower_expression(key, source),
                lower_expression(value, source),
            ));
        }
    }
    ExpressionKind::MapLiteral {
        key_type: types.first().cloned(),
        value_type: types.get(1).cloned(),
        entries,
    }
}

fn lower_closure(node: Node<'_>, source: &str) -> ExpressionKind {
    let parameters = direct_child_of_kind(node, "formal_parameter_list")
        .map(|value| lower_parameters(value, source))
        .unwrap_or_default();
    let body_node =
        find_first(node, "function_expression_body").or_else(|| find_first(node, "block"));
    let body = body_node
        .map(|value| lower_function_body(value, source))
        .unwrap_or_default();
    ExpressionKind::Closure {
        parameters,
        body: Box::new(body),
    }
}

fn lower_string_literal(node: Node<'_>, source: &str) -> ExpressionKind {
    let raw = node_text(node, source);
    if !raw.contains('$') {
        return ExpressionKind::Literal(Literal::String(raw.to_string()));
    }
    let mut parts = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind().starts_with("string_literal_") {
            let mut inner = child.walk();
            for piece in child.named_children(&mut inner) {
                if piece.kind() == "template_substitution" {
                    let expression = first_named_child(piece)
                        .map(|value| lower_expression(value, source))
                        .unwrap_or_else(|| raw_expression(piece, source));
                    parts.push(StringPart::Expression(expression));
                } else {
                    parts.push(StringPart::Text(node_text(piece, source).to_string()));
                }
            }
        }
    }
    if parts.is_empty() {
        ExpressionKind::Literal(Literal::String(raw.to_string()))
    } else {
        ExpressionKind::StringInterpolation(parts)
    }
}

fn lower_switch_expression(node: Node<'_>, source: &str) -> ExpressionKind {
    let condition = node
        .child_by_field_name("condition")
        .or_else(|| first_named_child(node))
        .map(|value| lower_expression(unwrap_parenthesized(value), source))
        .unwrap_or_else(|| raw_expression(node, source));
    let mut cases = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() != "switch_expression_case" {
            continue;
        }
        let children = direct_named_children(child);
        if children.len() >= 2 {
            cases.push(SwitchExpressionCase {
                pattern: lower_pattern(children[0], source),
                value: lower_expression(*children.last().unwrap(), source),
                span: span(child),
            });
        }
    }
    ExpressionKind::Switch {
        expression: Box::new(condition),
        cases,
    }
}

fn lower_pattern(node: Node<'_>, source: &str) -> Pattern {
    let source_text = node_text(node, source).to_string();
    let kind = match node.kind() {
        "object_pattern" => {
            let type_ref = find_first(node, "type_identifier")
                .and_then(|value| parse_type_reference(node_text(value, source)))
                .unwrap_or_else(TypeReference::dynamic);
            let fields = source_text
                .find('(')
                .and_then(|open| source_text.rfind(')').map(|close| (open, close)))
                .map(|(open, close)| {
                    split_top_level(&source_text[open + 1..close])
                        .into_iter()
                        .filter_map(|field| {
                            let (name, binding) = field.split_once(':')?;
                            Some(PatternField {
                                name: name.trim().to_string(),
                                binding: binding
                                    .trim()
                                    .trim_start_matches("final ")
                                    .trim_start_matches("var ")
                                    .to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            PatternKind::Object { type_ref, fields }
        }
        "variable_pattern" => PatternKind::Variable {
            name: field_text(node, "name", source).unwrap_or_else(|| {
                source_text
                    .trim_start_matches("final ")
                    .trim_start_matches("var ")
                    .to_string()
            }),
            is_final: source_text.trim_start().starts_with("final "),
        },
        "wildcard_pattern" if source_text.trim() == "_" => PatternKind::Wildcard,
        "constant_pattern" => PatternKind::Constant(raw_expression(node, source)),
        "default" => PatternKind::Default,
        _ => PatternKind::Raw {
            syntax_kind: node.kind().into(),
        },
    };
    Pattern {
        kind,
        source: source_text,
        span: span(node),
    }
}

fn raw_expression(node: Node<'_>, source: &str) -> Expression {
    Expression {
        kind: ExpressionKind::Raw {
            syntax_kind: node.kind().into(),
        },
        source: node_text(node, source).to_string(),
        span: span(node),
    }
}

fn is_binary_kind(kind: &str) -> bool {
    matches!(
        kind,
        "additive_expression"
            | "multiplicative_expression"
            | "equality_expression"
            | "relational_expression"
            | "logical_and_expression"
            | "logical_or_expression"
            | "bitwise_and_expression"
            | "bitwise_or_expression"
            | "bitwise_xor_expression"
            | "shift_expression"
    )
}

fn is_expression_kind(kind: &str) -> bool {
    is_binary_kind(kind)
        || kind.ends_with("_expression")
        || matches!(
            kind,
            "identifier"
                | "string_literal"
                | "decimal_integer_literal"
                | "decimal_floating_point_literal"
                | "true"
                | "false"
                | "null_literal"
                | "list_literal"
                | "set_or_map_literal"
        )
}

fn operator_between(left: Node<'_>, right: Node<'_>, source: &str) -> String {
    source
        .get(left.end_byte()..right.start_byte())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn unwrap_parenthesized(node: Node<'_>) -> Node<'_> {
    if node.kind() == "parenthesized_expression" {
        first_named_child(node).unwrap_or(node)
    } else {
        node
    }
}

fn lower_parameters(node: Node<'_>, source: &str) -> Vec<Parameter> {
    fn collect(node: Node<'_>, source: &str, kind: ParameterKind, output: &mut Vec<Parameter>) {
        if node.kind() == "formal_parameter" {
            let name = field_text(node, "name", source)
                .or_else(|| {
                    ["constructor_param", "super_formal_parameter"]
                        .iter()
                        .find_map(|kind| find_first(node, kind))
                        .and_then(|value| find_first(value, "identifier"))
                        .map(|value| node_text(value, source).to_string())
                })
                .or_else(|| {
                    find_first(node, "identifier").map(|value| node_text(value, source).to_string())
                })
                .unwrap_or_default();
            let type_ref = node
                .child_by_field_name("type")
                .or_else(|| find_first(node, "type"))
                .and_then(|value| parse_type_reference(node_text(value, source)))
                .unwrap_or_else(TypeReference::dynamic);
            let text = node_text(node, source);
            output.push(Parameter {
                name,
                type_ref,
                kind,
                is_required: text.trim_start().starts_with("required "),
                default_value: None,
                span: span(node),
            });
            return;
        }
        let next_kind = if node.kind() == "optional_formal_parameters" {
            if node_text(node, source).trim_start().starts_with('{') {
                ParameterKind::Named
            } else {
                ParameterKind::OptionalPositional
            }
        } else {
            kind
        };
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect(child, source, next_kind, output);
        }
    }
    let mut parameters = Vec::new();
    collect(node, source, ParameterKind::Positional, &mut parameters);
    parameters
}

fn resolve_constructor_parameter_types(members: &mut [ClassMember]) {
    let field_types = members
        .iter()
        .filter_map(|member| match member {
            ClassMember::Field(field) => Some((field.name.clone(), field.type_ref.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    for member in members {
        let ClassMember::Constructor(constructor) = member else {
            continue;
        };
        for parameter in &mut constructor.parameters {
            if parameter.type_ref.name != "dynamic" {
                continue;
            }
            if let Some((_, field_type)) =
                field_types.iter().find(|(name, _)| name == &parameter.name)
            {
                parameter.type_ref = field_type.clone();
            }
        }
    }
}

fn lower_type_parameters(node: Node<'_>, source: &str) -> Vec<TypeParameter> {
    let mut parameters = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() != "type_parameter" {
            continue;
        }
        let name = field_text(child, "name", source).unwrap_or_default();
        parameters.push(TypeParameter {
            name,
            bound: None,
            span: span(child),
        });
    }
    parameters
}

fn parse_type_reference(raw: &str) -> Option<TypeReference> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let nullable = raw.ends_with('?');
    let clean = raw.trim_end_matches('?').trim();
    if let Some(open) = clean.find('<') {
        let close = clean.rfind('>')?;
        let name = clean[..open].trim().to_string();
        let arguments = split_top_level(&clean[open + 1..close])
            .iter()
            .filter_map(|value| parse_type_reference(value))
            .collect();
        Some(TypeReference {
            name,
            arguments,
            nullable,
        })
    } else {
        Some(TypeReference {
            name: clean.to_string(),
            arguments: Vec::new(),
            nullable,
        })
    }
}

fn parse_type_list(raw: &str) -> Vec<TypeReference> {
    split_top_level(raw)
        .iter()
        .filter_map(|value| parse_type_reference(value))
        .collect()
}

fn split_top_level(raw: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for character in raw.chars() {
        match character {
            '<' | '(' | '[' => {
                depth += 1;
                current.push(character);
            }
            '>' | ')' | ']' => {
                depth -= 1;
                current.push(character);
            }
            ',' if depth == 0 => {
                values.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(character),
        }
    }
    if !current.trim().is_empty() {
        values.push(current.trim().to_string());
    }
    values
}

fn clause_after(raw: &str, start: &str, stop: &str) -> Option<String> {
    let value = raw.trim().strip_prefix(start)?.trim();
    Some(value.split(stop).next().unwrap_or(value).trim().to_string())
}

fn clause_from(raw: &str, start: &str, stop: &str) -> Option<String> {
    let marker = format!("{} ", start);
    let offset = raw.find(&marker)? + marker.len();
    let value = raw[offset..].trim();
    Some(value.split(stop).next().unwrap_or(value).trim().to_string())
}

fn field_text(node: Node<'_>, field: &str, source: &str) -> Option<String> {
    node.child_by_field_name(field)
        .map(|child| node_text(child, source).to_string())
}

fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or("")
}

fn find_first<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = find_first(child, kind) {
            return Some(found);
        }
    }
    None
}

fn direct_child_of_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    let found = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == kind);
    found
}

fn first_named_child(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).next();
    found
}

fn direct_named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn walk_named(node: Node<'_>, visitor: &mut impl FnMut(Node<'_>)) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visitor(child);
        walk_named(child, visitor);
    }
}

fn collect_syntax_errors(node: Node<'_>, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    if node.is_error() || node.is_missing() {
        diagnostics.push(Diagnostic {
            code: "DART1001",
            severity: Severity::Error,
            message: format!(
                "Unexpected Dart syntax near `{}`",
                node_text(node, source).chars().take(32).collect::<String>()
            ),
            span: span(node),
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_syntax_errors(child, source, diagnostics);
    }
}

fn span(node: Node<'_>) -> SourceSpan {
    let start = node.start_position();
    let end = node.end_position();
    SourceSpan {
        start: SourcePosition {
            byte: node.start_byte(),
            line: start.row + 1,
            column: start.column + 1,
        },
        end: SourcePosition {
            byte: node.end_byte(),
            line: end.row + 1,
            column: end.column + 1,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPREHENSIVE_DART: &str = include_str!("../../../tests/fixtures/comprehensive.dart");

    #[test]
    fn parses_comprehensive_dart_without_syntax_errors() {
        let unit = DartFrontend.parse(COMPREHENSIVE_DART);
        assert!(unit.diagnostics.is_empty(), "{:#?}", unit.diagnostics);
        let names = unit
            .declarations
            .iter()
            .map(Declaration::name)
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "Mapper",
                "Role",
                "Timestamped",
                "Greeter",
                "Entity",
                "User",
                "Result",
                "Success",
                "Failure",
                "IntegerIterableX",
                "describeResult",
                "main"
            ]
        );
    }

    #[test]
    fn retains_imports_and_warns_for_external_packages() {
        let source = r#"
import 'dart:convert';
import 'package:dio/dio.dart' as dio show Dio, Options;

void main() {}
"#;
        let unit = DartFrontend.parse(source);
        assert_eq!(unit.imports.len(), 2);
        assert_eq!(unit.imports[0].uri, "dart:convert");
        assert_eq!(unit.imports[1].uri, "package:dio/dio.dart");
        assert_eq!(unit.imports[1].prefix.as_deref(), Some("dio"));
        assert_eq!(unit.imports[1].show, vec!["Dio", "Options"]);
        assert!(unit.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "DART2001"
                && diagnostic.message.contains("package:dio/dio.dart")
        }));
    }

    #[test]
    fn preserves_dart_class_relationships_and_modifiers() {
        let unit = DartFrontend.parse(COMPREHENSIVE_DART);
        let user = unit
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Class(value) if value.name == "User" => Some(value),
                _ => None,
            })
            .expect("User class missing");
        assert_eq!(user.kind, ClassKind::Final);
        assert_eq!(
            user.extends.as_ref().map(|value| value.name.as_str()),
            Some("Entity")
        );
        assert_eq!(
            user.mixins
                .iter()
                .map(|value| value.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Timestamped"]
        );
        assert_eq!(
            user.implements
                .iter()
                .map(|value| value.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Greeter"]
        );
        assert_eq!(user.implements[0].arguments[0].name, "String");
        assert_eq!(
            user.members
                .iter()
                .filter(|member| matches!(member, ClassMember::Field(_)))
                .count(),
            5
        );
        assert_eq!(
            user.members
                .iter()
                .filter(|member| matches!(member, ClassMember::Constructor(_)))
                .count(),
            3
        );
        assert_eq!(
            user.members
                .iter()
                .filter(|member| matches!(member, ClassMember::Getter(_)))
                .count(),
            3
        );
        assert_eq!(
            user.members
                .iter()
                .filter(|member| matches!(member, ClassMember::Setter(_)))
                .count(),
            2
        );
        assert_eq!(
            user.members
                .iter()
                .filter(|member| matches!(member, ClassMember::Operator(_)))
                .count(),
            1
        );
        assert_eq!(
            user.members
                .iter()
                .filter(|member| matches!(member, ClassMember::Method(_)))
                .count(),
            5
        );
        let greet = user
            .members
            .iter()
            .find_map(|member| match member {
                ClassMember::Method(value) if value.name == "greet" => Some(value),
                _ => None,
            })
            .expect("greet method missing");
        assert_eq!(greet.parameters.len(), 1);
        assert_eq!(greet.parameters[0].name, "prefix");
        assert_eq!(greet.parameters[0].type_ref.name, "String");
        let primary_constructor = user
            .members
            .iter()
            .find_map(|member| match member {
                ClassMember::Constructor(value) if value.named.is_none() && !value.is_factory => {
                    Some(value)
                }
                _ => None,
            })
            .expect("primary constructor missing");
        assert_eq!(
            primary_constructor
                .parameters
                .iter()
                .map(|value| value.name.as_str())
                .collect::<Vec<_>>(),
            vec!["id", "_name", "role", "tags"]
        );
        assert_eq!(primary_constructor.parameters[2].kind, ParameterKind::Named);
    }

    #[test]
    fn lowers_comprehensive_main_into_structured_statements() {
        let unit = DartFrontend.parse(COMPREHENSIVE_DART);
        let main = unit
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(value) if value.name == "main" => Some(value),
                _ => None,
            })
            .expect("main function missing");
        let BodyKind::Block(statements) = &main.body.as_ref().expect("main body missing").kind
        else {
            panic!("main was not lowered as a block");
        };
        assert_eq!(
            statements
                .iter()
                .filter(|statement| matches!(statement.kind, StatementKind::Variable { .. }))
                .count(),
            7
        );
        assert!(statements
            .iter()
            .any(|statement| matches!(statement.kind, StatementKind::ForEach { .. })));
        assert!(statements
            .iter()
            .any(|statement| matches!(statement.kind, StatementKind::While { .. })));
        assert!(statements
            .iter()
            .any(|statement| matches!(statement.kind, StatementKind::DoWhile { .. })));
        assert!(statements
            .iter()
            .any(|statement| matches!(statement.kind, StatementKind::Switch { .. })));
        assert!(statements
            .iter()
            .any(|statement| matches!(statement.kind, StatementKind::Try { .. })));
    }

    #[test]
    fn lowers_object_patterns_and_cascades_as_typed_ir() {
        let unit = DartFrontend.parse(COMPREHENSIVE_DART);
        let describe = unit
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(value) if value.name == "describeResult" => Some(value),
                _ => None,
            })
            .expect("describeResult missing");
        let BodyKind::Expression(Expression {
            kind: ExpressionKind::Switch { cases, .. },
            ..
        }) = &describe
            .body
            .as_ref()
            .expect("describeResult body missing")
            .kind
        else {
            panic!("pattern switch missing");
        };
        assert_eq!(cases.len(), 2);
        assert!(cases.iter().all(|case| match &case.pattern.kind {
            PatternKind::Object { fields, .. } =>
                fields.len() == 1 && !fields[0].binding.is_empty(),
            _ => false,
        }));

        let main = unit
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(value) if value.name == "main" => Some(value),
                _ => None,
            })
            .expect("main missing");
        let BodyKind::Block(statements) = &main.body.as_ref().unwrap().kind else {
            panic!("main block missing");
        };
        assert!(statements
            .iter()
            .filter_map(|statement| match &statement.kind {
                StatementKind::Variable {
                    initializer: Some(value),
                    ..
                } => Some(value),
                _ => None,
            })
            .take(2)
            .all(|value| matches!(value.kind, ExpressionKind::Cascade { .. })));
    }
}
