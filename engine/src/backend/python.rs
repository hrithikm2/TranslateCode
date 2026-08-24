use crate::backend::{
    emit_comments, is_python_main_guard, unsupported_diagnostics, Backend, BackendOutput,
};
use crate::typed_ir::{
    Argument, Body, BodyKind, ClassDeclaration, ClassMember, CollectionElement, CompilationUnit,
    Declaration, Expression, ExpressionKind, FieldDeclaration, FunctionDeclaration,
    IntrinsicOperation, Literal, Parameter, ParameterKind, Statement, StatementKind, StringPart,
    TypeReference,
};

/// Emits valid Python from Universal IR. Source-language spellings are never copied for nodes
/// that have structured IR because Dart named arguments, generics, closures, and `const` are not
/// valid Python syntax.
pub struct PythonBackend;

impl Backend for PythonBackend {
    fn emit(&self, unit: &CompilationUnit) -> BackendOutput {
        let mut sections = Vec::new();
        if !unit.comments.is_empty() {
            sections.push(emit_comments(&unit.comments, crate::Language::Python));
        }
        sections.extend([
            "from __future__ import annotations".into(),
            "from typing import Any, Final".into(),
        ]);
        sections.extend(unit.imports.iter().map(emit_import));

        let mut entrypoint = None;
        for declaration in &unit.declarations {
            let rendered = match declaration {
                Declaration::Class(value) | Declaration::Mixin(value) => emit_class(value),
                Declaration::Function(value) if value.name == "main" => {
                    entrypoint = Some(emit_function(value, false));
                    continue;
                }
                Declaration::Function(value) => emit_function(value, false),
                Declaration::Enum(value) => {
                    let values = value
                        .values
                        .iter()
                        .enumerate()
                        .map(|(index, name)| format!("    {} = {}", name, index))
                        .collect::<Vec<_>>();
                    format!(
                        "class {}:\n{}",
                        value.name,
                        if values.is_empty() {
                            "    pass".into()
                        } else {
                            values.join("\n")
                        }
                    )
                }
                Declaration::Extension(_) | Declaration::TypeAlias(_) => continue,
            };
            if !rendered.is_empty() {
                sections.push(rendered);
            }
        }
        if let Some(entrypoint) = entrypoint {
            sections.push(entrypoint);
        }
        if !unit.top_level_statements.is_empty() {
            sections.push(
                unit.top_level_statements
                    .iter()
                    .filter(|value| !is_python_main_guard(value))
                    .map(emit_statement)
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        if unit
            .declarations
            .iter()
            .any(|value| matches!(value, Declaration::Function(value) if value.name == "main"))
        {
            sections.push("if __name__ == \"__main__\":\n    main()".into());
        }
        BackendOutput {
            code: sections.join("\n\n"),
            diagnostics: unsupported_diagnostics(unit),
        }
    }
}

fn emit_import(value: &crate::typed_ir::ImportDeclaration) -> String {
    let module = python_module_name(&value.uri);
    if value.show.is_empty() {
        format!(
            "import {}{}",
            module,
            value
                .prefix
                .as_ref()
                .map(|prefix| format!(" as {}", prefix))
                .unwrap_or_default()
        )
    } else {
        format!("from {} import {}", module, value.show.join(", "))
    }
}

fn python_module_name(uri: &str) -> String {
    let value = uri
        .trim()
        .trim_end_matches(".dart")
        .replace(':', ".")
        .replace(['/', '-'], ".");
    value
        .split('.')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut result = part
                .chars()
                .map(|character| {
                    if character.is_alphanumeric() || character == '_' {
                        character
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            if result
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_digit())
            {
                result.insert(0, '_');
            }
            result
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn emit_class(class: &ClassDeclaration) -> String {
    let bases = class
        .extends
        .iter()
        .chain(&class.mixins)
        .chain(&class.implements)
        .map(emit_type)
        .filter(|value| value != "Any")
        .collect::<Vec<_>>();
    let mut members = class
        .members
        .iter()
        .filter_map(|member| match member {
            ClassMember::Field(value) => Some(emit_field(value)),
            ClassMember::Method(value)
            | ClassMember::Getter(value)
            | ClassMember::Setter(value)
            | ClassMember::Operator(value) => Some(emit_function(value, !value.is_static)),
            ClassMember::Constructor(value) => {
                let mut parameters = vec!["self".into()];
                parameters.extend(value.parameters.iter().map(emit_parameter));
                Some(format!(
                    "def __init__({}):\n{}",
                    parameters.join(", "),
                    indent(&emit_body_or_pass(value.body.as_ref()), 1)
                ))
            }
            ClassMember::Unlowered { syntax_kind, .. } => {
                Some(format!("# unsupported class member: {}", syntax_kind))
            }
        })
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if members.is_empty() {
        members.push("pass".into());
    }
    format!(
        "class {}{}:\n{}",
        class.name,
        if bases.is_empty() {
            String::new()
        } else {
            format!("({})", bases.join(", "))
        },
        indent(&members.join("\n\n"), 1)
    )
}

fn emit_field(field: &FieldDeclaration) -> String {
    let value = field
        .initializer
        .as_ref()
        .and_then(|body| match &body.kind {
            BodyKind::Expression(value) => Some(emit_expression(value)),
            _ => None,
        })
        .unwrap_or_else(|| "None".into());
    format!("{}: {} = {}", field.name, emit_type(&field.type_ref), value)
}

fn emit_function(function: &FunctionDeclaration, method: bool) -> String {
    let mut parameters = Vec::new();
    if method {
        parameters.push("self".into());
    }
    parameters.extend(function.parameters.iter().map(emit_parameter));
    let return_type = if function.is_async
        && function.return_type.name == "Future"
        && function
            .return_type
            .arguments
            .first()
            .is_some_and(|value| value.name == "void")
    {
        "None".into()
    } else {
        emit_type(&function.return_type)
    };
    format!(
        "{}def {}({}) -> {}:\n{}",
        if function.is_async { "async " } else { "" },
        function.name,
        parameters.join(", "),
        return_type,
        indent(&emit_body_or_pass(function.body.as_ref()), 1)
    )
}

fn emit_parameter(parameter: &Parameter) -> String {
    let default = parameter
        .default_value
        .as_ref()
        .map(emit_expression)
        .or_else(|| {
            matches!(
                parameter.kind,
                ParameterKind::Named | ParameterKind::OptionalPositional
            )
            .then(|| "None".into())
        });
    format!(
        "{}: {}{}",
        parameter.name,
        emit_type(&parameter.type_ref),
        default
            .map(|value| format!(" = {}", value))
            .unwrap_or_default()
    )
}

fn emit_type(reference: &TypeReference) -> String {
    let name = match reference.name.as_str() {
        "dynamic" | "Object" => "Any",
        "void" | "Void" => "None",
        "String" => "str",
        "bool" => "bool",
        "int" => "int",
        "double" | "num" => "float",
        "List" | "Iterable" => "list",
        "Set" => "set",
        "Map" => "dict",
        other => other,
    };
    let rendered = if reference.arguments.is_empty() {
        name.into()
    } else {
        format!(
            "{}[{}]",
            name,
            reference
                .arguments
                .iter()
                .map(emit_type)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    if reference.nullable && rendered != "Any" && rendered != "None" {
        format!("{} | None", rendered)
    } else {
        rendered
    }
}

fn emit_body_or_pass(body: Option<&Body>) -> String {
    let emitted = body.map(emit_body).unwrap_or_default();
    if emitted.trim().is_empty() {
        "pass".into()
    } else {
        emitted
    }
}

fn emit_body(body: &Body) -> String {
    match &body.kind {
        BodyKind::Empty => String::new(),
        BodyKind::Unlowered => format!("# unsupported body: {}", body.syntax_kind),
        BodyKind::Expression(value) => format!("return {}", emit_expression(value)),
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
        StatementKind::Block(values) => values
            .iter()
            .map(emit_statement)
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        StatementKind::Variable {
            name,
            type_ref,
            is_final,
            initializer,
        } => format!(
            "{}: {} = {}",
            name,
            if *is_final {
                format!("Final[{}]", emit_type(type_ref))
            } else {
                emit_type(type_ref)
            },
            initializer
                .as_ref()
                .map(emit_expression)
                .unwrap_or_else(|| "None".into())
        ),
        StatementKind::Expression(value) => emit_expression(value),
        StatementKind::Return(value) => format!(
            "return{}",
            value
                .as_ref()
                .map(|value| format!(" {}", emit_expression(value)))
                .unwrap_or_default()
        ),
        StatementKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            let then_text = nonempty_statement(then_branch);
            let suffix = else_branch
                .as_ref()
                .map(|value| format!("\nelse:\n{}", indent(&nonempty_statement(value), 1)))
                .unwrap_or_default();
            format!(
                "if {}:\n{}{}",
                emit_expression(condition),
                indent(&then_text, 1),
                suffix
            )
        }
        StatementKind::ForEach {
            variable,
            iterable,
            body,
        } => format!(
            "for {} in {}:\n{}",
            variable,
            emit_expression(iterable),
            indent(&nonempty_statement(body), 1)
        ),
        StatementKind::For {
            initializers,
            condition,
            updates,
            body,
        } => {
            let mut lines = initializers.iter().map(emit_statement).collect::<Vec<_>>();
            let mut loop_body = nonempty_statement(body);
            if !updates.is_empty() {
                loop_body.push('\n');
                loop_body.push_str(
                    &updates
                        .iter()
                        .map(emit_expression)
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
            lines.push(format!(
                "while {}:\n{}",
                condition
                    .as_ref()
                    .map(emit_expression)
                    .unwrap_or_else(|| "True".into()),
                indent(&loop_body, 1)
            ));
            lines.join("\n")
        }
        StatementKind::While { condition, body } => format!(
            "while {}:\n{}",
            emit_expression(condition),
            indent(&nonempty_statement(body), 1)
        ),
        StatementKind::Break => "break".into(),
        StatementKind::Continue => "continue".into(),
        StatementKind::Assert(value) => format!("assert {}", emit_expression(value)),
        StatementKind::Throw(value) => format!("raise {}", emit_expression(value)),
        StatementKind::DoWhile { .. }
        | StatementKind::Switch { .. }
        | StatementKind::Try { .. } => "# unsupported structured statement".into(),
        StatementKind::Unlowered { syntax_kind } => {
            format!("# unsupported statement: {}", syntax_kind)
        }
    }
}

fn nonempty_statement(statement: &Statement) -> String {
    let value = emit_statement(statement);
    if value.trim().is_empty() {
        "pass".into()
    } else {
        value
    }
}

fn emit_expression(expression: &Expression) -> String {
    match &expression.kind {
        ExpressionKind::Identifier(value) => value.clone(),
        ExpressionKind::Literal(Literal::Null) => "None".into(),
        ExpressionKind::Literal(Literal::Bool(value)) => {
            if *value { "True" } else { "False" }.into()
        }
        ExpressionKind::Literal(
            Literal::Integer(value)
            | Literal::Float(value)
            | Literal::String(value)
            | Literal::Symbol(value),
        ) => value.clone(),
        ExpressionKind::StringInterpolation(parts) => format!(
            "f\"{}\"",
            parts
                .iter()
                .map(|part| match part {
                    StringPart::Text(value) => value.replace('"', "\\\""),
                    StringPart::Expression(value) => format!("{{{}}}", emit_expression(value)),
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
                "&&" => "and",
                "||" => "or",
                "??" => "or",
                other => other,
            },
            emit_expression(right)
        ),
        ExpressionKind::Unary { operator, operand } => format!(
            "{}{}",
            if operator == "!" { "not " } else { operator },
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
            object, property, ..
        } => {
            let object = emit_expression(object);
            match property.as_str() {
                "isEmpty" => format!("len({}) == 0", object),
                "isNotEmpty" => format!("len({}) != 0", object),
                "length" => format!("len({})", object),
                "hashCode" => format!("hash({})", object),
                _ => format!("{}.{}", object, property),
            }
        }
        ExpressionKind::Index { object, index, .. } => {
            format!("{}[{}]", emit_expression(object), emit_expression(index))
        }
        ExpressionKind::Call {
            callee,
            arguments,
            type_arguments,
        } => format!(
            "{}{}({})",
            emit_expression(callee),
            if type_arguments.is_empty() {
                String::new()
            } else {
                format!(
                    "[{}]",
                    type_arguments
                        .iter()
                        .map(emit_type_name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            },
            emit_arguments(arguments)
        ),
        ExpressionKind::IntrinsicCall {
            operation,
            receiver,
            arguments,
        } => {
            let receiver = emit_expression(receiver);
            let first = arguments
                .first()
                .map(emit_expression)
                .unwrap_or_else(|| "None".into());
            match operation {
                IntrinsicOperation::CollectionContains => format!("{} in {}", first, receiver),
                IntrinsicOperation::CollectionIndexOf => format!("{}.index({})", receiver, first),
            }
        }
        ExpressionKind::ObjectCreation {
            type_ref,
            constructor,
            arguments,
            ..
        } => format!(
            "{}{}({})",
            emit_type_name(type_ref),
            constructor
                .as_ref()
                .map(|value| format!(".{}", value))
                .unwrap_or_default(),
            emit_arguments(arguments)
        ),
        ExpressionKind::ListLiteral { elements, .. } => format!(
            "[{}]",
            elements
                .iter()
                .map(|value| match value {
                    CollectionElement::Expression(value) => emit_expression(value),
                    CollectionElement::Spread { expression, .. } => {
                        format!("*{}", emit_expression(expression))
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
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
        ExpressionKind::IfNull { left, right } => format!(
            "({} if {} is not None else {})",
            emit_expression(left),
            emit_expression(left),
            emit_expression(right)
        ),
        ExpressionKind::Await(value) => format!("await {}", emit_expression(value)),
        ExpressionKind::Cast { expression, .. } => emit_expression(expression),
        ExpressionKind::TypeTest {
            expression,
            type_ref,
            negated,
        } => format!(
            "{}isinstance({}, {})",
            if *negated { "not " } else { "" },
            emit_expression(expression),
            emit_type_name(type_ref)
        ),
        ExpressionKind::Cascade { target, .. } => emit_expression(target),
        ExpressionKind::Switch { .. } => expression.source.clone(),
        ExpressionKind::Raw { .. } => "None".into(),
    }
}

fn emit_arguments(arguments: &[Argument]) -> String {
    arguments
        .iter()
        .map(|argument| {
            let value = emit_expression(&argument.value);
            argument
                .name
                .as_ref()
                .map(|name| format!("{}={}", name, value))
                .unwrap_or(value)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn emit_type_name(reference: &TypeReference) -> String {
    match reference.name.as_str() {
        "List" => "list".into(),
        "Map" => "dict".into(),
        "Set" => "set".into(),
        other => other.into(),
    }
}

fn emit_closure(parameters: &[Parameter], body: &Body) -> String {
    let mut rendered_parameters = parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let captures = match &body.kind {
        BodyKind::Block(statements) => statements
            .iter()
            .filter_map(|statement| match &statement.kind {
                StatementKind::Variable {
                    name,
                    initializer: Some(value),
                    ..
                } => Some(format!("{}={}", name, emit_expression(value))),
                _ => None,
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    if !captures.is_empty() {
        if !rendered_parameters.is_empty() {
            rendered_parameters.push_str(", ");
        }
        rendered_parameters.push_str(&captures.join(", "));
    }
    let value = match &body.kind {
        BodyKind::Expression(value) => emit_expression(value),
        BodyKind::Block(statements) => {
            let values = statements
                .iter()
                .filter_map(closure_statement_expression)
                .collect::<Vec<_>>();
            match values.as_slice() {
                [] => "None".into(),
                [value] => value.clone(),
                _ => format!("({})[-1]", values.join(", ")),
            }
        }
        BodyKind::Empty | BodyKind::Unlowered => "None".into(),
    };
    format!("lambda {}: {}", rendered_parameters, value)
}

fn closure_statement_expression(statement: &Statement) -> Option<String> {
    match &statement.kind {
        StatementKind::Expression(value) | StatementKind::Return(Some(value)) => {
            Some(emit_expression(value))
        }
        StatementKind::Block(values) => values.iter().rev().find_map(closure_statement_expression),
        _ => None,
    }
}

fn indent(value: &str, level: usize) -> String {
    let prefix = "    ".repeat(level);
    value
        .lines()
        .map(|line| format!("{}{}", prefix, line))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::dart::DartFrontend;
    use crate::frontend::Frontend;

    #[test]
    fn emits_structured_dart_calls_as_valid_python() {
        let source = r#"
Future<void> initialize() async {
  final dio = Dio(BaseOptions(baseUrl: AppConstants.baseUrl));
  await Get.put<NewsRepository>(NewsRepositoryImpl(remote: dio));
}
"#;
        let output = PythonBackend.emit(&DartFrontend.parse(source)).code;
        assert!(
            output.contains("async def initialize() -> None:"),
            "{}",
            output
        );
        assert!(
            output.contains("Dio(BaseOptions(baseUrl=AppConstants.baseUrl))"),
            "{}",
            output
        );
        assert!(
            output.contains("await Get.put[NewsRepository](NewsRepositoryImpl(remote=dio))"),
            "{}",
            output
        );
        assert!(!output.contains("<NewsRepository>"), "{}", output);
    }

    #[test]
    fn emits_indexed_dart_loops_and_collection_intrinsics() {
        let source = r#"class Solution {
  List<int> twoSum(List<int> nums, int target) {
    for (int i = 0; i < nums.length; i++) {
      final offset = target - nums[i];
      if (nums.contains(offset)) {
        return [i, nums.indexOf(offset)];
      }
    }
    return [];
  }
}"#;
        let output = PythonBackend.emit(&DartFrontend.parse(source));
        assert!(output.diagnostics.is_empty(), "{:#?}", output.diagnostics);
        assert!(
            output.code.contains("while i < len(nums):"),
            "{}",
            output.code
        );
        assert!(
            output.code.contains("if offset in nums:"),
            "{}",
            output.code
        );
        assert!(
            output.code.contains("return [i, nums.index(offset)]"),
            "{}",
            output.code
        );
        assert!(output.code.contains("i += 1"), "{}", output.code);
        assert!(!output.code.contains("++i"), "{}", output.code);
        assert!(!output.code.contains(".contains("), "{}", output.code);
        assert!(!output.code.contains(".indexOf("), "{}", output.code);
    }

    #[test]
    fn emits_dart_map_contains_key_as_python_membership() {
        let source = r#"bool hasValue(Map<int, String> valuesMap, int num) {
  return valuesMap.containsKey(num);
}"#;
        let output = PythonBackend.emit(&DartFrontend.parse(source));
        assert!(output.diagnostics.is_empty(), "{:#?}", output.diagnostics);
        assert!(
            output.code.contains("return num in valuesMap"),
            "{}",
            output.code
        );
        assert!(!output.code.contains(".containsKey("), "{}", output.code);
    }
}
