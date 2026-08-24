use crate::backend::{
    emit_comments, is_python_main_guard, unsupported_diagnostics, Backend, BackendOutput,
};
use crate::typed_ir::{
    Argument, Body, BodyKind, ClassDeclaration, ClassMember, CollectionElement, CompilationUnit,
    Declaration, Expression, ExpressionKind, IntrinsicOperation, Literal, Parameter, ParameterKind,
    PatternKind, Statement, StatementKind, StringPart, TypeReference,
};

/// Emits Dart exclusively from Universal IR. This is used for Python-to-Dart translation so
/// multiline calls and nested named arguments are never reconstructed from source lines.
pub struct DartBackend;

impl Backend for DartBackend {
    fn emit(&self, unit: &CompilationUnit) -> BackendOutput {
        let imports = unit
            .imports
            .iter()
            .filter(|value| !matches!(value.uri.as_str(), "__future__" | "typing"))
            .filter(|value| !crate::collection_ir::is_standard_collection_import(&value.uri))
            .map(emit_import)
            .collect::<Vec<_>>();
        let mut declarations = unit
            .declarations
            .iter()
            .filter_map(|declaration| {
                let value = match declaration {
                    Declaration::Class(value) | Declaration::Mixin(value) => emit_class(value),
                    Declaration::Enum(value) => format!(
                        "enum {} {{\n{}\n}}",
                        value.name,
                        indent(&value.values.join(",\n"), 1)
                    ),
                    Declaration::Extension(value) => format!(
                        "extension {} on {} {{\n{}\n}}",
                        value.name,
                        emit_type(&value.on_type),
                        emit_members(&value.members)
                    ),
                    Declaration::TypeAlias(value) => {
                        format!(
                            "typedef {} = {};",
                            value.name,
                            emit_type(&value.aliased_type)
                        )
                    }
                    Declaration::Function(value) => emit_function(value, 0),
                };
                (!value.is_empty()).then_some(value)
            })
            .collect::<Vec<_>>();
        if !unit.top_level_statements.is_empty() {
            let has_main = unit
                .declarations
                .iter()
                .any(|value| matches!(value, Declaration::Function(value) if value.name == "main"));
            declarations.push(format!(
                "void {}() {{\n{}\n}}",
                if has_main {
                    "_runTopLevelStatements"
                } else {
                    "main"
                },
                indent(
                    &unit
                        .top_level_statements
                        .iter()
                        .filter(|value| !is_python_main_guard(value))
                        .map(emit_statement)
                        .collect::<Vec<_>>()
                        .join("\n"),
                    1
                )
            ));
        }
        let needs_collection_import = declarations.iter().any(|value| value.contains("Queue"));
        let needs_contains_runtime = declarations
            .iter()
            .any(|value| value.contains("_tcContains("));
        let code = [
            (!unit.comments.is_empty())
                .then(|| emit_comments(&unit.comments, crate::Language::Dart)),
            (needs_collection_import || !imports.is_empty()).then(|| {
                let mut values = imports.clone();
                if needs_collection_import {
                    values.insert(0, "import 'dart:collection';".into());
                }
                values.join("\n")
            }),
            needs_contains_runtime.then(|| {
                "bool _tcContains(Object? collection, Object? value) {\n  if (collection is Map) return collection.containsKey(value);\n  if (collection is Iterable) return collection.contains(value);\n  return false;\n}"
                    .into()
            }),
            (!declarations.is_empty()).then(|| declarations.join("\n\n")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n\n");
        BackendOutput {
            code,
            diagnostics: unsupported_diagnostics(unit),
        }
    }
}

fn emit_import(value: &crate::typed_ir::ImportDeclaration) -> String {
    let mut uri = value.uri.clone();
    if uri.starts_with("package.") {
        uri = format!(
            "package:{}{}.dart",
            uri.trim_start_matches("package.").replace('.', "/"),
            ""
        );
    } else if uri.starts_with("dart.") {
        uri = format!("dart:{}", uri.trim_start_matches("dart.").replace('.', "/"));
    } else if !uri.contains(':') && !uri.contains('/') {
        uri = format!("{}.dart", uri.replace('.', "/"));
    }
    let mut result = format!("import '{}';", uri);
    if let Some(prefix) = &value.prefix {
        result.insert_str(result.len() - 1, &format!(" as {}", prefix));
    }
    if !value.show.is_empty() {
        result.insert_str(
            result.len() - 1,
            &format!(" show {}", value.show.join(", ")),
        );
    }
    if !value.hide.is_empty() {
        result.insert_str(
            result.len() - 1,
            &format!(" hide {}", value.hide.join(", ")),
        );
    }
    result
}

fn emit_class(class: &ClassDeclaration) -> String {
    let mut relationships = String::new();
    if let Some(parent) = &class.extends {
        relationships.push_str(&format!(" extends {}", emit_type(parent)));
    }
    if !class.mixins.is_empty() {
        relationships.push_str(&format!(
            " with {}",
            class
                .mixins
                .iter()
                .map(emit_type)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !class.implements.is_empty() {
        relationships.push_str(&format!(
            " implements {}",
            class
                .implements
                .iter()
                .map(emit_type)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    format!(
        "class {}{} {{\n{}\n}}",
        class.name,
        relationships,
        emit_members(&class.members)
    )
}

fn emit_members(members: &[ClassMember]) -> String {
    indent(
        &members
            .iter()
            .filter_map(|member| match member {
                ClassMember::Field(value) => {
                    let declaration_type = if value.is_final {
                        "final".into()
                    } else {
                        emit_type(&value.type_ref)
                    };
                    Some(format!(
                        "{}{} {}{};",
                        if value.is_static { "static " } else { "" },
                        declaration_type,
                        value.name,
                        value
                            .initializer
                            .as_ref()
                            .and_then(body_expression)
                            .map(|value| format!(" = {}", emit_expression(value)))
                            .unwrap_or_default()
                    ))
                }
                ClassMember::Method(value)
                | ClassMember::Getter(value)
                | ClassMember::Setter(value)
                | ClassMember::Operator(value) => Some(emit_function(value, 1)),
                ClassMember::Constructor(value) => {
                    let parameters = emit_parameters(&value.parameters);
                    let body = emit_body_or_empty(value.body.as_ref());
                    Some(format!(
                        "{}{}{}({}){}",
                        if value.is_const { "const " } else { "" },
                        value.class_name,
                        value
                            .named
                            .as_ref()
                            .map(|name| format!(".{}", name))
                            .unwrap_or_default(),
                        parameters,
                        if body.is_empty() {
                            ";".into()
                        } else {
                            format!(" {{\n{}\n}}", indent(&body, 1))
                        }
                    ))
                }
                ClassMember::Unlowered { syntax_kind, .. } => {
                    Some(format!("/* unsupported class member: {} */", syntax_kind))
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        1,
    )
}

fn emit_function(function: &crate::typed_ir::FunctionDeclaration, level: usize) -> String {
    let parameters = emit_parameters(&function.parameters);
    let return_type = if function.is_async {
        if matches!(
            function.return_type.name.as_str(),
            "None" | "void" | "dynamic"
        ) {
            "Future<void>".into()
        } else if function.return_type.name == "Future" {
            emit_type(&function.return_type)
        } else {
            format!("Future<{}>", emit_type(&function.return_type))
        }
    } else {
        emit_type(&function.return_type)
    };
    let body = emit_body_or_empty(function.body.as_ref());
    let header = format!(
        "{}{} {}({}){}",
        if function.is_static && level > 0 {
            "static "
        } else {
            ""
        },
        return_type,
        function.name,
        parameters,
        if function.is_async { " async" } else { "" }
    );
    format!("{} {{\n{}\n}}", header, indent(&body, 1))
}

fn emit_parameter(parameter: &Parameter) -> String {
    let value = format!("{} {}", emit_type(&parameter.type_ref), parameter.name);
    let value = if parameter.is_required && parameter.kind == ParameterKind::Named {
        format!("required {}", value)
    } else {
        value
    };
    value
}

fn emit_parameters(parameters: &[Parameter]) -> String {
    let positional = parameters
        .iter()
        .filter(|value| value.kind == ParameterKind::Positional)
        .map(emit_parameter)
        .collect::<Vec<_>>();
    let optional = parameters
        .iter()
        .filter(|value| value.kind == ParameterKind::OptionalPositional)
        .map(emit_parameter)
        .collect::<Vec<_>>();
    let named = parameters
        .iter()
        .filter(|value| value.kind == ParameterKind::Named)
        .map(emit_parameter)
        .collect::<Vec<_>>();
    let mut groups = positional;
    if !optional.is_empty() {
        groups.push(format!("[{}]", optional.join(", ")));
    }
    if !named.is_empty() {
        groups.push(format!("{{{}}}", named.join(", ")));
    }
    groups.join(", ")
}

fn emit_type(reference: &TypeReference) -> String {
    let name = match reference.name.as_str() {
        "Any" | "Object" | "dynamic" => "dynamic",
        "None" | "void" | "Void" => "void",
        "str" | "String" => "String",
        "float" | "double" | "num" => "double",
        "list" | "List" => "List",
        "dict" | "Map" => "Map",
        "set" | "Set" => "Set",
        "deque" | "Queue" => "Queue",
        other => other,
    };
    let mut value = if reference.arguments.is_empty() {
        name.into()
    } else {
        format!(
            "{}<{}>",
            name,
            reference
                .arguments
                .iter()
                .map(emit_type)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    if reference.nullable && value != "dynamic" && value != "void" {
        value.push('?');
    }
    value
}

fn body_expression(body: &Body) -> Option<&Expression> {
    match &body.kind {
        BodyKind::Expression(value) => Some(value),
        _ => None,
    }
}

fn emit_body_or_empty(body: Option<&Body>) -> String {
    body.map(emit_body).unwrap_or_default()
}

fn emit_body(body: &Body) -> String {
    match &body.kind {
        BodyKind::Empty => String::new(),
        BodyKind::Unlowered => format!("/* unsupported body: {} */", body.syntax_kind),
        BodyKind::Expression(value) => format!("return {};", emit_expression(value)),
        BodyKind::Block(values) => values
            .iter()
            .map(emit_statement)
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn emit_statement(statement: &Statement) -> String {
    match &statement.kind {
        StatementKind::Block(values) => format!(
            "{{\n{}\n}}",
            indent(
                &values
                    .iter()
                    .map(emit_statement)
                    .filter(|value| !value.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
                1
            )
        ),
        StatementKind::Variable {
            name,
            type_ref,
            is_final,
            initializer,
        } => format!(
            "{} {}{};",
            if *is_final || type_ref.name == "Final" {
                "final".into()
            } else {
                emit_type(type_ref)
            },
            name,
            initializer
                .as_ref()
                .map(|value| format!(" = {}", emit_expression(value)))
                .unwrap_or_default()
        ),
        StatementKind::Expression(value) if matches!(&value.kind, ExpressionKind::Raw { .. }) => {
            "/* unsupported expression statement */".into()
        }
        StatementKind::Expression(value) => format!("{};", emit_expression(value)),
        StatementKind::Return(value) => format!(
            "return{};",
            value
                .as_ref()
                .map(|value| format!(" {}", emit_expression(value)))
                .unwrap_or_default()
        ),
        StatementKind::If {
            condition,
            then_branch,
            else_branch,
        } => format!(
            "if ({}) {}{}",
            emit_expression(condition),
            emit_statement_block(then_branch),
            else_branch
                .as_ref()
                .map(|value| format!(" else {}", emit_statement_block(value)))
                .unwrap_or_default()
        ),
        StatementKind::ForEach {
            variable,
            iterable,
            body,
        } => format!(
            "for (final {} in {}) {}",
            variable,
            emit_expression(iterable),
            emit_statement_block(body)
        ),
        StatementKind::For {
            initializers,
            condition,
            updates,
            body,
        } => format!(
            "for ({}; {}; {}) {}",
            initializers
                .iter()
                .map(emit_for_initializer)
                .collect::<Vec<_>>()
                .join(", "),
            condition.as_ref().map(emit_expression).unwrap_or_default(),
            updates
                .iter()
                .map(emit_expression)
                .collect::<Vec<_>>()
                .join(", "),
            emit_statement_block(body)
        ),
        StatementKind::While { condition, body } => format!(
            "while ({}) {}",
            emit_expression(condition),
            emit_statement_block(body)
        ),
        StatementKind::DoWhile { body, condition } => format!(
            "do {} while ({});",
            emit_statement_block(body),
            emit_expression(condition)
        ),
        StatementKind::Switch { expression, cases } => {
            let cases = cases
                .iter()
                .map(|case| {
                    let label = match &case.pattern.kind {
                        PatternKind::Default => "default".into(),
                        PatternKind::Constant(value) => {
                            format!("case {}", emit_expression(value))
                        }
                        _ => format!("case {}", case.pattern.source),
                    };
                    format!(
                        "{}:\n{}",
                        label,
                        indent(
                            &case
                                .statements
                                .iter()
                                .map(emit_statement)
                                .collect::<Vec<_>>()
                                .join("\n"),
                            1
                        )
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "switch ({}) {{\n{}\n}}",
                emit_expression(expression),
                indent(&cases, 1)
            )
        }
        StatementKind::Try { body, .. } => format!("try {}", emit_statement_block(body)),
        StatementKind::Throw(value) => format!("throw {};", emit_expression(value)),
        StatementKind::Assert(value) => format!("assert({});", emit_expression(value)),
        StatementKind::Break => "break;".into(),
        StatementKind::Continue => "continue;".into(),
        StatementKind::Unlowered { syntax_kind } => {
            format!("/* unsupported statement: {} */", syntax_kind)
        }
    }
}

fn emit_for_initializer(statement: &Statement) -> String {
    emit_statement(statement).trim_end_matches(';').to_string()
}

fn emit_statement_block(statement: &Statement) -> String {
    match &statement.kind {
        StatementKind::Block(_) => emit_statement(statement),
        _ => format!("{{\n{}\n}}", indent(&emit_statement(statement), 1)),
    }
}

fn emit_expression(expression: &Expression) -> String {
    match &expression.kind {
        ExpressionKind::Identifier(value) => value.clone(),
        ExpressionKind::Literal(Literal::Null) => "null".into(),
        ExpressionKind::Literal(Literal::Bool(value)) => value.to_string(),
        ExpressionKind::Literal(
            Literal::Integer(value)
            | Literal::Float(value)
            | Literal::String(value)
            | Literal::Symbol(value),
        ) => value.clone(),
        ExpressionKind::StringInterpolation(parts) => format!(
            "\"{}\"",
            parts
                .iter()
                .map(|part| match part {
                    StringPart::Text(value) => value.clone(),
                    StringPart::Expression(value) => format!("${{{}}}", emit_expression(value)),
                })
                .collect::<String>()
        ),
        ExpressionKind::Binary {
            operator,
            left,
            right,
        } => format!(
            "{} {} {}",
            emit_expression(left),
            match operator.as_str() {
                "and" => "&&",
                "or" => "||",
                other => other,
            },
            emit_expression(right)
        ),
        ExpressionKind::Unary { operator, operand } => format!(
            "{}{}",
            if operator.trim() == "not" {
                "!"
            } else {
                operator
            },
            emit_expression(operand)
        ),
        ExpressionKind::Assignment {
            target,
            operator,
            value,
        } => format!(
            "{} {} {}",
            emit_expression(target),
            operator,
            emit_expression(value)
        ),
        ExpressionKind::Member {
            object,
            property,
            null_aware,
        } => format!(
            "{}{}{}",
            emit_expression(object),
            if *null_aware { "?." } else { "." },
            property
        ),
        ExpressionKind::Index {
            object,
            index,
            null_aware,
        } => format!(
            "{}{}[{}]",
            emit_expression(object),
            if *null_aware { "?" } else { "" },
            emit_expression(index)
        ),
        ExpressionKind::Call {
            callee,
            arguments,
            type_arguments,
        } => {
            let (callee, inferred_types) = call_target(callee);
            let types = if type_arguments.is_empty() {
                inferred_types
            } else {
                type_arguments.iter().map(emit_type).collect()
            };
            emit_invocation(
                &format!(
                    "{}{}",
                    callee,
                    if types.is_empty() {
                        String::new()
                    } else {
                        format!("<{}>", types.join(", "))
                    }
                ),
                arguments,
            )
        }
        ExpressionKind::IntrinsicCall {
            operation,
            receiver,
            arguments,
        } => {
            let receiver = emit_expression(receiver);
            let arguments = arguments
                .iter()
                .map(emit_expression)
                .collect::<Vec<_>>()
                .join(", ");
            if *operation == IntrinsicOperation::CollectionContains {
                format!("_tcContains({}, {})", receiver, arguments)
            } else {
                format!(
                    "{}.{}({})",
                    receiver,
                    match operation {
                        IntrinsicOperation::CollectionContains => unreachable!(),
                        IntrinsicOperation::CollectionIndexOf => "indexOf",
                        IntrinsicOperation::CollectionSlice => "sublist",
                        IntrinsicOperation::CollectionClear => "clear",
                        IntrinsicOperation::CollectionAdd => "add",
                        IntrinsicOperation::CollectionAddAll => "addAll",
                        IntrinsicOperation::CollectionRemove => "remove",
                        IntrinsicOperation::CollectionRemoveAt => "removeAt",
                        IntrinsicOperation::QueueAddFirst => "addFirst",
                        IntrinsicOperation::QueueAddLast => "addLast",
                        IntrinsicOperation::QueueRemoveFirst => "removeFirst",
                        IntrinsicOperation::QueueRemoveLast => "removeLast",
                        IntrinsicOperation::MapContainsKey => "containsKey",
                        IntrinsicOperation::MapContainsValue => "containsValue",
                    },
                    arguments
                )
            }
        }
        ExpressionKind::ObjectCreation {
            type_ref,
            constructor,
            arguments,
            is_const,
        } => emit_invocation(
            &format!(
                "{}{}{}",
                if *is_const { "const " } else { "" },
                emit_type(type_ref),
                constructor
                    .as_ref()
                    .map(|value| format!(".{}", value))
                    .unwrap_or_default()
            ),
            arguments,
        ),
        ExpressionKind::ListLiteral { elements, .. } => {
            let values = elements
                .iter()
                .map(|value| match value {
                    CollectionElement::Expression(value) => emit_expression(value),
                    CollectionElement::Spread {
                        expression,
                        null_aware,
                    } => format!(
                        "...{}{}",
                        if *null_aware { "?" } else { "" },
                        emit_expression(expression)
                    ),
                })
                .collect::<Vec<_>>();
            emit_collection("[", "]", &values)
        }
        ExpressionKind::MapLiteral { entries, .. } => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|(key, value)| {
                    format!("{}: {}", emit_expression(key), emit_expression(value))
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExpressionKind::Closure { parameters, body } => emit_closure(parameters, body),
        ExpressionKind::IfNull { left, right } => {
            format!("{} ?? {}", emit_expression(left), emit_expression(right))
        }
        ExpressionKind::Await(value) => format!("await {}", emit_expression(value)),
        ExpressionKind::Cast {
            expression,
            type_ref,
        } => format!("{} as {}", emit_expression(expression), emit_type(type_ref)),
        ExpressionKind::TypeTest {
            expression,
            type_ref,
            negated,
        } => format!(
            "{} is{} {}",
            emit_expression(expression),
            if *negated { "!" } else { "" },
            emit_type(type_ref)
        ),
        ExpressionKind::Cascade { target, .. } => emit_expression(target),
        ExpressionKind::Switch { .. } => expression.source.clone(),
        ExpressionKind::Raw { .. } => "null /* unsupported expression */".into(),
    }
}

fn call_target(callee: &Expression) -> (String, Vec<String>) {
    if let ExpressionKind::Index { object, index, .. } = &callee.kind {
        return (
            emit_expression(object),
            vec![emit_expression(index).trim_matches(['(', ')']).into()],
        );
    }
    (emit_expression(callee), Vec::new())
}

fn emit_invocation(callee: &str, arguments: &[Argument]) -> String {
    let values = arguments
        .iter()
        .map(|argument| {
            let value = emit_expression(&argument.value);
            argument
                .name
                .as_ref()
                .map(|name| format!("{}: {}", name, value))
                .unwrap_or(value)
        })
        .collect::<Vec<_>>();
    let compact = values.join(", ");
    if values.is_empty() || (callee.len() + compact.len() <= 60 && !compact.contains('\n')) {
        return format!("{}({})", callee, compact);
    }
    format!(
        "{}(\n{}\n)",
        callee,
        indent(
            &values
                .iter()
                .map(|value| format!("{},", value))
                .collect::<Vec<_>>()
                .join("\n"),
            1
        )
    )
}

fn emit_collection(open: &str, close: &str, values: &[String]) -> String {
    let compact = values.join(", ");
    if compact.len() <= 60 && !compact.contains('\n') {
        return format!("{}{}{}", open, compact, close);
    }
    format!(
        "{}\n{}\n{}",
        open,
        indent(
            &values
                .iter()
                .map(|value| format!("{},", value))
                .collect::<Vec<_>>()
                .join("\n"),
            1
        ),
        close
    )
}

fn emit_closure(parameters: &[Parameter], body: &Body) -> String {
    let captured = parameters
        .iter()
        .filter_map(|parameter| {
            parameter
                .default_value
                .as_ref()
                .map(|value| (parameter.name.as_str(), value))
        })
        .collect::<Vec<_>>();
    if !captured.is_empty() {
        let declarations = captured
            .iter()
            .map(|(name, value)| format!("final {} = {};", name, emit_expression(value)))
            .collect::<Vec<_>>()
            .join("\n");
        let returned = match &body.kind {
            BodyKind::Expression(value) => format!("return {};", emit_expression(value)),
            _ => String::new(),
        };
        return format!(
            "() {{\n{}\n}}",
            indent(&format!("{}\n{}", declarations, returned), 1)
        );
    }
    format!(
        "({}) => {}",
        parameters
            .iter()
            .map(|value| value.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        match &body.kind {
            BodyKind::Expression(value) => emit_expression(value),
            _ => "null".into(),
        }
    )
}

fn indent(value: &str, level: usize) -> String {
    let prefix = "  ".repeat(level);
    value
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{}{}", prefix, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
