use crate::backend::{Backend, BackendOutput};
use crate::typed_ir::{
    Body, BodyKind, ClassDeclaration, ClassMember, CompilationUnit, Declaration, Expression,
    ExpressionKind, Literal, Parameter, Statement, StatementKind, TypeReference,
};

/// Typed Dart-to-Python emitter.  This intentionally consumes the Tree-sitter IR rather than
/// reparsing source lines: Dart's generic types and nested blocks must not be inferred from text.
pub struct PythonBackend;

impl Backend for PythonBackend {
    fn emit(&self, unit: &CompilationUnit) -> BackendOutput {
        let mut sections = Vec::new();
        let mut entrypoint = None;
        for declaration in &unit.declarations {
            let rendered = match declaration {
                Declaration::Class(value) | Declaration::Mixin(value) => emit_class(value),
                Declaration::Function(value) if value.name == "main" => {
                    entrypoint = Some(emit_function(value, false));
                    continue;
                }
                Declaration::Function(value) => emit_function(value, false),
                Declaration::Enum(value) => format!("class {}:\n    pass", value.name),
                Declaration::Extension(_) | Declaration::TypeAlias(_) => continue,
            };
            if !rendered.is_empty() { sections.push(rendered); }
        }
        if let Some(entrypoint) = entrypoint {
            sections.push(entrypoint);
            sections.push("if __name__ == \"__main__\":\n    main()".into());
        }
        BackendOutput { code: sections.join("\n\n"), diagnostics: Vec::new() }
    }
}

fn emit_class(class: &ClassDeclaration) -> String {
    let mut members = Vec::new();
    for member in &class.members {
        match member {
            ClassMember::Method(value) | ClassMember::Getter(value) | ClassMember::Setter(value) | ClassMember::Operator(value) => members.push(emit_function(value, !value.is_static)),
            ClassMember::Constructor(value) => {
                let params = value.parameters.iter().map(emit_parameter).collect::<Vec<_>>();
                let body = value.body.as_ref().map(emit_body).unwrap_or_default();
                let body = if body.is_empty() { "pass".into() } else { body };
                members.push(format!("    def __init__(self{}{}):\n{}", if params.is_empty() { "" } else { ", " }, params.join(", "), indent(&body, 2)));
            }
            ClassMember::Field(_) | ClassMember::Unlowered { .. } => {}
        }
    }
    if members.is_empty() { members.push("    pass".into()); }
    format!("class {}:\n{}", class.name, indent(&members.join("\n\n"), 1))
}

fn emit_function(function: &crate::typed_ir::FunctionDeclaration, method: bool) -> String {
    let mut params = Vec::new();
    if method { params.push("self".into()); }
    params.extend(function.parameters.iter().map(emit_parameter));
    let body = function.body.as_ref().map(emit_body).unwrap_or_default();
    let body = if body.is_empty() { "pass".into() } else { body };
    format!("def {}({}) -> {}:\n{}", function.name, params.join(", "), emit_type(&function.return_type), indent(&body, 1))
}

fn emit_parameter(parameter: &Parameter) -> String {
    let default = parameter.default_value.as_ref().map(|value| format!(" = {}", emit_expression(value))).unwrap_or_default();
    format!("{}: {}{}", parameter.name, emit_type(&parameter.type_ref), default)
}

fn emit_type(reference: &TypeReference) -> String {
    let name = match reference.name.as_str() {
        "dynamic" | "Object" => "Any",
        "void" => "None",
        "String" => "str",
        "bool" => "bool",
        "int" => "int",
        "double" | "num" => "float",
        "List" | "Iterable" => "list",
        "Set" => "set",
        "Map" => "dict",
        other => other,
    };
    if reference.arguments.is_empty() { return name.into(); }
    format!("{}[{}]", name, reference.arguments.iter().map(emit_type).collect::<Vec<_>>().join(", "))
}

fn emit_body(body: &Body) -> String {
    match &body.kind {
        BodyKind::Empty | BodyKind::Unlowered => String::new(),
        BodyKind::Expression(value) => format!("return {}", emit_expression(value)),
        BodyKind::Block(values) => values.iter().map(emit_statement).collect::<Vec<_>>().join("\n"),
    }
}

fn emit_statement(statement: &Statement) -> String {
    match &statement.kind {
        StatementKind::Block(values) => values.iter().map(emit_statement).collect::<Vec<_>>().join("\n"),
        StatementKind::Variable { name, type_ref, initializer, .. } => format!("{}: {}{}", name, emit_type(type_ref), initializer.as_ref().map(|value| format!(" = {}", emit_expression(value))).unwrap_or_default()),
        StatementKind::Expression(value) => emit_expression(value),
        StatementKind::Return(value) => format!("return{}", value.as_ref().map(|value| format!(" {}", emit_expression(value))).unwrap_or_default()),
        StatementKind::If { condition, then_branch, else_branch } => {
            let then_text = indent(&emit_statement(then_branch), 1);
            let suffix = else_branch.as_ref().map(|value| format!("\nelse:\n{}", indent(&emit_statement(value), 1))).unwrap_or_default();
            format!("if {}:\n{}{}", emit_expression(condition), then_text, suffix)
        }
        StatementKind::ForEach { variable, iterable, body } => format!("for {} in {}:\n{}", variable, emit_expression(iterable), indent(&emit_statement(body), 1)),
        StatementKind::While { condition, body } => format!("while {}:\n{}", emit_expression(condition), indent(&emit_statement(body), 1)),
        StatementKind::Break => "break".into(),
        StatementKind::Continue => "continue".into(),
        StatementKind::Assert(value) => format!("assert {}", emit_expression(value)),
        StatementKind::Throw(value) => format!("raise {}", emit_expression(value)),
        StatementKind::Unlowered { .. } | StatementKind::DoWhile { .. } | StatementKind::Switch { .. } | StatementKind::Try { .. } => "pass".into(),
    }
}

fn emit_expression(expression: &Expression) -> String {
    match &expression.kind {
        ExpressionKind::Identifier(value) => value.clone(),
        ExpressionKind::Literal(Literal::Null) => "None".into(),
        ExpressionKind::Literal(Literal::Bool(value)) => value.to_string().to_uppercase(),
        ExpressionKind::Literal(Literal::Integer(value) | Literal::Float(value) | Literal::String(value) | Literal::Symbol(value)) => value.clone(),
        ExpressionKind::StringInterpolation(parts) => format!("f\"{}\"", parts.iter().map(|part| match part { crate::typed_ir::StringPart::Text(value) => value.clone(), crate::typed_ir::StringPart::Expression(value) => format!("{{{}}}", emit_expression(value)) }).collect::<String>()),
        ExpressionKind::Binary { operator, left, right } => format!("{} {} {}", emit_expression(left), match operator.as_str() { "&&" => "and", "||" => "or", "??" => "or", other => other }, emit_expression(right)),
        ExpressionKind::Unary { operator, operand } => format!("{}{}", if operator == "!" { "not " } else { operator }, emit_expression(operand)),
        ExpressionKind::Assignment { target, operator, value } => format!("{} {} {}", emit_expression(target), operator, emit_expression(value)),
        ExpressionKind::Member { object, property, .. } => {
            let object = emit_expression(object);
            match property.as_str() {
                "isEmpty" => format!("len({}) == 0", object),
                "isNotEmpty" => format!("len({}) != 0", object),
                "length" => format!("len({})", object),
                "hashCode" => format!("hash({})", object),
                _ => format!("{}.{}", object, property),
            }
        }
        ExpressionKind::Index { object, index, .. } => format!("{}[{}]", emit_expression(object), emit_expression(index)),
        ExpressionKind::Call { .. } => emit_source_expression(&expression.source),
        ExpressionKind::ObjectCreation { .. } => emit_source_expression(&expression.source),
        ExpressionKind::ListLiteral { elements, .. } => format!("[{}]", elements.iter().map(|value| match value { crate::typed_ir::CollectionElement::Expression(value) => emit_expression(value), crate::typed_ir::CollectionElement::Spread { expression, .. } => format!("*{}", emit_expression(expression)) }).collect::<Vec<_>>().join(", ")),
        ExpressionKind::MapLiteral { entries, .. } => format!("{{{}}}", entries.iter().map(|(key, value)| format!("{}: {}", emit_expression(key), emit_expression(value))).collect::<Vec<_>>().join(", ")),
        ExpressionKind::IfNull { left, right } => format!("({} if {} is not None else {})", emit_expression(left), emit_expression(left), emit_expression(right)),
        ExpressionKind::Await(value) => emit_expression(value),
        ExpressionKind::Cast { expression, .. } | ExpressionKind::TypeTest { expression, .. } => emit_expression(expression),
        ExpressionKind::Raw { .. } | ExpressionKind::Closure { .. } | ExpressionKind::Cascade { .. } | ExpressionKind::Switch { .. } => expression.source.clone(),
    }
}

fn emit_source_expression(source: &str) -> String {
    source.trim().trim_end_matches(';').replace("true", "True").replace("false", "False").replace("null", "None")
}

fn indent(value: &str, level: usize) -> String {
    let prefix = "    ".repeat(level);
    value.lines().map(|line| format!("{}{}", prefix, line)).collect::<Vec<_>>().join("\n")
}
