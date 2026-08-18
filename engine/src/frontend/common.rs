//! Shared Tree-sitter setup and Universal IR lowering used by all source frontends.

use tree_sitter::{Language as TreeSitterLanguage, Node, Parser, Tree};

use crate::diagnostic::{Diagnostic, Severity, SourcePosition, SourceSpan};
use crate::typed_ir::{
    Argument, Body, BodyKind, CollectionElement, CompilationUnit, Expression, ExpressionKind,
    FunctionDeclaration, Literal, Parameter, ParameterKind, Statement, StatementKind,
    TypeReference,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AstLanguage {
    JavaScript,
    Java,
    Dart,
    Swift,
    Python,
    Go,
    Rust,
}

impl AstLanguage {
    fn grammar(self) -> TreeSitterLanguage {
        match self {
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::Java => tree_sitter_java::LANGUAGE.into(),
            Self::Dart => tree_sitter_dart::LANGUAGE.into(),
            Self::Swift => tree_sitter_swift::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
        }
    }

    fn diagnostic_code(self) -> &'static str {
        match self {
            Self::JavaScript => "JS0001",
            Self::Java => "JAVA0001",
            Self::Dart => "DART1001",
            Self::Swift => "SWIFT0001",
            Self::Python => "PYTHON0001",
            Self::Go => "GO0001",
            Self::Rust => "RUST0001",
        }
    }

    fn initialization_diagnostic_code(self) -> &'static str {
        match self {
            Self::Dart => "DART0001",
            _ => self.diagnostic_code(),
        }
    }

    fn parse_diagnostic_code(self) -> &'static str {
        match self {
            Self::Dart => "DART0002",
            _ => self.diagnostic_code(),
        }
    }
}

pub(crate) fn syntax_tree(source: &str, language: AstLanguage) -> Result<Tree, CompilationUnit> {
    let mut parser = Parser::new();
    if parser.set_language(&language.grammar()).is_err() {
        return Err(failed_unit(
            language.initialization_diagnostic_code(),
            "Unable to initialize the AST parser",
        ));
    }
    parser.parse(source, None).ok_or_else(|| {
        failed_unit(
            language.parse_diagnostic_code(),
            "The AST parser did not produce a syntax tree",
        )
    })
}

fn failed_unit(code: &'static str, message: &str) -> CompilationUnit {
    CompilationUnit {
        imports: Vec::new(),
        declarations: Vec::new(),
        diagnostics: vec![Diagnostic {
            code,
            severity: Severity::Error,
            message: message.into(),
            span: SourceSpan::default(),
        }],
    }
}

pub(crate) fn lower_function(
    node: Node<'_>,
    source: &str,
    language: AstLanguage,
) -> FunctionDeclaration {
    let name = field_text(node, "name", source)
        .or_else(|| {
            direct_named_children(node)
                .into_iter()
                .find(|value| is_identifier(*value))
                .map(|value| text(value, source).into())
        })
        .unwrap_or_default();
    let parameters = lower_parameters(node, source, language);
    let return_type = function_return_type(node, source, language);
    let body_node = node.child_by_field_name("body").or_else(|| {
        direct_named_children(node)
            .into_iter()
            .find(|child| is_block(child.kind()))
    });
    let body = body_node.map(|body| Body {
        kind: BodyKind::Block(lower_block(body, source, language)),
        source: text(body, source).into(),
        syntax_kind: body.kind().into(),
        span: span(body),
    });
    FunctionDeclaration {
        name,
        return_type,
        parameters,
        is_static: language == AstLanguage::Java
            && text(node, source).trim_start().starts_with("static "),
        body,
        span: span(node),
        ..FunctionDeclaration::default()
    }
}

pub(crate) fn lower_parameters(
    node: Node<'_>,
    source: &str,
    language: AstLanguage,
) -> Vec<Parameter> {
    let container = node.child_by_field_name("parameters").or_else(|| {
        direct_named_children(node).into_iter().find(|child| {
            matches!(
                child.kind(),
                "formal_parameters" | "parameters" | "parameter_list"
            )
        })
    });
    let parameter_nodes = match (language, container) {
        (AstLanguage::Swift, _) => direct_named_children(node)
            .into_iter()
            .filter(|child| child.kind() == "parameter")
            .collect(),
        (_, Some(container)) => direct_named_children(container),
        _ => Vec::new(),
    };
    parameter_nodes
        .into_iter()
        .filter_map(|parameter| lower_parameter(parameter, source, language))
        .collect()
}

fn lower_parameter(node: Node<'_>, source: &str, language: AstLanguage) -> Option<Parameter> {
    let children = direct_named_children(node);
    let (name_node, type_node) = match language {
        AstLanguage::JavaScript => {
            if node.kind() == "assignment_pattern" {
                (
                    node.child_by_field_name("left")
                        .or_else(|| first_named_child(node)),
                    None,
                )
            } else {
                (Some(node), None)
            }
        }
        AstLanguage::Swift => {
            let identifiers = children
                .iter()
                .copied()
                .filter(|value| is_identifier(*value))
                .collect::<Vec<_>>();
            let name = identifiers
                .get(1)
                .copied()
                .or_else(|| identifiers.first().copied());
            let ty = children
                .iter()
                .copied()
                .rev()
                .find(|child| !is_identifier(*child));
            (name, ty)
        }
        AstLanguage::Python => {
            if is_identifier(node) {
                (Some(node), None)
            } else {
                (
                    children.iter().copied().find(|value| is_identifier(*value)),
                    node.child_by_field_name("type"),
                )
            }
        }
        _ => (
            node.child_by_field_name("name")
                .or_else(|| node.child_by_field_name("pattern"))
                .or_else(|| children.iter().copied().find(|value| is_identifier(*value))),
            node.child_by_field_name("type").or_else(|| {
                children
                    .iter()
                    .copied()
                    .rev()
                    .find(|child| !is_identifier(*child) && child.kind() != "mutable_specifier")
            }),
        ),
    };
    let name_node = name_node?;
    Some(Parameter {
        name: text(name_node, source).trim_start_matches("mut ").into(),
        type_ref: type_node
            .map(|value| lower_type(value, source, language))
            .unwrap_or_else(TypeReference::dynamic),
        kind: ParameterKind::Positional,
        is_required: true,
        default_value: node
            .child_by_field_name("value")
            .or_else(|| node.child_by_field_name("right"))
            .map(|value| lower_expression(value, source, language)),
        span: span(node),
    })
}

fn function_return_type(node: Node<'_>, source: &str, language: AstLanguage) -> TypeReference {
    let explicit = node.child_by_field_name("return_type").or_else(|| {
        if language == AstLanguage::Java {
            node.child_by_field_name("type")
        } else if language == AstLanguage::Go {
            node.child_by_field_name("result")
        } else if language == AstLanguage::Swift {
            direct_named_children(node).into_iter().rev().find(|child| {
                !matches!(
                    child.kind(),
                    "function_body" | "parameter" | "simple_identifier"
                )
            })
        } else {
            None
        }
    });
    explicit
        .map(|value| lower_type(value, source, language))
        .unwrap_or_else(TypeReference::dynamic)
}

pub(crate) fn lower_block(node: Node<'_>, source: &str, language: AstLanguage) -> Vec<Statement> {
    let container = direct_named_children(node)
        .into_iter()
        .find(|child| matches!(child.kind(), "statements" | "statement_list"));
    let owner = container.unwrap_or(node);
    let children = direct_named_children(owner);
    let last_id = children.last().map(Node::id);
    children
        .into_iter()
        .map(|child| {
            if language == AstLanguage::Rust
                && Some(child.id()) == last_id
                && !is_statement_kind(child.kind())
            {
                statement(
                    child,
                    StatementKind::Return(Some(lower_expression(child, source, language))),
                    source,
                )
            } else {
                lower_statement(child, source, language)
            }
        })
        .collect()
}

fn lower_statement(node: Node<'_>, source: &str, language: AstLanguage) -> Statement {
    let kind = match node.kind() {
        kind if is_block(kind) => StatementKind::Block(lower_block(node, source, language)),
        "lexical_declaration"
        | "variable_declaration"
        | "local_variable_declaration"
        | "let_declaration"
        | "property_declaration"
        | "short_var_declaration" => lower_variable(node, source, language),
        "expression_statement" => {
            let value = first_named_child(node).unwrap_or(node);
            if language == AstLanguage::Python && value.kind() == "assignment" {
                lower_assignment_as_statement(value, source, language)
            } else if language == AstLanguage::Rust && value.kind() == "for_expression" {
                lower_for_each(value, source, language)
            } else if language == AstLanguage::Rust && value.kind() == "while_expression" {
                lower_while(value, source, language)
            } else if language == AstLanguage::Rust && value.kind() == "if_expression" {
                lower_if(value, source, language)
            } else {
                StatementKind::Expression(lower_expression(value, source, language))
            }
        }
        "assignment_statement" => {
            StatementKind::Expression(lower_expression(node, source, language))
        }
        "if_statement" | "if_expression" => lower_if(node, source, language),
        "for_in_statement" | "enhanced_for_statement" | "for_statement" | "for_expression" => {
            if is_for_each(node, language) {
                lower_for_each(node, source, language)
            } else if language == AstLanguage::Go {
                lower_while(node, source, language)
            } else {
                StatementKind::Unlowered {
                    syntax_kind: node.kind().into(),
                }
            }
        }
        "while_statement" | "while_expression" => lower_while(node, source, language),
        "return_statement" => StatementKind::Return(first_named_child(node).map(|value| {
            let value = if value.kind() == "expression_list" {
                first_named_child(value).unwrap_or(value)
            } else {
                value
            };
            lower_expression(value, source, language)
        })),
        "control_transfer_statement" if text(node, source).trim_start().starts_with("return") => {
            StatementKind::Return(
                node.child_by_field_name("result")
                    .or_else(|| first_named_child(node))
                    .map(|value| lower_expression(value, source, language)),
            )
        }
        "break_statement" => StatementKind::Break,
        "continue_statement" => StatementKind::Continue,
        _ if is_expression_kind(node.kind()) => {
            StatementKind::Expression(lower_expression(node, source, language))
        }
        _ => StatementKind::Unlowered {
            syntax_kind: node.kind().into(),
        },
    };
    statement(node, kind, source)
}

fn lower_variable(node: Node<'_>, source: &str, language: AstLanguage) -> StatementKind {
    let declaration = match node.kind() {
        "lexical_declaration" | "variable_declaration" => find_first(node, "variable_declarator"),
        "local_variable_declaration" => node
            .child_by_field_name("declarator")
            .or_else(|| find_first(node, "variable_declarator")),
        _ => Some(node),
    };
    let Some(declaration) = declaration else {
        return StatementKind::Unlowered {
            syntax_kind: node.kind().into(),
        };
    };
    let name_node = match language {
        AstLanguage::Swift => declaration.child_by_field_name("name").and_then(|pattern| {
            pattern
                .child_by_field_name("bound_identifier")
                .or_else(|| first_named_child(pattern))
        }),
        AstLanguage::Go => declaration
            .child_by_field_name("left")
            .and_then(last_named_child),
        AstLanguage::Rust => declaration.child_by_field_name("pattern"),
        _ => declaration
            .child_by_field_name("name")
            .or_else(|| find_first(declaration, "identifier")),
    };
    let Some(name_node) = name_node else {
        return StatementKind::Unlowered {
            syntax_kind: node.kind().into(),
        };
    };
    let initializer_node = match language {
        AstLanguage::Swift => declaration.child_by_field_name("value"),
        AstLanguage::Go => declaration
            .child_by_field_name("right")
            .and_then(first_named_child),
        AstLanguage::Rust => declaration.child_by_field_name("value"),
        _ => declaration
            .child_by_field_name("value")
            .or_else(|| declaration.child_by_field_name("right")),
    };
    let type_node = declaration
        .child_by_field_name("type")
        .or_else(|| node.child_by_field_name("type"))
        .or_else(|| find_first(node, "type_annotation").and_then(last_named_child));
    let source_text = text(node, source).trim_start();
    StatementKind::Variable {
        name: text(name_node, source).into(),
        type_ref: type_node
            .map(|value| lower_type(value, source, language))
            .unwrap_or_else(TypeReference::dynamic),
        is_final: matches!(
            language,
            AstLanguage::JavaScript | AstLanguage::Swift | AstLanguage::Rust
        ) && (source_text.starts_with("const ") || source_text.starts_with("let ")),
        initializer: initializer_node.map(|value| lower_expression(value, source, language)),
    }
}

fn lower_assignment_as_statement(
    node: Node<'_>,
    source: &str,
    language: AstLanguage,
) -> StatementKind {
    let left = node
        .child_by_field_name("left")
        .or_else(|| first_named_child(node));
    let right = node
        .child_by_field_name("right")
        .or_else(|| last_named_child(node));
    let type_node = node.child_by_field_name("type");
    if let (Some(left), Some(right)) = (left, right) {
        if is_identifier(left) {
            return StatementKind::Variable {
                name: text(left, source).into(),
                type_ref: type_node
                    .map(|value| lower_type(value, source, language))
                    .unwrap_or_else(TypeReference::dynamic),
                is_final: false,
                initializer: Some(lower_expression(right, source, language)),
            };
        }
    }
    StatementKind::Expression(lower_expression(node, source, language))
}

fn lower_if(node: Node<'_>, source: &str, language: AstLanguage) -> StatementKind {
    let children = direct_named_children(node);
    let condition = node
        .child_by_field_name("condition")
        .or_else(|| children.first().copied());
    let then_node = node.child_by_field_name("consequence").or_else(|| {
        children
            .iter()
            .copied()
            .skip(1)
            .find(|child| is_block(child.kind()) || is_statement_kind(child.kind()))
    });
    let else_node = node.child_by_field_name("alternative").or_else(|| {
        then_node.and_then(|then_node| {
            children
                .iter()
                .copied()
                .skip_while(|child| child.id() != then_node.id())
                .nth(1)
        })
    });
    match (condition, then_node) {
        (Some(condition), Some(then_node)) => StatementKind::If {
            condition: lower_expression(unwrap_parenthesized(condition), source, language),
            then_branch: Box::new(lower_statement(then_node, source, language)),
            else_branch: else_node.map(|value| Box::new(lower_statement(value, source, language))),
        },
        _ => StatementKind::Unlowered {
            syntax_kind: node.kind().into(),
        },
    }
}

fn is_for_each(node: Node<'_>, language: AstLanguage) -> bool {
    match language {
        AstLanguage::Go => find_first(node, "range_clause").is_some(),
        AstLanguage::JavaScript => node.kind() == "for_in_statement",
        AstLanguage::Java => node.kind() == "enhanced_for_statement",
        AstLanguage::Python | AstLanguage::Dart | AstLanguage::Swift | AstLanguage::Rust => true,
    }
}

fn lower_for_each(node: Node<'_>, source: &str, language: AstLanguage) -> StatementKind {
    let range = if language == AstLanguage::Go {
        find_first(node, "range_clause").unwrap_or(node)
    } else {
        node
    };
    let variable = match language {
        AstLanguage::JavaScript => range.child_by_field_name("left"),
        AstLanguage::Java => range.child_by_field_name("name"),
        AstLanguage::Python => range.child_by_field_name("left"),
        AstLanguage::Dart => range
            .child_by_field_name("name")
            .or_else(|| range.child_by_field_name("pattern")),
        AstLanguage::Go => range.child_by_field_name("left").and_then(last_named_child),
        AstLanguage::Rust => range.child_by_field_name("pattern"),
        AstLanguage::Swift => range.child_by_field_name("item").and_then(|value| {
            value
                .child_by_field_name("bound_identifier")
                .or_else(|| first_named_child(value))
        }),
    };
    let iterable = match language {
        AstLanguage::JavaScript | AstLanguage::Python | AstLanguage::Go => {
            range.child_by_field_name("right")
        }
        AstLanguage::Java => range.child_by_field_name("value"),
        AstLanguage::Dart => range
            .child_by_field_name("value")
            .or_else(|| range.child_by_field_name("iterable")),
        AstLanguage::Rust => range.child_by_field_name("value"),
        AstLanguage::Swift => range.child_by_field_name("collection"),
    };
    let body = node.child_by_field_name("body").or_else(|| {
        direct_named_children(node)
            .into_iter()
            .rev()
            .find(|child| is_block(child.kind()))
    });
    match (variable, iterable, body) {
        (Some(variable), Some(iterable), Some(body)) => StatementKind::ForEach {
            variable: text(variable, source)
                .trim_start_matches("const ")
                .trim_start_matches("let ")
                .into(),
            iterable: lower_expression(iterable, source, language),
            body: Box::new(lower_statement(body, source, language)),
        },
        _ => StatementKind::Unlowered {
            syntax_kind: node.kind().into(),
        },
    }
}

fn lower_while(node: Node<'_>, source: &str, language: AstLanguage) -> StatementKind {
    let children = direct_named_children(node);
    let condition = node.child_by_field_name("condition").or_else(|| {
        children
            .iter()
            .copied()
            .find(|child| !is_block(child.kind()) && child.kind() != "range_clause")
    });
    let body = node.child_by_field_name("body").or_else(|| {
        children
            .iter()
            .copied()
            .rev()
            .find(|child| is_block(child.kind()))
    });
    match (condition, body) {
        (Some(condition), Some(body)) => StatementKind::While {
            condition: lower_expression(unwrap_parenthesized(condition), source, language),
            body: Box::new(lower_statement(body, source, language)),
        },
        _ => StatementKind::Unlowered {
            syntax_kind: node.kind().into(),
        },
    }
}

pub(crate) fn lower_expression(node: Node<'_>, source: &str, language: AstLanguage) -> Expression {
    let source_text = text(node, source).to_string();
    let kind = match node.kind() {
        kind if is_identifier_kind(kind) => ExpressionKind::Identifier(source_text.clone()),
        "this" | "self" | "self_expression" => ExpressionKind::Identifier(source_text.clone()),
        "true" | "false" | "true_literal" | "false_literal" => {
            ExpressionKind::Literal(Literal::Bool(source_text.eq_ignore_ascii_case("true")))
        }
        "null" | "null_literal" | "nil" | "none" => ExpressionKind::Literal(Literal::Null),
        "number"
        | "integer"
        | "int_literal"
        | "integer_literal"
        | "decimal_integer_literal"
        | "hex_integer_literal" => ExpressionKind::Literal(Literal::Integer(source_text.clone())),
        "float" | "float_literal" | "decimal_floating_point_literal" | "real_literal" => {
            ExpressionKind::Literal(Literal::Float(source_text.clone()))
        }
        "string" | "string_literal" | "line_string_literal" => {
            ExpressionKind::Literal(Literal::String(source_text.clone()))
        }
        "parenthesized_expression"
        | "expression_list"
        | "directly_assignable_expression"
        | "literal_element" => first_named_child(node)
            .map(|value| lower_expression(value, source, language).kind)
            .unwrap_or_else(|| ExpressionKind::Raw {
                syntax_kind: node.kind().into(),
            }),
        "binary_expression"
        | "binary_operator"
        | "comparison_operator"
        | "comparison_expression"
        | "additive_expression"
        | "multiplicative_expression"
        | "conjunction_expression"
        | "disjunction_expression" => lower_binary(node, source, language),
        "unary_expression" | "unary_operator" | "reference_expression" => {
            let operand = node
                .child_by_field_name("argument")
                .or_else(|| node.child_by_field_name("value"))
                .or_else(|| first_named_child(node));
            operand
                .map(|operand| ExpressionKind::Unary {
                    operator: operator_outside(node, operand, source),
                    operand: Box::new(lower_expression(operand, source, language)),
                })
                .unwrap_or_else(|| ExpressionKind::Raw {
                    syntax_kind: node.kind().into(),
                })
        }
        "await" | "await_expression" => first_named_child(node)
            .map(|value| ExpressionKind::Await(Box::new(lower_expression(value, source, language))))
            .unwrap_or_else(|| ExpressionKind::Raw {
                syntax_kind: node.kind().into(),
            }),
        "lambda" => lower_lambda(node, source, language),
        "assignment"
        | "assignment_expression"
        | "assignment_statement"
        | "augmented_assignment"
        | "augmented_assignment_expression"
        | "compound_assignment_expr" => lower_assignment(node, source, language),
        "subscript" | "index_expression" => lower_index_or_slice(node, source, language),
        "slice_expression" => lower_native_slice(node, source, language),
        "call_expression" | "call" | "method_invocation" => lower_call(node, source, language),
        "new_expression" | "object_creation_expression" => {
            lower_object_creation(node, source, language)
        }
        "struct_expression" => lower_struct_expression(node, source, language),
        "member_expression"
        | "field_expression"
        | "navigation_expression"
        | "field_access"
        | "attribute"
        | "selector_expression" => lower_member(node, source, language),
        "array" | "list" | "list_literal" | "array_literal" | "array_expression"
        | "composite_literal" => lower_list(node, source, language),
        "dictionary" | "dictionary_literal" | "map_literal" => lower_map(node, source, language),
        "set" => ExpressionKind::ObjectCreation {
            type_ref: TypeReference {
                name: "Set".into(),
                arguments: Vec::new(),
                nullable: false,
            },
            constructor: Some("literal".into()),
            arguments: direct_named_children(node)
                .into_iter()
                .map(|value| Argument {
                    name: None,
                    value: lower_expression(value, source, language),
                })
                .collect(),
            is_const: false,
        },
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

fn lower_binary(node: Node<'_>, source: &str, language: AstLanguage) -> ExpressionKind {
    let left = node
        .child_by_field_name("left")
        .or_else(|| node.child_by_field_name("lhs"))
        .or_else(|| first_named_child(node));
    let right = node
        .child_by_field_name("right")
        .or_else(|| node.child_by_field_name("rhs"))
        .or_else(|| last_named_child(node));
    match (left, right) {
        (Some(left), Some(right)) if left.id() != right.id() => ExpressionKind::Binary {
            operator: operator_between(left, right, source),
            left: Box::new(lower_expression(left, source, language)),
            right: Box::new(lower_expression(right, source, language)),
        },
        _ => ExpressionKind::Raw {
            syntax_kind: node.kind().into(),
        },
    }
}

fn lower_assignment(node: Node<'_>, source: &str, language: AstLanguage) -> ExpressionKind {
    let left = node
        .child_by_field_name("left")
        .or_else(|| node.child_by_field_name("target"))
        .or_else(|| first_named_child(node));
    let right = node
        .child_by_field_name("right")
        .or_else(|| node.child_by_field_name("result"))
        .or_else(|| last_named_child(node));
    match (left, right) {
        (Some(left), Some(right)) if left.id() != right.id() => ExpressionKind::Assignment {
            target: Box::new(lower_expression(unwrap_single(left), source, language)),
            operator: operator_between(left, right, source),
            value: Box::new(lower_expression(unwrap_single(right), source, language)),
        },
        _ => ExpressionKind::Raw {
            syntax_kind: node.kind().into(),
        },
    }
}

fn lower_index_or_slice(node: Node<'_>, source: &str, language: AstLanguage) -> ExpressionKind {
    let children = direct_named_children(node);
    let object = node
        .child_by_field_name("value")
        .or_else(|| node.child_by_field_name("operand"))
        .or_else(|| children.first().copied());
    let subscript = node
        .child_by_field_name("subscript")
        .or_else(|| node.child_by_field_name("index"))
        .or_else(|| children.get(1).copied());
    match (object, subscript) {
        (Some(object), Some(range)) if matches!(range.kind(), "slice" | "range_expression") => {
            lower_slice_call(object, range, node, source, language)
        }
        (Some(object), Some(index)) => {
            let lowered_index = if language == AstLanguage::Python
                && index.kind() == "unary_operator"
                && text(index, source).trim_start().starts_with('-')
            {
                lower_negative_index(object, index, source, language)
            } else {
                lower_expression(index, source, language)
            };
            ExpressionKind::Index {
                object: Box::new(lower_expression(object, source, language)),
                index: Box::new(lowered_index),
                null_aware: false,
            }
        }
        _ => ExpressionKind::Raw {
            syntax_kind: node.kind().into(),
        },
    }
}

fn lower_negative_index(
    object: Node<'_>,
    index: Node<'_>,
    source: &str,
    language: AstLanguage,
) -> Expression {
    let magnitude = first_named_child(index).unwrap_or(index);
    let object_expression = lower_expression(object, source, language);
    let length_call = Expression {
        kind: ExpressionKind::Call {
            callee: Box::new(Expression {
                kind: ExpressionKind::Identifier("len".into()),
                source: "len".into(),
                span: span(object),
            }),
            arguments: vec![Argument {
                name: None,
                value: object_expression,
            }],
            type_arguments: Vec::new(),
        },
        source: format!("len({})", text(object, source)),
        span: span(index),
    };
    Expression {
        kind: ExpressionKind::Binary {
            operator: "-".into(),
            left: Box::new(length_call),
            right: Box::new(lower_expression(magnitude, source, language)),
        },
        source: text(index, source).into(),
        span: span(index),
    }
}

fn lower_native_slice(node: Node<'_>, source: &str, language: AstLanguage) -> ExpressionKind {
    let object = node
        .child_by_field_name("operand")
        .or_else(|| first_named_child(node));
    let range = find_first(node, "range_expression");
    match (object, range) {
        (Some(object), Some(range)) => lower_slice_call(object, range, node, source, language),
        (Some(object), None) if language == AstLanguage::Go => {
            let callee = Expression {
                kind: ExpressionKind::Member {
                    object: Box::new(lower_expression(object, source, language)),
                    property: "slice".into(),
                    null_aware: false,
                },
                source: format!("{}.slice", text(object, source)),
                span: span(node),
            };
            let arguments = [
                node.child_by_field_name("start"),
                node.child_by_field_name("end"),
            ]
            .into_iter()
            .flatten()
            .map(|value| Argument {
                name: None,
                value: lower_expression(value, source, language),
            })
            .collect();
            ExpressionKind::Call {
                callee: Box::new(callee),
                arguments,
                type_arguments: Vec::new(),
            }
        }
        _ => ExpressionKind::Raw {
            syntax_kind: node.kind().into(),
        },
    }
}

fn lower_slice_call(
    object: Node<'_>,
    range: Node<'_>,
    owner: Node<'_>,
    source: &str,
    language: AstLanguage,
) -> ExpressionKind {
    let (start, end, step) = slice_bounds(range, source);
    let callee = Expression {
        kind: ExpressionKind::Member {
            object: Box::new(lower_expression(object, source, language)),
            property: "slice".into(),
            null_aware: false,
        },
        source: format!("{}.slice", text(object, source)),
        span: span(owner),
    };
    let arguments = [start, end, step]
        .into_iter()
        .flatten()
        .map(|value| Argument {
            name: None,
            value: lower_expression(value, source, language),
        })
        .collect();
    ExpressionKind::Call {
        callee: Box::new(callee),
        arguments,
        type_arguments: Vec::new(),
    }
}

fn slice_bounds<'tree>(
    range: Node<'tree>,
    source: &str,
) -> (
    Option<Node<'tree>>,
    Option<Node<'tree>>,
    Option<Node<'tree>>,
) {
    let children = direct_named_children(range);
    if range.kind() == "range_expression" {
        let start = range.child_by_field_name("start");
        let end = range.child_by_field_name("end");
        if start.is_some() || end.is_some() {
            return (start, end, None);
        }
        let raw = text(range, source).trim();
        return if raw.starts_with("..") {
            (None, children.first().copied(), None)
        } else if raw.ends_with("..") {
            (children.first().copied(), None, None)
        } else {
            (children.first().copied(), children.get(1).copied(), None)
        };
    }
    let raw = text(range, source);
    let leading = raw.trim_start().starts_with(':');
    let trailing = raw.trim_end().ends_with(':');
    match children.as_slice() {
        [] => (None, None, None),
        [only] if leading => (None, Some(*only), None),
        [only] if trailing => (Some(*only), None, None),
        [only] => (Some(*only), None, None),
        [first, second] => (Some(*first), Some(*second), None),
        [first, second, third, ..] => (Some(*first), Some(*second), Some(*third)),
    }
}

fn lower_call(node: Node<'_>, source: &str, language: AstLanguage) -> ExpressionKind {
    if language == AstLanguage::Swift {
        if let Some(object) = first_named_child(node) {
            let delimiter = source[object.end_byte()..node.end_byte()]
                .trim_start()
                .chars()
                .next();
            if delimiter == Some('[') {
                let value = find_arguments(node)
                    .and_then(|arguments| find_first(arguments, "value_argument"))
                    .and_then(|argument| {
                        argument
                            .child_by_field_name("value")
                            .or_else(|| first_named_child(argument))
                    });
                if let Some(value) = value {
                    if value.kind() == "range_expression" {
                        return lower_slice_call(object, value, node, source, language);
                    }
                    return ExpressionKind::Index {
                        object: Box::new(lower_expression(object, source, language)),
                        index: Box::new(lower_expression(value, source, language)),
                        null_aware: false,
                    };
                }
            }
        }
    }
    let callee = if node.kind() == "method_invocation" {
        let name = node.child_by_field_name("name");
        match (node.child_by_field_name("object"), name) {
            (Some(object), Some(name)) => Some(Expression {
                kind: ExpressionKind::Member {
                    object: Box::new(lower_expression(object, source, language)),
                    property: text(name, source).into(),
                    null_aware: false,
                },
                source: format!("{}.{}", text(object, source), text(name, source)),
                span: span(node),
            }),
            (None, Some(name)) => Some(lower_expression(name, source, language)),
            _ => None,
        }
    } else {
        node.child_by_field_name("function")
            .or_else(|| first_named_child(node))
            .map(|value| lower_expression(value, source, language))
    };
    let Some(callee) = callee else {
        return ExpressionKind::Raw {
            syntax_kind: node.kind().into(),
        };
    };
    let mut arguments = find_arguments(node)
        .map(|arguments| lower_arguments(arguments, source, language))
        .unwrap_or_default();

    let callee_name = match &callee.kind {
        ExpressionKind::Identifier(value) => Some(value.as_str()),
        ExpressionKind::Member { property, .. } => Some(property.as_str()),
        _ => None,
    };
    if callee_name == Some("make")
        && text(node, source).contains("map[")
        && text(node, source).contains("struct{}")
    {
        return ExpressionKind::ObjectCreation {
            type_ref: TypeReference {
                name: "Set".into(),
                arguments: Vec::new(),
                nullable: false,
            },
            constructor: None,
            arguments,
            is_const: false,
        };
    }
    if matches!(callee_name, Some("Map" | "HashMap" | "Dictionary"))
        || (callee_name == Some("make") && text(node, source).contains("map["))
        || text(node, source).contains("HashMap::new")
    {
        return ExpressionKind::MapLiteral {
            key_type: None,
            value_type: None,
            entries: Vec::new(),
        };
    }
    if matches!(callee_name, Some("Set" | "HashSet")) || text(node, source).contains("HashSet::") {
        return ExpressionKind::ObjectCreation {
            type_ref: TypeReference {
                name: "Set".into(),
                arguments: Vec::new(),
                nullable: false,
            },
            constructor: None,
            arguments,
            is_const: false,
        };
    }
    if matches!(callee_name, Some("Array" | "List")) {
        return ExpressionKind::ObjectCreation {
            type_ref: TypeReference {
                name: "List".into(),
                arguments: Vec::new(),
                nullable: false,
            },
            constructor: None,
            arguments,
            is_const: false,
        };
    }
    if let ExpressionKind::Member {
        object, property, ..
    } = &callee.kind
    {
        if property == "of"
            && matches!(&object.kind, ExpressionKind::Identifier(value) if value == "List")
        {
            return ExpressionKind::ListLiteral {
                element_type: None,
                elements: arguments
                    .into_iter()
                    .map(|argument| CollectionElement::Expression(argument.value))
                    .collect(),
            };
        }
        if matches!(property.as_str(), "slice" | "subList" | "sublist") {
            let slice_callee = Expression {
                kind: ExpressionKind::Member {
                    object: object.clone(),
                    property: "slice".into(),
                    null_aware: false,
                },
                source: callee.source.clone(),
                span: callee.span,
            };
            return ExpressionKind::Call {
                callee: Box::new(slice_callee),
                arguments,
                type_arguments: Vec::new(),
            };
        }
    }
    ExpressionKind::Call {
        callee: Box::new(callee),
        arguments: std::mem::take(&mut arguments),
        type_arguments: Vec::new(),
    }
}

fn find_arguments(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("arguments").or_else(|| {
        direct_named_children(node).into_iter().find(|child| {
            matches!(
                child.kind(),
                "arguments" | "argument_list" | "call_suffix" | "value_arguments"
            )
        })
    })
}

fn lower_arguments(node: Node<'_>, source: &str, language: AstLanguage) -> Vec<Argument> {
    let owner = if node.kind() == "call_suffix" {
        find_first(node, "value_arguments").unwrap_or(node)
    } else {
        node
    };
    direct_named_children(owner)
        .into_iter()
        .filter_map(|child| {
            if matches!(child.kind(), "type_arguments" | "map_type") {
                return None;
            }
            if child.kind() == "keyword_argument" {
                let name = child
                    .child_by_field_name("name")
                    .or_else(|| first_named_child(child));
                let value = child
                    .child_by_field_name("value")
                    .or_else(|| last_named_child(child));
                return Some(Argument {
                    name: name.map(|value| text(value, source).into()),
                    value: lower_expression(value?, source, language),
                });
            }
            let value = if child.kind() == "value_argument" {
                child
                    .child_by_field_name("value")
                    .or_else(|| first_named_child(child))?
            } else {
                child
            };
            Some(Argument {
                name: None,
                value: lower_expression(value, source, language),
            })
        })
        .collect()
}

fn lower_lambda(node: Node<'_>, source: &str, language: AstLanguage) -> ExpressionKind {
    let parameters = node
        .child_by_field_name("parameters")
        .map(|owner| {
            direct_named_children(owner)
                .into_iter()
                .filter_map(|parameter| lower_parameter(parameter, source, language))
                .collect()
        })
        .unwrap_or_default();
    let body = node
        .child_by_field_name("body")
        .or_else(|| last_named_child(node));
    match body {
        Some(body) => ExpressionKind::Closure {
            parameters,
            body: Box::new(Body {
                kind: BodyKind::Expression(lower_expression(body, source, language)),
                source: text(body, source).into(),
                syntax_kind: body.kind().into(),
                span: span(body),
            }),
        },
        None => ExpressionKind::Raw {
            syntax_kind: node.kind().into(),
        },
    }
}

fn lower_object_creation(node: Node<'_>, source: &str, language: AstLanguage) -> ExpressionKind {
    let type_node = node
        .child_by_field_name("constructor")
        .or_else(|| node.child_by_field_name("type"))
        .or_else(|| first_named_child(node));
    let type_ref = type_node
        .map(|value| lower_type(value, source, language))
        .unwrap_or_else(TypeReference::dynamic);
    let arguments = find_arguments(node)
        .map(|value| lower_arguments(value, source, language))
        .unwrap_or_default();
    match type_ref.name.as_str() {
        "Map" => ExpressionKind::MapLiteral {
            key_type: type_ref.arguments.first().cloned(),
            value_type: type_ref.arguments.get(1).cloned(),
            entries: Vec::new(),
        },
        "Set" => ExpressionKind::ObjectCreation {
            type_ref,
            constructor: None,
            arguments,
            is_const: false,
        },
        _ => ExpressionKind::ObjectCreation {
            type_ref,
            constructor: None,
            arguments,
            is_const: text(node, source).trim_start().starts_with("const "),
        },
    }
}

fn lower_struct_expression(node: Node<'_>, source: &str, language: AstLanguage) -> ExpressionKind {
    let type_node = node
        .child_by_field_name("name")
        .or_else(|| first_named_child(node));
    let type_ref = type_node
        .map(|value| lower_type(value, source, language))
        .unwrap_or_else(TypeReference::dynamic);
    let arguments = node
        .child_by_field_name("body")
        .map(|body| {
            direct_named_children(body)
                .into_iter()
                .filter_map(|field| match field.kind() {
                    "field_initializer" => Some(Argument {
                        name: field_text(field, "field", source)
                            .or_else(|| field_text(field, "name", source)),
                        value: field
                            .child_by_field_name("value")
                            .map(|value| lower_expression(value, source, language))?,
                    }),
                    "shorthand_field_initializer" => {
                        let value = first_named_child(field)?;
                        Some(Argument {
                            name: Some(text(value, source).into()),
                            value: lower_expression(value, source, language),
                        })
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    ExpressionKind::ObjectCreation {
        type_ref,
        constructor: None,
        arguments,
        is_const: false,
    }
}

fn lower_member(node: Node<'_>, source: &str, language: AstLanguage) -> ExpressionKind {
    let object = node
        .child_by_field_name("object")
        .or_else(|| node.child_by_field_name("value"))
        .or_else(|| node.child_by_field_name("target"))
        .or_else(|| first_named_child(node));
    let property = node
        .child_by_field_name("property")
        .or_else(|| node.child_by_field_name("field"))
        .or_else(|| node.child_by_field_name("suffix"))
        .or_else(|| last_named_child(node));
    match (object, property) {
        (Some(object), Some(property)) if object.id() != property.id() => ExpressionKind::Member {
            object: Box::new(lower_expression(object, source, language)),
            property: text(property, source).trim_start_matches('.').into(),
            null_aware: text(node, source).contains("?."),
        },
        _ => ExpressionKind::Raw {
            syntax_kind: node.kind().into(),
        },
    }
}

fn lower_list(node: Node<'_>, source: &str, language: AstLanguage) -> ExpressionKind {
    if node.kind() == "composite_literal" {
        if let Some(type_node) = node.child_by_field_name("type") {
            if type_node.kind() == "map_type" {
                let is_set = text(type_node, source).contains("struct{}");
                if is_set {
                    return ExpressionKind::ObjectCreation {
                        type_ref: TypeReference {
                            name: "Set".into(),
                            arguments: Vec::new(),
                            nullable: false,
                        },
                        constructor: Some("literal".into()),
                        arguments: Vec::new(),
                        is_const: false,
                    };
                }
                return ExpressionKind::MapLiteral {
                    key_type: type_node
                        .child_by_field_name("key")
                        .map(|value| lower_type(value, source, language)),
                    value_type: type_node
                        .child_by_field_name("value")
                        .map(|value| lower_type(value, source, language)),
                    entries: Vec::new(),
                };
            }
            if !matches!(type_node.kind(), "slice_type" | "array_type") {
                let arguments = node
                    .child_by_field_name("body")
                    .or_else(|| last_named_child(node))
                    .map(|body| {
                        direct_named_children(body)
                            .into_iter()
                            .filter_map(|element| {
                                if element.kind() != "keyed_element" {
                                    return None;
                                }
                                let key = element
                                    .child_by_field_name("key")
                                    .and_then(first_named_child);
                                let value = element
                                    .child_by_field_name("value")
                                    .and_then(first_named_child)?;
                                Some(Argument {
                                    name: key.map(|value| text(value, source).into()),
                                    value: lower_expression(value, source, language),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                return ExpressionKind::ObjectCreation {
                    type_ref: lower_type(type_node, source, language),
                    constructor: None,
                    arguments,
                    is_const: false,
                };
            }
        }
        if let Some(body) = last_named_child(node) {
            return ExpressionKind::ListLiteral {
                element_type: node
                    .child_by_field_name("type")
                    .map(|value| lower_type(value, source, language)),
                elements: direct_named_children(body)
                    .into_iter()
                    .map(|value| {
                        CollectionElement::Expression(lower_expression(value, source, language))
                    })
                    .collect(),
            };
        }
    }
    ExpressionKind::ListLiteral {
        element_type: None,
        elements: direct_named_children(node)
            .into_iter()
            .map(|value| CollectionElement::Expression(lower_expression(value, source, language)))
            .collect(),
    }
}

fn lower_map(node: Node<'_>, source: &str, language: AstLanguage) -> ExpressionKind {
    let mut entries = Vec::new();
    for child in direct_named_children(node) {
        let parts = direct_named_children(child);
        if parts.len() >= 2 {
            entries.push((
                lower_expression(parts[0], source, language),
                lower_expression(*parts.last().unwrap(), source, language),
            ));
        }
    }
    ExpressionKind::MapLiteral {
        key_type: None,
        value_type: None,
        entries,
    }
}

pub(crate) fn lower_type(node: Node<'_>, source: &str, language: AstLanguage) -> TypeReference {
    let raw = text(node, source).trim();
    match node.kind() {
        "type_annotation" | "type" => last_named_child(node)
            .map(|value| lower_type(value, source, language))
            .unwrap_or_else(|| type_from_text(raw, language)),
        "array_type" | "slice_type" => {
            let element = node
                .child_by_field_name("element")
                .or_else(|| last_named_child(node))
                .map(|value| lower_type(value, source, language))
                .unwrap_or_else(TypeReference::dynamic);
            TypeReference {
                name: "List".into(),
                arguments: vec![element],
                nullable: false,
            }
        }
        "dictionary_type" | "map_type" => {
            let children = direct_named_children(node);
            let key = node
                .child_by_field_name("key")
                .or_else(|| children.first().copied());
            let value = node
                .child_by_field_name("value")
                .or_else(|| children.get(1).copied());
            TypeReference {
                name: "Map".into(),
                arguments: [key, value]
                    .into_iter()
                    .flatten()
                    .map(|value| lower_type(value, source, language))
                    .collect(),
                nullable: false,
            }
        }
        "generic_type" => {
            let base = node
                .child_by_field_name("type")
                .or_else(|| first_named_child(node))
                .map(|value| text(value, source))
                .unwrap_or(raw);
            let arguments_node = node
                .child_by_field_name("type_arguments")
                .or_else(|| find_first(node, "type_arguments"))
                .or_else(|| find_first(node, "type_parameter"));
            let arguments = arguments_node
                .map(|arguments| {
                    direct_named_children(arguments)
                        .into_iter()
                        .map(|value| {
                            let value = if value.kind() == "type" {
                                first_named_child(value).unwrap_or(value)
                            } else {
                                value
                            };
                            lower_type(value, source, language)
                        })
                        .collect()
                })
                .unwrap_or_default();
            TypeReference {
                name: canonical_type_name(base),
                arguments,
                nullable: false,
            }
        }
        _ => type_from_text(raw, language),
    }
}

pub(crate) fn type_from_text(raw: &str, language: AstLanguage) -> TypeReference {
    let value = raw
        .trim()
        .trim_start_matches(':')
        .trim()
        .trim_end_matches('?');
    if language == AstLanguage::Swift && value.starts_with('[') && value.ends_with(']') {
        let inner = &value[1..value.len() - 1];
        if let Some(index) = top_level_separator(inner, ':') {
            return TypeReference {
                name: "Map".into(),
                arguments: vec![
                    type_from_text(&inner[..index], language),
                    type_from_text(&inner[index + 1..], language),
                ],
                nullable: raw.trim().ends_with('?'),
            };
        }
        return TypeReference {
            name: "List".into(),
            arguments: vec![type_from_text(inner, language)],
            nullable: raw.trim().ends_with('?'),
        };
    }
    if language == AstLanguage::Go && value.starts_with("[]") {
        return TypeReference {
            name: "List".into(),
            arguments: vec![type_from_text(&value[2..], language)],
            nullable: false,
        };
    }
    if let Some(open) = value.find(['<', '[']) {
        let close = if value.as_bytes()[open] == b'<' {
            '>'
        } else {
            ']'
        };
        if value.ends_with(close) {
            let arguments = split_top_level(&value[open + 1..value.len() - 1])
                .into_iter()
                .map(|part| type_from_text(part, language))
                .collect();
            return TypeReference {
                name: canonical_type_name(value[..open].trim()),
                arguments,
                nullable: raw.trim().ends_with('?'),
            };
        }
    }
    TypeReference {
        name: canonical_type_name(value),
        arguments: Vec::new(),
        nullable: raw.trim().ends_with('?'),
    }
}

fn canonical_type_name(name: &str) -> String {
    let leaf = name.rsplit("::").next().unwrap_or(name).trim();
    match leaf {
        "Array" | "ArrayList" | "List" | "Vec" | "list" => "List".into(),
        "Dictionary" | "HashMap" | "Map" | "dict" | "map" => "Map".into(),
        "HashSet" | "Set" | "set" => "Set".into(),
        other => other.into(),
    }
}

pub(crate) fn split_top_level(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (index, character) in value.char_indices() {
        match character {
            '<' | '[' | '(' => depth += 1,
            '>' | ']' | ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(value[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    if start < value.len() {
        parts.push(value[start..].trim());
    }
    parts
}

fn top_level_separator(value: &str, separator: char) -> Option<usize> {
    let mut depth = 0i32;
    for (index, character) in value.char_indices() {
        match character {
            '<' | '[' | '(' => depth += 1,
            '>' | ']' | ')' => depth -= 1,
            character if character == separator && depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn statement(node: Node<'_>, kind: StatementKind, source: &str) -> Statement {
    Statement {
        kind,
        source: text(node, source).into(),
        span: span(node),
    }
}

pub(crate) fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or_default()
}

pub(crate) fn field_text(node: Node<'_>, field: &str, source: &str) -> Option<String> {
    node.child_by_field_name(field)
        .map(|value| text(value, source).to_string())
}

pub(crate) fn direct_named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

pub(crate) fn direct_child_of_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    direct_named_children(node)
        .into_iter()
        .find(|child| child.kind() == kind)
}

pub(crate) fn first_named_child(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    let child = node.named_children(&mut cursor).next();
    child
}

pub(crate) fn last_named_child(node: Node<'_>) -> Option<Node<'_>> {
    direct_named_children(node).last().copied()
}

pub(crate) fn find_first<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind {
        return Some(node);
    }
    for child in direct_named_children(node) {
        if let Some(found) = find_first(child, kind) {
            return Some(found);
        }
    }
    None
}

pub(crate) fn walk_named(node: Node<'_>, visitor: &mut impl FnMut(Node<'_>)) {
    for child in direct_named_children(node) {
        visitor(child);
        walk_named(child, visitor);
    }
}

pub(crate) fn unwrap_parenthesized(node: Node<'_>) -> Node<'_> {
    if node.kind() == "parenthesized_expression" {
        first_named_child(node).unwrap_or(node)
    } else {
        node
    }
}

fn unwrap_single(node: Node<'_>) -> Node<'_> {
    if matches!(
        node.kind(),
        "expression_list" | "directly_assignable_expression"
    ) {
        first_named_child(node).unwrap_or(node)
    } else {
        node
    }
}

fn is_identifier(node: Node<'_>) -> bool {
    is_identifier_kind(node.kind())
}

fn is_identifier_kind(kind: &str) -> bool {
    matches!(
        kind,
        "identifier"
            | "simple_identifier"
            | "type_identifier"
            | "field_identifier"
            | "property_identifier"
            | "bound_identifier"
            | "scoped_identifier"
    )
}

fn is_block(kind: &str) -> bool {
    matches!(
        kind,
        "block" | "statement_block" | "function_body" | "statements" | "statement_list"
    )
}

fn is_statement_kind(kind: &str) -> bool {
    kind.ends_with("_statement")
        || matches!(
            kind,
            "block"
                | "statement_block"
                | "let_declaration"
                | "property_declaration"
                | "lexical_declaration"
                | "variable_declaration"
                | "local_variable_declaration"
                | "short_var_declaration"
        )
}

fn is_expression_kind(kind: &str) -> bool {
    kind.ends_with("_expression")
        || matches!(
            kind,
            "assignment"
                | "augmented_assignment"
                | "call"
                | "subscript"
                | "identifier"
                | "simple_identifier"
                | "integer"
                | "int_literal"
                | "integer_literal"
                | "decimal_integer_literal"
                | "string"
                | "string_literal"
        )
}

pub(crate) fn operator_between(left: Node<'_>, right: Node<'_>, source: &str) -> String {
    source
        .get(left.end_byte()..right.start_byte())
        .unwrap_or_default()
        .trim()
        .into()
}

fn operator_outside(owner: Node<'_>, operand: Node<'_>, source: &str) -> String {
    let prefix = source[owner.start_byte()..operand.start_byte()].trim();
    let suffix = source[operand.end_byte()..owner.end_byte()].trim();
    if prefix.is_empty() {
        suffix.into()
    } else {
        prefix.into()
    }
}

pub(crate) fn span(node: Node<'_>) -> SourceSpan {
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

pub(crate) fn collect_syntax_errors(
    node: Node<'_>,
    source: &str,
    language: AstLanguage,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if node.is_error() || node.is_missing() {
        diagnostics.push(Diagnostic {
            code: language.diagnostic_code(),
            severity: Severity::Error,
            message: format!("Syntax error near `{}`", text(node, source).trim()),
            span: span(node),
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_syntax_errors(child, source, language, diagnostics);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typed_ir::{ClassMember, Declaration};
    use crate::Language;

    #[derive(Default, Debug)]
    struct Audit {
        list: bool,
        map: bool,
        set: bool,
        branch: bool,
        iterator: bool,
        while_loop: bool,
        slice: bool,
        recursion: bool,
        raw: Vec<String>,
        unlowered: Vec<String>,
    }

    #[test]
    fn canonical_algorithms_lower_losslessly_for_all_seven_sources() {
        let cases = [
            (
                "JavaScript",
                crate::frontend::parse_source(
                    include_str!("../../tests/fixtures/canonical.js"),
                    Language::JavaScript,
                ),
            ),
            (
                "Java",
                crate::frontend::parse_source(
                    include_str!("../../tests/fixtures/canonical.java"),
                    Language::Java,
                ),
            ),
            (
                "Dart",
                crate::frontend::parse_source(
                    include_str!("../../tests/fixtures/canonical.dart"),
                    Language::Dart,
                ),
            ),
            (
                "Swift",
                crate::frontend::parse_source(
                    include_str!("../../tests/fixtures/canonical.swift"),
                    Language::Swift,
                ),
            ),
            (
                "Python",
                crate::frontend::parse_source(
                    include_str!("../../tests/fixtures/canonical.py"),
                    Language::Python,
                ),
            ),
            (
                "Go",
                crate::frontend::parse_source(
                    include_str!("../../tests/fixtures/canonical.go"),
                    Language::Go,
                ),
            ),
            (
                "Rust",
                crate::frontend::parse_source(
                    include_str!("../../tests/fixtures/canonical.rs"),
                    Language::Rust,
                ),
            ),
        ];
        for (language, unit) in cases {
            assert!(
                unit.diagnostics.is_empty(),
                "{language}: {:#?}",
                unit.diagnostics
            );
            let mut audit = Audit::default();
            for declaration in &unit.declarations {
                audit_declaration(declaration, &mut audit);
            }
            assert!(audit.list, "{language} lost its list: {audit:#?}");
            assert!(audit.map, "{language} lost its map: {audit:#?}");
            assert!(audit.set, "{language} lost its set: {audit:#?}");
            assert!(audit.branch, "{language} lost its conditional: {audit:#?}");
            assert!(
                audit.iterator,
                "{language} lost its iterator loop: {audit:#?}"
            );
            assert!(
                audit.while_loop,
                "{language} lost its while loop: {audit:#?}"
            );
            assert!(audit.slice, "{language} lost its slice: {audit:#?}");
            assert!(
                audit.recursion,
                "{language} lost its recursive call: {audit:#?}"
            );
            assert!(
                audit.unlowered.is_empty(),
                "{language} has unlowered statements: {audit:#?}"
            );
            assert!(
                audit.raw.is_empty(),
                "{language} has raw expressions: {audit:#?}"
            );
        }
    }

    #[test]
    fn comprehensive_six_language_frontends_keep_structured_algorithm_ir() {
        let cases = [
            (
                "JavaScript",
                crate::frontend::parse_source(
                    include_str!("../../tests/fixtures/comprehensive.js"),
                    Language::JavaScript,
                ),
            ),
            (
                "Java",
                crate::frontend::parse_source(
                    include_str!("../../tests/fixtures/comprehensive.java"),
                    Language::Java,
                ),
            ),
            (
                "Swift",
                crate::frontend::parse_source(
                    include_str!("../../tests/fixtures/comprehensive.swift"),
                    Language::Swift,
                ),
            ),
            (
                "Python",
                crate::frontend::parse_source(
                    include_str!("../../tests/fixtures/comprehensive.py"),
                    Language::Python,
                ),
            ),
            (
                "Go",
                crate::frontend::parse_source(
                    include_str!("../../tests/fixtures/comprehensive.go"),
                    Language::Go,
                ),
            ),
            (
                "Rust",
                crate::frontend::parse_source(
                    include_str!("../../tests/fixtures/comprehensive.rs"),
                    Language::Rust,
                ),
            ),
        ];
        for (language, unit) in cases {
            let mut audit = Audit::default();
            for declaration in &unit.declarations {
                audit_declaration(declaration, &mut audit);
            }
            assert!(
                audit.list && audit.map && audit.set,
                "{language}: {audit:#?}"
            );
            assert!(
                audit.branch && audit.iterator && audit.while_loop,
                "{language}: {audit:#?}"
            );
            assert!(audit.slice && audit.recursion, "{language}: {audit:#?}");
            assert!(audit.raw.is_empty(), "{language} raw IR: {audit:#?}");
            assert!(
                audit.unlowered.is_empty(),
                "{language} unlowered IR: {audit:#?}"
            );
        }
    }

    #[test]
    fn python_negative_index_uses_length_relative_universal_index() {
        let unit = crate::frontend::parse_source(
            include_str!("../../tests/fixtures/canonical.py"),
            Language::Python,
        );
        let mut found = false;
        visit_unit_expressions(&unit, &mut |expression| {
            if let ExpressionKind::Index { index, .. } = &expression.kind {
                if matches!(
                    &index.kind,
                    ExpressionKind::Binary { operator, left, .. }
                        if operator == "-" && matches!(left.kind, ExpressionKind::Call { .. })
                ) {
                    found = true;
                }
            }
        });
        assert!(
            found,
            "negative index did not lower to len(collection) - offset"
        );
    }

    fn audit_declaration(declaration: &Declaration, audit: &mut Audit) {
        match declaration {
            Declaration::Function(function) => audit_function(function, audit),
            Declaration::Class(class) | Declaration::Mixin(class) => {
                for member in &class.members {
                    match member {
                        ClassMember::Method(function)
                        | ClassMember::Getter(function)
                        | ClassMember::Setter(function)
                        | ClassMember::Operator(function) => audit_function(function, audit),
                        ClassMember::Constructor(constructor) => {
                            if let Some(body) = &constructor.body {
                                audit_body(body, audit);
                            }
                        }
                        ClassMember::Field(field) => {
                            audit_type(&field.type_ref, audit);
                            if let Some(initializer) = &field.initializer {
                                audit_body(initializer, audit);
                            }
                        }
                        ClassMember::Unlowered { syntax_kind, .. } => {
                            audit.unlowered.push(syntax_kind.clone())
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn audit_function(function: &FunctionDeclaration, audit: &mut Audit) {
        audit_type(&function.return_type, audit);
        for parameter in &function.parameters {
            audit_type(&parameter.type_ref, audit);
        }
        if let Some(body) = &function.body {
            audit_body(body, audit);
        }
    }

    fn audit_type(value: &TypeReference, audit: &mut Audit) {
        match value.name.as_str() {
            "List" => audit.list = true,
            "Map" => audit.map = true,
            "Set" => audit.set = true,
            _ => {}
        }
        for argument in &value.arguments {
            audit_type(argument, audit);
        }
    }

    fn audit_body(body: &Body, audit: &mut Audit) {
        match &body.kind {
            BodyKind::Block(statements) => statements
                .iter()
                .for_each(|value| audit_statement(value, audit)),
            BodyKind::Expression(value) => audit_expression(value, audit),
            BodyKind::Unlowered => audit.unlowered.push(body.syntax_kind.clone()),
            BodyKind::Empty => {}
        }
    }

    fn audit_statement(statement: &Statement, audit: &mut Audit) {
        match &statement.kind {
            StatementKind::Block(values) => values
                .iter()
                .for_each(|value| audit_statement(value, audit)),
            StatementKind::Variable {
                type_ref,
                initializer,
                ..
            } => {
                audit_type(type_ref, audit);
                if let Some(value) = initializer {
                    audit_expression(value, audit);
                }
            }
            StatementKind::Expression(value)
            | StatementKind::Throw(value)
            | StatementKind::Assert(value) => audit_expression(value, audit),
            StatementKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                audit.branch = true;
                audit_expression(condition, audit);
                audit_statement(then_branch, audit);
                if let Some(value) = else_branch {
                    audit_statement(value, audit);
                }
            }
            StatementKind::ForEach { iterable, body, .. } => {
                audit.iterator = true;
                audit_expression(iterable, audit);
                audit_statement(body, audit);
            }
            StatementKind::While { condition, body }
            | StatementKind::DoWhile { condition, body } => {
                audit.while_loop = true;
                audit_expression(condition, audit);
                audit_statement(body, audit);
            }
            StatementKind::Return(value) => {
                if let Some(value) = value {
                    audit_expression(value, audit);
                }
            }
            StatementKind::Unlowered { syntax_kind } => audit.unlowered.push(syntax_kind.clone()),
            StatementKind::Switch { .. }
            | StatementKind::Try { .. }
            | StatementKind::Break
            | StatementKind::Continue => {}
        }
    }

    fn audit_expression(expression: &Expression, audit: &mut Audit) {
        match &expression.kind {
            ExpressionKind::Raw { syntax_kind } => audit.raw.push(syntax_kind.clone()),
            ExpressionKind::ListLiteral { elements, .. } => {
                audit.list = true;
                for element in elements {
                    audit_collection_element(element, audit);
                }
            }
            ExpressionKind::MapLiteral { entries, .. } => {
                audit.map = true;
                for (key, value) in entries {
                    audit_expression(key, audit);
                    audit_expression(value, audit);
                }
            }
            ExpressionKind::ObjectCreation {
                type_ref,
                arguments,
                ..
            } => {
                audit_type(type_ref, audit);
                for argument in arguments {
                    audit_expression(&argument.value, audit);
                }
            }
            ExpressionKind::Call {
                callee, arguments, ..
            } => {
                if matches!(&callee.kind, ExpressionKind::Member { property, .. } if property == "slice")
                {
                    audit.slice = true;
                }
                if matches!(&callee.kind, ExpressionKind::Identifier(name) if name.eq_ignore_ascii_case("solve"))
                    || matches!(&callee.kind, ExpressionKind::Member { property, .. } if property.eq_ignore_ascii_case("solve"))
                {
                    audit.recursion = true;
                }
                audit_expression(callee, audit);
                for argument in arguments {
                    audit_expression(&argument.value, audit);
                }
            }
            ExpressionKind::Binary { left, right, .. } | ExpressionKind::IfNull { left, right } => {
                audit_expression(left, audit);
                audit_expression(right, audit);
            }
            ExpressionKind::Unary { operand, .. } | ExpressionKind::Await(operand) => {
                audit_expression(operand, audit)
            }
            ExpressionKind::Assignment { target, value, .. } => {
                audit_expression(target, audit);
                audit_expression(value, audit);
            }
            ExpressionKind::Member { object, .. } => audit_expression(object, audit),
            ExpressionKind::Index { object, index, .. } => {
                audit_expression(object, audit);
                audit_expression(index, audit);
            }
            ExpressionKind::Cast {
                expression,
                type_ref,
            }
            | ExpressionKind::TypeTest {
                expression,
                type_ref,
                ..
            } => {
                audit_expression(expression, audit);
                audit_type(type_ref, audit);
            }
            ExpressionKind::Cascade { target, sections } => {
                audit_expression(target, audit);
                sections
                    .iter()
                    .for_each(|value| audit_expression(value, audit));
            }
            ExpressionKind::Closure { body, .. } => audit_body(body, audit),
            ExpressionKind::StringInterpolation(_)
            | ExpressionKind::Switch { .. }
            | ExpressionKind::Identifier(_)
            | ExpressionKind::Literal(_) => {}
        }
    }

    fn audit_collection_element(element: &CollectionElement, audit: &mut Audit) {
        match element {
            CollectionElement::Expression(value)
            | CollectionElement::Spread {
                expression: value, ..
            } => audit_expression(value, audit),
        }
    }

    fn visit_unit_expressions(unit: &CompilationUnit, visitor: &mut impl FnMut(&Expression)) {
        fn visit_expression(value: &Expression, visitor: &mut impl FnMut(&Expression)) {
            visitor(value);
            match &value.kind {
                ExpressionKind::Binary { left, right, .. }
                | ExpressionKind::IfNull { left, right } => {
                    visit_expression(left, visitor);
                    visit_expression(right, visitor);
                }
                ExpressionKind::Unary { operand, .. } | ExpressionKind::Await(operand) => {
                    visit_expression(operand, visitor)
                }
                ExpressionKind::Assignment { target, value, .. } => {
                    visit_expression(target, visitor);
                    visit_expression(value, visitor);
                }
                ExpressionKind::Member { object, .. } => visit_expression(object, visitor),
                ExpressionKind::Index { object, index, .. } => {
                    visit_expression(object, visitor);
                    visit_expression(index, visitor);
                }
                ExpressionKind::Call {
                    callee, arguments, ..
                } => {
                    visit_expression(callee, visitor);
                    for argument in arguments {
                        visit_expression(&argument.value, visitor);
                    }
                }
                _ => {}
            }
        }
        fn visit_statement(value: &Statement, visitor: &mut impl FnMut(&Expression)) {
            match &value.kind {
                StatementKind::Block(values) => values
                    .iter()
                    .for_each(|value| visit_statement(value, visitor)),
                StatementKind::Variable { initializer, .. } => {
                    if let Some(value) = initializer {
                        visit_expression(value, visitor);
                    }
                }
                StatementKind::Expression(value)
                | StatementKind::Throw(value)
                | StatementKind::Assert(value) => visit_expression(value, visitor),
                StatementKind::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    visit_expression(condition, visitor);
                    visit_statement(then_branch, visitor);
                    if let Some(value) = else_branch {
                        visit_statement(value, visitor);
                    }
                }
                StatementKind::ForEach { iterable, body, .. } => {
                    visit_expression(iterable, visitor);
                    visit_statement(body, visitor);
                }
                StatementKind::While { condition, body }
                | StatementKind::DoWhile { condition, body } => {
                    visit_expression(condition, visitor);
                    visit_statement(body, visitor);
                }
                StatementKind::Return(value) => {
                    if let Some(value) = value {
                        visit_expression(value, visitor);
                    }
                }
                _ => {}
            }
        }
        for declaration in &unit.declarations {
            let functions: Vec<&FunctionDeclaration> = match declaration {
                Declaration::Function(value) => vec![value],
                Declaration::Class(value) => value
                    .members
                    .iter()
                    .filter_map(|member| {
                        if let ClassMember::Method(value) = member {
                            Some(value)
                        } else {
                            None
                        }
                    })
                    .collect(),
                _ => Vec::new(),
            };
            for function in functions {
                if let Some(Body {
                    kind: BodyKind::Block(statements),
                    ..
                }) = &function.body
                {
                    statements
                        .iter()
                        .for_each(|value| visit_statement(value, visitor));
                }
            }
        }
    }
}
