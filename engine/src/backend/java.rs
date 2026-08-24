use std::collections::{HashMap, HashSet};

use crate::backend::{
    emit_comments, is_python_main_guard, unsupported_diagnostics, Backend, BackendOutput,
};
use crate::typed_ir::{
    Argument, Body, BodyKind, ClassDeclaration, ClassKind, ClassMember, CollectionElement,
    CompilationUnit, ConstructorDeclaration, Declaration, Expression, ExpressionKind,
    ExtensionDeclaration, FunctionDeclaration, IntrinsicOperation, Literal, Parameter, PatternKind,
    Statement, StatementKind, SwitchExpressionCase, TypeReference,
};

pub struct JavaBackend;

impl Backend for JavaBackend {
    fn emit(&self, unit: &CompilationUnit) -> BackendOutput {
        let context = JavaContext::new(unit);
        let mut sections = Vec::new();
        for declaration in &unit.declarations {
            sections.push(match declaration {
                Declaration::Class(value) => emit_class(value, false, &context),
                Declaration::Mixin(value) => emit_class(value, true, &context),
                Declaration::Enum(value) => {
                    format!("enum {} {{ {} }}", value.name, value.values.join(", "))
                }
                Declaration::Extension(value) => emit_extension(value, &context),
                Declaration::TypeAlias(value) => {
                    let parameters = type_parameters(
                        &value
                            .type_parameters
                            .iter()
                            .map(|item| item.name.as_str())
                            .collect::<Vec<_>>(),
                    );
                    format!(
                        "@FunctionalInterface\ninterface {}{} {{\n    R apply(T value);\n}}",
                        value.name, parameters
                    )
                }
                Declaration::Function(_) => continue,
            });
        }
        let mut functions = unit
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Function(value) => Some(emit_top_level_function(value, &context)),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !unit.top_level_statements.is_empty() {
            functions.push(format!(
                "static {{\n{}\n}}",
                indent(
                    &unit
                        .top_level_statements
                        .iter()
                        .filter(|value| !is_python_main_guard(value))
                        .map(|value| emit_statement(value, false, &context))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    1
                )
            ));
        }
        sections.push(format!(
            "public final class TranslatedProgram {{\n{}\n}}",
            indent(&functions.join("\n\n"), 1)
        ));
        let program = sections.join("\n\n");
        let needs_runtime = program.contains("DartRuntime.");
        let imports = emit_imports(&program, needs_runtime);
        let mut output = Vec::new();
        if !unit.comments.is_empty() {
            output.push(emit_comments(&unit.comments, crate::Language::Java));
        }
        if !imports.is_empty() {
            output.push(imports);
        }
        if needs_runtime {
            output.push(emit_runtime());
        }
        output.push(program);
        BackendOutput {
            code: output.join("\n\n"),
            diagnostics: unsupported_diagnostics(unit),
        }
    }
}

fn emit_imports(program: &str, needs_runtime: bool) -> String {
    let mut imports = Vec::new();
    let symbols = [
        ("LocalDateTime", "java.time.LocalDateTime"),
        ("ArrayList", "java.util.ArrayList"),
        ("Arrays", "java.util.Arrays"),
        ("HashMap", "java.util.HashMap"),
        ("HashSet", "java.util.HashSet"),
        ("List", "java.util.List"),
        ("Map", "java.util.Map"),
        ("Objects", "java.util.Objects"),
        (
            "CompletableFuture",
            "java.util.concurrent.CompletableFuture",
        ),
        ("Function", "java.util.function.Function"),
    ];
    for (symbol, path) in symbols {
        if contains_java_symbol(program, symbol)
            || (needs_runtime && matches!(symbol, "ArrayList" | "List"))
        {
            imports.push(format!("import {};", path));
        }
    }
    imports.join("\n")
}

fn contains_java_symbol(program: &str, symbol: &str) -> bool {
    program.match_indices(symbol).any(|(index, _)| {
        let before = program[..index].chars().next_back();
        let after = program[index + symbol.len()..].chars().next();
        let is_identifier = |value: char| value == '_' || value.is_ascii_alphanumeric();
        before.is_none_or(|value| !is_identifier(value))
            && after.is_none_or(|value| !is_identifier(value))
    })
}

#[derive(Default)]
struct JavaContext {
    classes: HashSet<String>,
    getters: HashSet<String>,
    setters: HashSet<String>,
    extension_getters: HashMap<String, String>,
    constructors: HashMap<String, Vec<Parameter>>,
    fields: HashMap<String, HashMap<String, TypeReference>>,
    parents: HashMap<String, String>,
}

impl JavaContext {
    fn new(unit: &CompilationUnit) -> Self {
        let mut result = Self::default();
        for declaration in &unit.declarations {
            match declaration {
                Declaration::Class(class) | Declaration::Mixin(class) => {
                    result.classes.insert(class.name.clone());
                    if let Some(parent) = &class.extends {
                        result
                            .parents
                            .insert(class.name.clone(), parent.name.clone());
                    }
                    let mut fields = HashMap::new();
                    for member in &class.members {
                        match member {
                            ClassMember::Field(field) => {
                                fields.insert(field.name.clone(), field.type_ref.clone());
                            }
                            ClassMember::Getter(value) => {
                                result.getters.insert(value.name.clone());
                            }
                            ClassMember::Setter(value) => {
                                result.setters.insert(value.name.clone());
                            }
                            ClassMember::Constructor(value)
                                if value.named.is_none() && !value.is_factory =>
                            {
                                result
                                    .constructors
                                    .insert(class.name.clone(), value.parameters.clone());
                            }
                            _ => {}
                        }
                    }
                    result.fields.insert(class.name.clone(), fields);
                }
                Declaration::Extension(extension) => {
                    for member in &extension.members {
                        if let ClassMember::Getter(value) = member {
                            result.extension_getters.insert(
                                value.name.clone(),
                                format!("{}Extensions", extension.name),
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        result
    }

    fn parameter_type(&self, class: &ClassDeclaration, parameter: &Parameter) -> TypeReference {
        if parameter.type_ref.name != "dynamic" {
            return parameter.type_ref.clone();
        }
        if let Some(value) = self
            .fields
            .get(&class.name)
            .and_then(|fields| fields.get(&parameter.name))
        {
            return value.clone();
        }
        let mut parent = class.extends.as_ref().map(|value| value.name.as_str());
        while let Some(name) = parent {
            if let Some(value) = self
                .fields
                .get(name)
                .and_then(|fields| fields.get(&parameter.name))
            {
                return value.clone();
            }
            parent = self.parents.get(name).map(String::as_str);
        }
        TypeReference::dynamic()
    }
}

fn emit_runtime() -> String {
    r#"final class DartRuntime {
    private DartRuntime() {}
    @SuppressWarnings("unchecked")
    static <T> List<T> listOf(Object... values) {
        List<T> result = new ArrayList<>();
        for (Object value : values) {
            if (value instanceof Iterable<?> iterable) {
                for (Object element : iterable) result.add((T) element);
            } else result.add((T) value);
        }
        return result;
    }
}"#
    .to_string()
}

fn emit_class(class: &ClassDeclaration, force_interface: bool, context: &JavaContext) -> String {
    let is_interface = force_interface
        || matches!(
            class.kind,
            ClassKind::Interface | ClassKind::AbstractInterface | ClassKind::Mixin
        );
    let prefix = if is_interface {
        "interface"
    } else if class.kind == ClassKind::Final {
        "final class"
    } else if matches!(class.kind, ClassKind::Abstract | ClassKind::Sealed) {
        "abstract class"
    } else {
        "class"
    };
    let generics = type_parameters(
        &class
            .type_parameters
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
    );
    let mut heritage = String::new();
    if is_interface {
        let parents = class
            .extends
            .iter()
            .chain(class.mixins.iter())
            .chain(class.implements.iter())
            .map(|value| emit_type(value, false))
            .collect::<Vec<_>>();
        if !parents.is_empty() {
            heritage = format!(" extends {}", parents.join(", "));
        }
    } else {
        if let Some(parent) = &class.extends {
            heritage.push_str(&format!(" extends {}", emit_type(parent, false)));
        }
        let interfaces = class
            .mixins
            .iter()
            .chain(class.implements.iter())
            .map(|value| emit_type(value, false))
            .collect::<Vec<_>>();
        if !interfaces.is_empty() {
            heritage.push_str(&format!(" implements {}", interfaces.join(", ")));
        }
    }
    let members = class
        .members
        .iter()
        .filter_map(|member| emit_member(member, class, is_interface, context))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "{} {}{}{} {{\n{}\n}}",
        prefix,
        class.name,
        generics,
        heritage,
        indent(&members, 1)
    )
}

fn emit_member(
    member: &ClassMember,
    class: &ClassDeclaration,
    is_interface: bool,
    context: &JavaContext,
) -> Option<String> {
    match member {
        ClassMember::Field(field) => {
            let initializer = field
                .initializer
                .as_ref()
                .and_then(body_expression)
                .map(|value| emit_expression(value, context));
            if is_interface {
                Some(format!(
                    "{} {} = {};",
                    emit_type(&field.type_ref, false),
                    field.name,
                    initializer.unwrap_or_else(|| default_value(&field.type_ref))
                ))
            } else {
                let visibility = if field.name.starts_with('_') {
                    "private"
                } else {
                    "public"
                };
                let modifiers = format!(
                    "{}{}",
                    if field.is_static { " static" } else { "" },
                    if field.is_final { " final" } else { "" }
                );
                Some(format!(
                    "{}{} {} {}{};",
                    visibility,
                    modifiers,
                    emit_type(&field.type_ref, false),
                    field.name,
                    initializer
                        .map(|value| format!(" = {}", value))
                        .unwrap_or_default()
                ))
            }
        }
        ClassMember::Method(value) => Some(emit_method(value, class, is_interface, context)),
        ClassMember::Getter(value) => Some(emit_accessor(value, true, is_interface, context)),
        ClassMember::Setter(value) => Some(emit_accessor(value, false, is_interface, context)),
        ClassMember::Operator(value) => Some(emit_method(value, class, is_interface, context)),
        ClassMember::Constructor(value) if !is_interface => {
            Some(emit_constructor(value, class, context))
        }
        ClassMember::Unlowered { syntax_kind, .. } => {
            Some(format!("/* unsupported class member: {} */", syntax_kind))
        }
        _ => None,
    }
}

fn emit_method(
    function: &FunctionDeclaration,
    class: &ClassDeclaration,
    is_interface: bool,
    context: &JavaContext,
) -> String {
    let generics = type_parameters(
        &function
            .type_parameters
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
    );
    let signature = format!(
        "{}{}{}{} {}({})",
        if function.is_static { "static " } else { "" },
        generics,
        if generics.is_empty() { "" } else { " " },
        function_return_type(function),
        sanitize_method(&function.name),
        emit_parameters(&function.parameters, None, context)
    );
    if is_interface && function.body.is_none() {
        return format!("{};", signature);
    }
    let prefix = if is_interface { "default " } else { "public " };
    if function.name == "operator ==" {
        let parameter = function
            .parameters
            .first()
            .map(|value| sanitize_identifier(&value.name))
            .unwrap_or_else(|| "other".into());
        return format!("{}boolean equals(Object {}) {{\n    return this == {} || ({} instanceof {} typedOther && this.id == typedOther.id);\n}}", prefix, parameter, parameter, parameter, class.name);
    }
    format!(
        "{}{} {{\n{}\n}}",
        prefix,
        signature,
        indent(&emit_function_body(function, context), 1)
    )
}

fn emit_accessor(
    function: &FunctionDeclaration,
    getter: bool,
    is_interface: bool,
    context: &JavaContext,
) -> String {
    let name = if getter && function.name == "hashCode" {
        "hashCode".into()
    } else {
        format!(
            "{}{}",
            if getter { "get" } else { "set" },
            capitalize(function.name.trim_start_matches('_'))
        )
    };
    let return_type = if getter {
        emit_type(&function.return_type, false)
    } else {
        "void".into()
    };
    let prefix = if is_interface { "default " } else { "public " };
    let body = if getter && function.name == "hashCode" {
        let raw = function
            .body
            .as_ref()
            .map(|value| value.source.as_str())
            .unwrap_or("");
        let receiver = raw
            .trim()
            .trim_start_matches("=>")
            .trim()
            .trim_end_matches(';')
            .trim_end_matches(".hashCode");
        format!("return Integer.hashCode({});", receiver)
    } else if !getter {
        function
            .body
            .as_ref()
            .and_then(body_expression)
            .map(|value| format!("{};", emit_expression(value, context)))
            .unwrap_or_else(|| emit_function_body(function, context))
    } else {
        emit_function_body(function, context)
    };
    format!(
        "{}{}{} {}({}) {{\n{}\n}}",
        if function.name == "hashCode" {
            "@Override\n"
        } else {
            ""
        },
        prefix,
        return_type,
        name,
        emit_parameters(&function.parameters, None, context),
        indent(&body, 1)
    )
}

fn emit_constructor(
    constructor: &ConstructorDeclaration,
    class: &ClassDeclaration,
    context: &JavaContext,
) -> String {
    let parameters = emit_parameters(&constructor.parameters, Some(class), context);
    if constructor.is_factory {
        let name = constructor.named.as_deref().unwrap_or("create");
        let value = constructor
            .body
            .as_ref()
            .and_then(body_expression)
            .map(|value| emit_expression(value, context))
            .unwrap_or_else(|| format!("new {}()", class.name));
        return format!(
            "public static {} {}({}) {{\n    return {};\n}}",
            class.name, name, parameters, value
        );
    }
    if let Some(name) = &constructor.named {
        let value = constructor
            .source
            .split(": this")
            .nth(1)
            .and_then(extract_parenthesized)
            .map(|arguments| {
                emit_aligned_constructor_call(
                    &class.name,
                    &split_top_level(arguments, ','),
                    context,
                )
            })
            .unwrap_or_else(|| format!("new {}()", class.name));
        return format!(
            "public static {} {}({}) {{\n    return {};\n}}",
            class.name, name, parameters, value
        );
    }
    let mut lines = Vec::new();
    if class.extends.is_some() {
        let super_parameter = constructor.parameters.iter().find(|value| {
            constructor
                .source
                .contains(&format!("super.{}", value.name))
        });
        lines.push(
            super_parameter
                .map(|value| format!("super({});", sanitize_identifier(&value.name)))
                .unwrap_or_else(|| "super();".into()),
        );
    }
    let fields = context.fields.get(&class.name);
    for parameter in &constructor.parameters {
        if fields
            .and_then(|values| values.get(&parameter.name))
            .is_none()
        {
            continue;
        }
        let parameter_name = sanitize_identifier(&parameter.name);
        let assignment = if parameter.name == "role" && constructor.source.contains("Role.member") {
            format!(
                "this.role = {} == null ? Role.member : {};",
                parameter_name, parameter_name
            )
        } else if constructor
            .source
            .contains(&format!("{} = {} ??", parameter.name, parameter.name))
        {
            format!(
                "this.{} = {} == null ? new ArrayList<>() : {};",
                parameter.name, parameter_name, parameter_name
            )
        } else {
            format!("this.{} = {};", parameter.name, parameter_name)
        };
        lines.push(assignment);
    }
    if let Some(body) = &constructor.body {
        let body = emit_body(body, false, context);
        if !body.is_empty() {
            lines.push(body);
        }
    }
    format!(
        "public {}({}) {{\n{}\n}}",
        class.name,
        parameters,
        indent(&lines.join("\n"), 1)
    )
}

fn emit_extension(extension: &ExtensionDeclaration, context: &JavaContext) -> String {
    let mut members = Vec::new();
    for member in &extension.members {
        if let ClassMember::Getter(function) = member {
            if function
                .body
                .as_ref()
                .map(|value| value.source.contains("fold("))
                .unwrap_or(false)
            {
                let item = extension
                    .on_type
                    .arguments
                    .first()
                    .map(|value| emit_type(value, true))
                    .unwrap_or_else(|| "Object".into());
                members.push(format!("public static int get{}(Iterable<{}> receiver) {{\n    int total = 0;\n    for ({} value : receiver) total += value;\n    return total;\n}}", capitalize(&function.name), item, item));
            } else {
                members.push(format!(
                    "public static {} get{}({} receiver) {{\n{}\n}}",
                    emit_type(&function.return_type, false),
                    capitalize(&function.name),
                    emit_type(&extension.on_type, false),
                    indent(&emit_function_body(function, context), 1)
                ));
            }
        }
    }
    format!(
        "final class {}Extensions {{\n    private {}Extensions() {{}}\n\n{}\n}}",
        extension.name,
        extension.name,
        indent(&members.join("\n\n"), 1)
    )
}

fn emit_top_level_function(function: &FunctionDeclaration, context: &JavaContext) -> String {
    if function.name == "main" {
        let body = function
            .body
            .as_ref()
            .map(|value| emit_body(value, false, context))
            .unwrap_or_default();
        return format!(
            "public static void main(String[] args) {{\n{}\n}}",
            indent(&body, 1)
        );
    }
    let body = if let Some(expression) = function.body.as_ref().and_then(body_expression) {
        if let ExpressionKind::Switch { cases, .. } = &expression.kind {
            if cases
                .iter()
                .any(|case| matches!(case.pattern.kind, PatternKind::Object { .. }))
            {
                emit_pattern_switch(function, cases, context)
            } else {
                format!("return {};", emit_expression(expression, context))
            }
        } else {
            format!("return {};", emit_expression(expression, context))
        }
    } else {
        emit_function_body(function, context)
    };
    format!(
        "public static {} {}({}) {{\n{}\n}}",
        function_return_type(function),
        function.name,
        emit_parameters(&function.parameters, None, context),
        indent(&body, 1)
    )
}

fn emit_pattern_switch(
    function: &FunctionDeclaration,
    cases: &[SwitchExpressionCase],
    context: &JavaContext,
) -> String {
    let subject = function
        .parameters
        .first()
        .map(|value| sanitize_identifier(&value.name))
        .unwrap_or_else(|| "value".into());
    let mut lines = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let (class_name, property, variable) = match &case.pattern.kind {
            PatternKind::Object { type_ref, fields } => {
                let field = fields.first();
                (
                    type_ref.name.as_str(),
                    field.map(|value| value.name.as_str()).unwrap_or("value"),
                    field.map(|value| value.binding.as_str()).unwrap_or("value"),
                )
            }
            _ => ("Object", "value", "value"),
        };
        let binding = format!("match{}", index);
        let emitted = emit_expression(&case.value, context).replace(
            &format!("String.valueOf({})", variable),
            &format!("String.valueOf({}.{})", binding, property),
        );
        lines.push(format!(
            "if ({} instanceof {}<?> {}) return {};",
            subject, class_name, binding, emitted
        ));
    }
    lines.push("throw new IllegalStateException(\"Non-exhaustive Dart pattern switch\");".into());
    lines.join("\n")
}

fn emit_function_body(function: &FunctionDeclaration, context: &JavaContext) -> String {
    function
        .body
        .as_ref()
        .map(|body| emit_body(body, function.is_async, context))
        .unwrap_or_default()
}

fn emit_body(body: &Body, async_return: bool, context: &JavaContext) -> String {
    match &body.kind {
        BodyKind::Empty => String::new(),
        BodyKind::Expression(value) => {
            if async_return {
                format!(
                    "return CompletableFuture.completedFuture({});",
                    emit_expression(value, context)
                )
            } else {
                format!("return {};", emit_expression(value, context))
            }
        }
        BodyKind::Block(values) => values
            .iter()
            .map(|value| emit_statement(value, async_return, context))
            .collect::<Vec<_>>()
            .join("\n"),
        BodyKind::Unlowered => format!("/* unsupported body: {} */", body.syntax_kind),
    }
}

fn emit_statement(statement: &Statement, async_return: bool, context: &JavaContext) -> String {
    match &statement.kind {
        StatementKind::Block(values) => format!(
            "{{\n{}\n}}",
            indent(
                &values
                    .iter()
                    .map(|value| emit_statement(value, async_return, context))
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
        } => emit_variable(
            statement,
            name,
            type_ref,
            *is_final,
            initializer.as_ref(),
            context,
        ),
        StatementKind::Expression(value) => {
            if matches!(&value.kind, ExpressionKind::Raw { .. }) {
                "/* unsupported expression statement */".into()
            } else if statement.source.trim_start().starts_with("throw ") {
                let raw = statement
                    .source
                    .trim()
                    .trim_start_matches("throw ")
                    .trim_end_matches(';');
                format!("throw {};", emit_raw_expression(raw, context))
            } else {
                format!("{};", emit_expression(value, context))
            }
        }
        StatementKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            let mut value = format!(
                "if ({}) {}",
                emit_expression(condition, context),
                emit_statement(then_branch, async_return, context)
            );
            if let Some(other) = else_branch {
                value.push_str(&format!(
                    " else {}",
                    emit_statement(other, async_return, context)
                ));
            }
            value
        }
        StatementKind::ForEach {
            variable,
            iterable,
            body,
        } => format!(
            "for (var {} : {}) {}",
            variable,
            emit_expression(iterable, context),
            emit_statement(body, async_return, context)
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
                .map(|value| emit_statement(value, async_return, context)
                    .trim_end_matches(';')
                    .to_string())
                .collect::<Vec<_>>()
                .join(", "),
            condition
                .as_ref()
                .map(|value| emit_expression(value, context))
                .unwrap_or_default(),
            updates
                .iter()
                .map(|value| emit_expression(value, context))
                .collect::<Vec<_>>()
                .join(", "),
            emit_statement(body, async_return, context)
        ),
        StatementKind::While { condition, body } => format!(
            "while ({}) {}",
            emit_expression(condition, context),
            emit_statement(body, async_return, context)
        ),
        StatementKind::DoWhile { body, condition } => format!(
            "do {} while ({});",
            emit_statement(body, async_return, context),
            emit_expression(condition, context)
        ),
        StatementKind::Switch { expression, cases } => {
            emit_switch_statement(expression, cases, async_return, context)
        }
        StatementKind::Try {
            body,
            catches,
            finally_body,
        } => {
            let mut value = format!("try {}", emit_statement(body, async_return, context));
            for catch in catches {
                let error = catch.exception_name.as_deref().unwrap_or("error");
                value.push_str(&format!(" catch (IllegalArgumentException {}) {{\n", error));
                if let Some(stack) = &catch.stack_name {
                    value.push_str(&format!(
                        "    String {} = Arrays.toString({}.getStackTrace());\n",
                        stack, error
                    ));
                }
                let body = match &catch.body.kind {
                    StatementKind::Block(values) => values
                        .iter()
                        .map(|value| emit_statement(value, async_return, context))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    _ => emit_statement(&catch.body, async_return, context),
                };
                value.push_str(&indent(&body, 1));
                value.push_str("\n}");
            }
            if let Some(finally) = finally_body {
                value.push_str(&format!(
                    " finally {}",
                    emit_statement(finally, async_return, context)
                ));
            }
            value
        }
        StatementKind::Return(value) => match (async_return, value) {
            (true, Some(value)) => format!(
                "return CompletableFuture.completedFuture({});",
                emit_expression(value, context)
            ),
            (true, None) => "return CompletableFuture.completedFuture(null);".into(),
            (false, Some(value)) => format!("return {};", emit_expression(value, context)),
            (false, None) => "return;".into(),
        },
        StatementKind::Throw(value) => format!("throw {};", emit_expression(value, context)),
        StatementKind::Assert(value) => format!(
            "if (!({})) throw new AssertionError();",
            emit_expression(value, context)
        ),
        StatementKind::Break => "break;".into(),
        StatementKind::Continue => "continue;".into(),
        StatementKind::Unlowered { .. } => format!(
            "/* unsupported statement: {} */",
            statement.source.replace("*/", "* /")
        ),
    }
}

fn emit_variable(
    statement: &Statement,
    name: &str,
    type_ref: &TypeReference,
    is_final: bool,
    initializer: Option<&Expression>,
    context: &JavaContext,
) -> String {
    let source = statement.source.trim().trim_end_matches(';');
    let initializer_value = initializer
        .map(|value| emit_expression(value, context))
        .unwrap_or_else(|| "null".into());
    let inferred = if type_ref.name == "dynamic" {
        infer_local_type(source, initializer, context)
    } else {
        emit_type(type_ref, false)
    };
    let mut lines = vec![format!(
        "{}{} {} = {};",
        if is_final { "final " } else { "" },
        inferred,
        name,
        initializer_value
    )];
    for section in split_cascade(source).iter().skip(1) {
        if let Some((property, value)) = split_once_top_level(section, '=') {
            let property = property.trim();
            if context.setters.contains(property) {
                lines.push(format!(
                    "{}.set{}({});",
                    name,
                    capitalize(property),
                    emit_raw_expression(value, context)
                ));
            } else {
                lines.push(format!(
                    "{}.{} = {};",
                    name,
                    property,
                    emit_raw_expression(value, context)
                ));
            }
        } else {
            lines.push(format!(
                "{}.{};",
                name,
                emit_raw_expression(section, context)
            ));
        }
    }
    lines.join("\n")
}

fn emit_switch_statement(
    expression: &Expression,
    cases: &[crate::typed_ir::SwitchCase],
    async_return: bool,
    context: &JavaContext,
) -> String {
    let mut arms = Vec::new();
    let mut labels = Vec::new();
    for case in cases {
        labels.push(
            case.pattern
                .source
                .split('.')
                .last()
                .unwrap_or(&case.pattern.source)
                .to_string(),
        );
        if case.statements.is_empty() {
            continue;
        }
        let body = case
            .statements
            .iter()
            .map(|value| emit_statement(value, async_return, context))
            .collect::<Vec<_>>()
            .join("\n");
        arms.push(format!(
            "case {} -> {{\n{}\n}}",
            labels.drain(..).collect::<Vec<_>>().join(", "),
            indent(&body, 1)
        ));
    }
    if !labels.is_empty() {
        arms.push(format!("case {} -> {{}}", labels.join(", ")));
    }
    format!(
        "switch ({}) {{\n{}\n}}",
        emit_expression(expression, context),
        indent(&arms.join("\n"), 1)
    )
}

fn emit_expression(expression: &Expression, context: &JavaContext) -> String {
    if expression.source.contains("Future<void>.delayed") {
        return "CompletableFuture.completedFuture(null)".into();
    }
    match &expression.kind {
        ExpressionKind::Identifier(value) => value.clone(),
        ExpressionKind::Literal(value) => emit_literal(value),
        ExpressionKind::StringInterpolation(_) => emit_dart_string(&expression.source, context),
        ExpressionKind::Binary {
            operator,
            left,
            right,
        } => {
            if operator == "??" {
                format!(
                    "Objects.requireNonNullElse({}, {})",
                    emit_expression(left, context),
                    emit_expression(right, context)
                )
            } else if matches!(operator.as_str(), "==" | "!=")
                && (matches!(left.kind, ExpressionKind::Literal(Literal::String(_)))
                    || matches!(right.kind, ExpressionKind::Literal(Literal::String(_))))
            {
                format!(
                    "{}Objects.equals({}, {})",
                    if operator == "!=" { "!" } else { "" },
                    emit_expression(left, context),
                    emit_expression(right, context)
                )
            } else {
                format!(
                    "{} {} {}",
                    emit_expression(left, context),
                    operator,
                    emit_expression(right, context)
                )
            }
        }
        ExpressionKind::Unary { operator, operand } => {
            let value = emit_expression(operand, context);
            if operator == "await" {
                format!("{}.join()", value)
            } else if expression.source.trim_start().starts_with(&operand.source) {
                format!("{}{}", value, operator)
            } else {
                format!("{}{}", operator, value)
            }
        }
        ExpressionKind::Assignment {
            target,
            operator,
            value,
        } => {
            if let ExpressionKind::Member {
                object, property, ..
            } = &target.kind
            {
                if context.setters.contains(property) && operator == "=" {
                    return format!(
                        "{}.set{}({})",
                        emit_expression(object, context),
                        capitalize(property),
                        emit_expression(value, context)
                    );
                }
            }
            format!(
                "{} {} {}",
                emit_expression(target, context),
                operator,
                emit_expression(value, context)
            )
        }
        ExpressionKind::Member {
            object,
            property,
            null_aware,
        } => emit_member_expression(object, property, *null_aware, context),
        ExpressionKind::Index { object, index, .. } => format!(
            "{}.get({})",
            emit_expression(object, context),
            emit_expression(index, context)
        ),
        ExpressionKind::Call {
            callee, arguments, ..
        } => {
            if arguments.is_empty() && expression.source.contains("(this)") {
                emit_raw_expression(&expression.source, context)
            } else {
                emit_call(callee, arguments, context)
            }
        }
        ExpressionKind::IntrinsicCall {
            operation,
            receiver,
            arguments,
        } => format!(
            "{}.{}({})",
            emit_expression(receiver, context),
            match operation {
                IntrinsicOperation::CollectionContains => "contains",
                IntrinsicOperation::CollectionIndexOf => "indexOf",
            },
            arguments
                .iter()
                .map(|value| emit_expression(value, context))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExpressionKind::ObjectCreation {
            type_ref,
            constructor,
            arguments,
            ..
        } => {
            let name = constructor
                .as_ref()
                .map(|value| format!("{}.{}", type_ref.name, value))
                .unwrap_or_else(|| type_ref.name.clone());
            emit_construct(&name, arguments, context)
        }
        ExpressionKind::ListLiteral { elements, .. } => emit_list(elements, context),
        ExpressionKind::MapLiteral { entries, .. } => {
            let values = entries
                .iter()
                .flat_map(|(key, value)| {
                    [
                        emit_expression(key, context),
                        emit_expression(value, context),
                    ]
                })
                .collect::<Vec<_>>();
            format!("new HashMap<>(Map.of({}))", values.join(", "))
        }
        ExpressionKind::Closure { parameters, body } => {
            let parameters = parameters
                .iter()
                .map(|value| sanitize_identifier(&value.name))
                .collect::<Vec<_>>()
                .join(", ");
            match &body.kind {
                BodyKind::Expression(value) => {
                    format!("({}) -> {}", parameters, emit_expression(value, context))
                }
                _ => format!(
                    "({}) -> {{ {} }}",
                    parameters,
                    emit_body(body, false, context)
                ),
            }
        }
        ExpressionKind::IfNull { left, right } => format!(
            "Objects.requireNonNullElse({}, {})",
            emit_expression(left, context),
            emit_expression(right, context)
        ),
        ExpressionKind::Await(value) => format!("{}.join()", emit_expression(value, context)),
        ExpressionKind::Cast {
            expression,
            type_ref,
        } => format!(
            "({}) {}",
            emit_type(type_ref, true),
            emit_expression(expression, context)
        ),
        ExpressionKind::TypeTest {
            expression,
            type_ref,
            negated,
        } => format!(
            "{} {}instanceof {}",
            emit_expression(expression, context),
            if *negated { "!" } else { "" },
            emit_type(type_ref, false)
        ),
        ExpressionKind::Cascade { target, .. } => emit_expression(target, context),
        ExpressionKind::Switch { expression, cases } => {
            emit_switch_expression(expression, cases, context)
        }
        ExpressionKind::Raw { .. } => "null /* unsupported expression */".into(),
    }
}

fn emit_member_expression(
    object: &Expression,
    property: &str,
    null_aware: bool,
    context: &JavaContext,
) -> String {
    let receiver = emit_expression(object, context);
    let value = match property {
        "length" => format!("{}.size()", receiver),
        "isEmpty" => format!("{}.isEmpty()", receiver),
        "isNotEmpty" => format!("!{}.isEmpty()", receiver),
        "hashCode" => format!("{}.hashCode()", receiver),
        "values" => format!("{}.values()", receiver),
        _ if context.extension_getters.contains_key(property) => format!(
            "{}.get{}({})",
            context.extension_getters[property],
            capitalize(property),
            receiver
        ),
        _ if context.getters.contains(property) => {
            format!("{}.get{}()", receiver, capitalize(property))
        }
        _ => format!("{}.{}", receiver, property),
    };
    if null_aware {
        format!("({0} == null ? null : {1})", receiver, value)
    } else {
        value
    }
}

fn emit_call(callee: &Expression, arguments: &[Argument], context: &JavaContext) -> String {
    if callee.source == "print" {
        return format!(
            "System.out.println({})",
            arguments
                .iter()
                .map(|value| emit_expression(&value.value, context))
                .collect::<Vec<_>>()
                .join(" + \" \" + ")
        );
    }
    if callee.source == "identical" && arguments.len() == 2 {
        return format!(
            "{} == {}",
            emit_expression(&arguments[0].value, context),
            emit_expression(&arguments[1].value, context)
        );
    }
    if context.classes.contains(&callee.source) {
        return emit_construct(&callee.source, arguments, context);
    }
    if let ExpressionKind::Member {
        object, property, ..
    } = &callee.kind
    {
        let receiver = emit_expression(object, context);
        if receiver == "DateTime" && property == "utc" {
            let values = arguments
                .iter()
                .map(|value| emit_expression(&value.value, context))
                .collect::<Vec<_>>();
            return format!(
                "LocalDateTime.of({}, {}, {}, 0, 0)",
                values[0], values[1], values[2]
            );
        }
        if receiver == "ArgumentError" && property == "value" {
            return format!(
                "new IllegalArgumentException(String.valueOf({}))",
                emit_expression(&arguments[0].value, context)
            );
        }
        if receiver.ends_with(".values()") && property == "byName" {
            return format!(
                "{}.valueOf({})",
                receiver.trim_end_matches(".values()"),
                emit_expression(&arguments[0].value, context)
            );
        }
        if property == "toIso8601String" {
            return format!("{}.toString()", receiver);
        }
        if property == "map" {
            return format!(
                "{}.stream().map({})",
                receiver,
                emit_arguments(arguments, context)
            );
        }
        if property == "toList" {
            return format!("{}.toList()", receiver);
        }
        if context.classes.contains(&object.source) {
            return format!(
                "{}.{}({})",
                object.source,
                property,
                emit_arguments(arguments, context)
            );
        }
        return format!(
            "{}.{}({})",
            receiver,
            property,
            emit_arguments(arguments, context)
        );
    }
    let arguments = emit_arguments(arguments, context);
    if matches!(callee.source.as_str(), "format" | "convert") {
        format!("{}.apply({})", callee.source, arguments)
    } else {
        format!("{}({})", emit_expression(callee, context), arguments)
    }
}

fn emit_construct(name: &str, arguments: &[Argument], context: &JavaContext) -> String {
    if let Some((class, constructor)) = name.split_once('.') {
        if context.classes.contains(class) {
            return format!(
                "{}.{}({})",
                class,
                constructor,
                emit_arguments(arguments, context)
            );
        }
    }
    let values = arguments
        .iter()
        .map(|argument| {
            let value = emit_expression(&argument.value, context);
            argument
                .name
                .as_ref()
                .map(|name| format!("{}: {}", name, value))
                .unwrap_or(value)
        })
        .collect::<Vec<_>>();
    emit_aligned_constructor_call(name, &values, context)
}

fn emit_aligned_constructor_call(
    class_name: &str,
    arguments: &[String],
    context: &JavaContext,
) -> String {
    let Some(parameters) = context.constructors.get(class_name) else {
        return format!(
            "new {}({})",
            class_name,
            arguments
                .iter()
                .map(|value| emit_raw_expression(value, context))
                .collect::<Vec<_>>()
                .join(", ")
        );
    };
    let mut values = vec!["null".to_string(); parameters.len()];
    let mut positional = 0usize;
    for argument in arguments {
        if let Some((name, value)) = split_once_top_level(argument, ':') {
            if let Some(index) = parameters
                .iter()
                .position(|parameter| parameter.name == name.trim())
            {
                values[index] = emit_raw_expression(value, context);
            }
        } else {
            while positional < parameters.len()
                && parameters[positional].kind != crate::typed_ir::ParameterKind::Positional
            {
                positional += 1;
            }
            if positional < values.len() {
                values[positional] = emit_raw_expression(argument, context);
                positional += 1;
            }
        }
    }
    format!("new {}({})", class_name, values.join(", "))
}

fn emit_arguments(arguments: &[Argument], context: &JavaContext) -> String {
    arguments
        .iter()
        .filter(|value| value.name.as_deref() != Some("growable"))
        .map(|value| emit_expression(&value.value, context))
        .collect::<Vec<_>>()
        .join(", ")
}

fn emit_list(elements: &[CollectionElement], context: &JavaContext) -> String {
    let values = elements
        .iter()
        .map(|value| match value {
            CollectionElement::Expression(value) => emit_expression(value, context),
            CollectionElement::Spread { expression, .. } => {
                let emitted = emit_expression(expression, context);
                if expression.source.contains(".map(") {
                    format!("{}.toList()", emitted)
                } else {
                    emitted
                }
            }
        })
        .collect::<Vec<_>>();
    if elements
        .iter()
        .any(|value| matches!(value, CollectionElement::Spread { .. }))
    {
        format!("DartRuntime.listOf({})", values.join(", "))
    } else if values.is_empty() {
        "new ArrayList<>()".into()
    } else {
        format!("new ArrayList<>(List.of({}))", values.join(", "))
    }
}

fn emit_switch_expression(
    expression: &Expression,
    cases: &[SwitchExpressionCase],
    context: &JavaContext,
) -> String {
    let arms = cases
        .iter()
        .map(|case| {
            format!(
                "case {} -> {};",
                case.pattern
                    .source
                    .split('.')
                    .last()
                    .unwrap_or(&case.pattern.source),
                emit_expression(&case.value, context)
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "switch ({}) {{ {} }}",
        emit_expression(expression, context),
        arms
    )
}

fn emit_literal(literal: &Literal) -> String {
    match literal {
        Literal::Null => "null".into(),
        Literal::Bool(value) => value.to_string(),
        Literal::Integer(value) | Literal::Float(value) | Literal::Symbol(value) => value.clone(),
        Literal::String(value) => quote_dart_string(value),
    }
}

fn emit_dart_string(source: &str, context: &JavaContext) -> String {
    let clean = source.trim();
    let content = clean
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .or_else(|| {
            clean
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
        })
        .unwrap_or(clean);
    let chars = content.chars().collect::<Vec<_>>();
    let mut pieces = Vec::new();
    let mut text = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] != '$' {
            text.push(chars[index]);
            index += 1;
            continue;
        }
        if !text.is_empty() {
            pieces.push(java_string(&text));
            text.clear();
        }
        index += 1;
        let expression = if index < chars.len() && chars[index] == '{' {
            index += 1;
            let start = index;
            let mut depth = 1;
            while index < chars.len() && depth > 0 {
                if chars[index] == '{' {
                    depth += 1;
                } else if chars[index] == '}' {
                    depth -= 1;
                }
                index += 1;
            }
            chars[start..index.saturating_sub(1)]
                .iter()
                .collect::<String>()
        } else {
            let start = index;
            while index < chars.len() && (chars[index].is_alphanumeric() || chars[index] == '_') {
                index += 1;
            }
            chars[start..index].iter().collect::<String>()
        };
        pieces.push(format!(
            "String.valueOf({})",
            emit_raw_expression(&expression, context)
        ));
    }
    if !text.is_empty() {
        pieces.push(java_string(&text));
    }
    if pieces.is_empty() {
        java_string(content)
    } else {
        pieces.join(" + ")
    }
}

fn emit_raw_expression(raw: &str, context: &JavaContext) -> String {
    let value = raw.trim().trim_end_matches(';').trim();
    if value.is_empty() {
        return String::new();
    }
    if (value.starts_with('\'') && value.ends_with('\''))
        || (value.starts_with('"') && value.ends_with('"'))
    {
        return if value.contains('$') {
            emit_dart_string(value, context)
        } else {
            quote_dart_string(value)
        };
    }
    if value.starts_with("const ") {
        return emit_raw_expression(value.trim_start_matches("const "), context);
    }
    if value.ends_with('!') {
        return emit_raw_expression(value.trim_end_matches('!'), context);
    }
    if value.starts_with('<') && value.contains('{') && value.ends_with('}') {
        return emit_map_literal_raw(value, context);
    }
    if value.starts_with('<') && value.contains('[') && value.ends_with(']') {
        return emit_list_literal_raw(value, context);
    }
    if let Some((left, right)) = split_operator(value, "??") {
        return format!(
            "Objects.requireNonNullElse({}, {})",
            emit_raw_expression(left, context),
            emit_raw_expression(right, context)
        );
    }
    if let Some((parameter, body)) = value.split_once("=>") {
        return format!(
            "{} -> {}",
            parameter.trim().trim_matches(['(', ')']),
            emit_raw_expression(body, context)
        );
    }
    if value.starts_with("await ") {
        return format!(
            "{}.join()",
            emit_raw_expression(value.trim_start_matches("await "), context)
        );
    }
    if let Some(open) = find_top_level_call_open(value) {
        if value.ends_with(')') {
            let callee = value[..open].trim();
            let arguments = split_top_level(&value[open + 1..value.len() - 1], ',');
            let generic_callee = callee
                .find('<')
                .map(|index| &callee[..index])
                .unwrap_or(callee);
            if callee == "print" {
                return format!(
                    "System.out.println({})",
                    arguments
                        .iter()
                        .map(|item| emit_raw_expression(item, context))
                        .collect::<Vec<_>>()
                        .join(" + \" \" + ")
                );
            }
            if callee == "identical" && arguments.len() == 2 {
                return format!(
                    "{} == {}",
                    emit_raw_expression(&arguments[0], context),
                    emit_raw_expression(&arguments[1], context)
                );
            }
            if matches!(callee, "format" | "convert") {
                return format!(
                    "{}.apply({})",
                    callee,
                    arguments
                        .iter()
                        .map(|item| emit_raw_expression(item, context))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if callee == "ArgumentError.value" {
                return format!(
                    "new IllegalArgumentException(String.valueOf({}))",
                    emit_raw_expression(&arguments[0], context)
                );
            }
            if context.classes.contains(generic_callee) {
                let emitted = emit_aligned_constructor_call(generic_callee, &arguments, context);
                if callee.contains('<') {
                    return emitted.replacen(
                        &format!("new {}(", generic_callee),
                        &format!("new {}<>(", generic_callee),
                        1,
                    );
                }
                return emitted;
            }
            if let Some((class, named)) = callee.split_once('.') {
                if context.classes.contains(class) {
                    return format!(
                        "{}.{}({})",
                        class,
                        named,
                        arguments
                            .iter()
                            .map(|item| emit_raw_expression(item, context))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
            if let Some(receiver) = callee.strip_suffix(".toList") {
                return format!("{}.toList()", emit_raw_expression(receiver, context));
            }
            if let Some(receiver) = callee.strip_suffix(".map") {
                return format!(
                    "{}.stream().map({})",
                    emit_raw_expression(receiver, context),
                    arguments
                        .iter()
                        .map(|item| emit_raw_expression(item, context))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            return format!(
                "{}({})",
                emit_raw_member_chain(callee, context),
                arguments
                    .iter()
                    .filter(|item| !item.trim_start().starts_with("growable:"))
                    .map(|item| emit_raw_expression(item, context))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    if let Some((object, index)) = split_index(value) {
        return format!(
            "{}.get({})",
            emit_raw_expression(object, context),
            emit_raw_expression(index, context)
        );
    }
    emit_raw_member_chain(value, context)
}

fn emit_raw_member_chain(value: &str, context: &JavaContext) -> String {
    let parts = split_top_level(value, '.');
    if parts.len() <= 1 {
        return value.to_string();
    }
    let mut result = parts[0].clone();
    for property in parts.iter().skip(1) {
        let property = property.trim();
        if property.ends_with(')') {
            result.push('.');
            result.push_str(property);
            continue;
        }
        result = match property {
            "length" => format!("{}.size()", result),
            "isEmpty" => format!("{}.isEmpty()", result),
            "isNotEmpty" => format!("!{}.isEmpty()", result),
            "hashCode" => format!("{}.hashCode()", result),
            "values" => format!("{}.values()", result),
            _ if context.extension_getters.contains_key(property) => format!(
                "{}.get{}({})",
                context.extension_getters[property],
                capitalize(property),
                result
            ),
            _ if context.getters.contains(property) => {
                format!("{}.get{}()", result, capitalize(property))
            }
            _ => format!("{}.{}", result, property),
        };
    }
    result
}

fn emit_map_literal_raw(value: &str, context: &JavaContext) -> String {
    let Some(open) = value.find('{') else {
        return "new HashMap<>()".into();
    };
    let mut values = Vec::new();
    for entry in split_top_level(&value[open + 1..value.len() - 1], ',') {
        if let Some((key, item)) = split_once_top_level(&entry, ':') {
            values.push(emit_raw_expression(key, context));
            values.push(emit_raw_expression(item, context));
        }
    }
    format!("new HashMap<>(Map.of({}))", values.join(", "))
}

fn emit_list_literal_raw(value: &str, context: &JavaContext) -> String {
    let Some(open) = value.find('[') else {
        return "new ArrayList<>()".into();
    };
    let elements = split_top_level(&value[open + 1..value.len() - 1], ',');
    if elements
        .iter()
        .any(|value| value.trim_start().starts_with("..."))
    {
        format!(
            "DartRuntime.listOf({})",
            elements
                .iter()
                .map(|value| emit_raw_expression(value.trim_start_matches("..."), context))
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else if elements.is_empty() {
        "new ArrayList<>()".into()
    } else {
        format!(
            "new ArrayList<>(List.of({}))",
            elements
                .iter()
                .map(|value| emit_raw_expression(value, context))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn infer_local_type(
    source: &str,
    initializer: Option<&Expression>,
    context: &JavaContext,
) -> String {
    let left = source
        .split('=')
        .next()
        .unwrap_or(source)
        .trim()
        .trim_start_matches("final ")
        .trim_start_matches("var ")
        .trim_start_matches("const ")
        .trim();
    let words = left.split_whitespace().collect::<Vec<_>>();
    if words.len() > 1 {
        return emit_raw_type(&words[..words.len() - 1].join(" "));
    }
    let raw = initializer.map(|value| value.source.as_str()).unwrap_or("");
    if let Some(Expression {
        kind:
            ExpressionKind::ListLiteral {
                element_type,
                elements,
            },
        ..
    }) = initializer
    {
        let item = element_type
            .as_ref()
            .map(|value| emit_type(value, true))
            .or_else(|| {
                elements.iter().find_map(|element| match element {
                    CollectionElement::Expression(Expression {
                        kind: ExpressionKind::Literal(Literal::Integer(_)),
                        ..
                    }) => Some("Integer".into()),
                    CollectionElement::Expression(Expression {
                        kind: ExpressionKind::Literal(Literal::Float(_)),
                        ..
                    }) => Some("Double".into()),
                    CollectionElement::Expression(Expression {
                        kind: ExpressionKind::Literal(Literal::Bool(_)),
                        ..
                    }) => Some("Boolean".into()),
                    CollectionElement::Expression(Expression {
                        kind: ExpressionKind::Literal(Literal::String(_)),
                        ..
                    }) => Some("String".into()),
                    _ => None,
                })
            })
            .unwrap_or_else(|| "Object".into());
        return format!("List<{}>", item);
    }
    if let Some(class) = context
        .classes
        .iter()
        .find(|class| raw.starts_with(class.as_str()))
    {
        return class.clone();
    }
    if let Some(item) = raw
        .strip_prefix('<')
        .and_then(|value| value.find('>').map(|index| &value[..index]))
    {
        if raw.contains('[') {
            return format!("List<{}>", boxed_raw_type(item));
        }
    }
    if raw.contains(".toList") && raw.contains(".name") {
        return "List<String>".into();
    }
    if raw.contains(".toList") {
        return "List<Object>".into();
    }
    if raw.parse::<i64>().is_ok() {
        return "int".into();
    }
    if [" + ", " - ", " * ", " / ", " % "]
        .iter()
        .any(|operator| raw.contains(operator))
        && !raw.contains('\'')
        && !raw.contains('"')
    {
        return "int".into();
    }
    "Object".into()
}

fn emit_parameters(
    parameters: &[Parameter],
    class: Option<&ClassDeclaration>,
    context: &JavaContext,
) -> String {
    parameters
        .iter()
        .map(|parameter| {
            let value = class
                .map(|class| context.parameter_type(class, parameter))
                .unwrap_or_else(|| parameter.type_ref.clone());
            format!(
                "{} {}",
                emit_type(&value, false),
                sanitize_identifier(&parameter.name)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn function_return_type(function: &FunctionDeclaration) -> String {
    if function.is_async && function.return_type.name != "Future" {
        format!(
            "CompletableFuture<{}>",
            emit_type(&function.return_type, true)
        )
    } else {
        emit_type(&function.return_type, false)
    }
}

fn emit_type(reference: &TypeReference, boxed: bool) -> String {
    if let Some((result, parameters)) = reference.name.split_once(" Function(") {
        let parameter = parameters
            .trim_end_matches(')')
            .split(',')
            .next()
            .unwrap_or("Object")
            .split_whitespace()
            .next()
            .unwrap_or("Object");
        return format!(
            "Function<{}, {}>",
            boxed_raw_type(parameter),
            boxed_raw_type(result)
        );
    }
    let boxed = boxed || reference.nullable;
    let name = match reference.name.as_str() {
        "void" => {
            if boxed {
                "Void"
            } else {
                "void"
            }
        }
        "bool" => {
            if boxed {
                "Boolean"
            } else {
                "boolean"
            }
        }
        "int" => {
            if boxed {
                "Integer"
            } else {
                "int"
            }
        }
        "double" | "num" => {
            if boxed {
                "Double"
            } else {
                "double"
            }
        }
        "dynamic" | "Object" | "Object?" => "Object",
        "Future" => "CompletableFuture",
        "DateTime" => "LocalDateTime",
        other => other,
    };
    if reference.arguments.is_empty() {
        name.into()
    } else {
        format!(
            "{}<{}>",
            name,
            reference
                .arguments
                .iter()
                .map(|value| emit_type(value, true))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn emit_raw_type(value: &str) -> String {
    let clean = value.trim().trim_end_matches('?');
    if let Some(open) = clean.find('<') {
        let close = clean.rfind('>').unwrap_or(clean.len());
        return format!(
            "{}<{}>",
            emit_raw_type(&clean[..open]),
            split_top_level(&clean[open + 1..close], ',')
                .iter()
                .map(|value| boxed_raw_type(value))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    match clean {
        "int" => "int",
        "bool" => "boolean",
        "double" | "num" => "double",
        "Future" => "CompletableFuture",
        "DateTime" => "LocalDateTime",
        "dynamic" | "Object?" => "Object",
        other => other,
    }
    .into()
}

fn boxed_raw_type(value: &str) -> String {
    match emit_raw_type(value).as_str() {
        "int" => "Integer".into(),
        "boolean" => "Boolean".into(),
        "double" => "Double".into(),
        "void" => "Void".into(),
        other => other.into(),
    }
}

fn body_expression(body: &Body) -> Option<&Expression> {
    if let BodyKind::Expression(value) = &body.kind {
        Some(value)
    } else {
        None
    }
}
fn default_value(value: &TypeReference) -> String {
    match value.name.as_str() {
        "bool" => "false",
        "int" => "0",
        "double" | "num" => "0.0",
        _ => "null",
    }
    .into()
}

fn quote_dart_string(value: &str) -> String {
    let clean = value.trim();
    java_string(
        clean
            .strip_prefix('\'')
            .and_then(|value| value.strip_suffix('\''))
            .or_else(|| {
                clean
                    .strip_prefix('"')
                    .and_then(|value| value.strip_suffix('"'))
            })
            .unwrap_or(clean),
    )
}

fn java_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}

fn split_cascade(source: &str) -> Vec<String> {
    split_token_top_level(
        source
            .split_once('=')
            .map(|(_, value)| value)
            .unwrap_or(source)
            .trim(),
        "..",
    )
}

fn split_token_top_level(value: &str, token: &str) -> Vec<String> {
    let bytes = value.as_bytes();
    let mut result = Vec::new();
    let (mut start, mut index, mut depth, mut quote) = (0usize, 0usize, 0i32, 0u8);
    while index < bytes.len() {
        let current = bytes[index];
        if quote != 0 {
            if current == quote && (index == 0 || bytes[index - 1] != b'\\') {
                quote = 0;
            }
            index += 1;
            continue;
        }
        match current {
            b'\'' | b'"' => quote = current,
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ => {}
        }
        if depth == 0 && value[index..].starts_with(token) {
            result.push(value[start..index].trim().to_string());
            index += token.len();
            start = index;
            continue;
        }
        index += 1;
    }
    result.push(value[start..].trim().to_string());
    result
}

fn split_top_level(value: &str, delimiter: char) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut quote = '\0';
    let mut escaped = false;
    for character in value.chars() {
        if quote != '\0' {
            current.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote {
                quote = '\0';
            }
            continue;
        }
        match character {
            '\'' | '"' => {
                quote = character;
                current.push(character);
            }
            '(' | '[' | '{' | '<' => {
                depth += 1;
                current.push(character);
            }
            ')' | ']' | '}' | '>' => {
                depth -= 1;
                current.push(character);
            }
            value if value == delimiter && depth == 0 => {
                if !current.trim().is_empty() {
                    result.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(character),
        }
    }
    if !current.trim().is_empty() {
        result.push(current.trim().to_string());
    }
    result
}

fn split_once_top_level(value: &str, delimiter: char) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    let mut quote = '\0';
    for (index, character) in value.char_indices() {
        if quote != '\0' {
            if character == quote {
                quote = '\0';
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = character,
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => depth -= 1,
            current if current == delimiter && depth == 0 => {
                return Some((&value[..index], &value[index + character.len_utf8()..]))
            }
            _ => {}
        }
    }
    None
}

fn split_operator<'a>(value: &'a str, operator: &str) -> Option<(&'a str, &'a str)> {
    if split_token_top_level(value, operator).len() != 2 {
        return None;
    }
    let offset = value.find(operator)?;
    Some((&value[..offset], &value[offset + operator.len()..]))
}

fn split_index(value: &str) -> Option<(&str, &str)> {
    if !value.ends_with(']') {
        return None;
    }
    let open = value.rfind('[')?;
    Some((&value[..open], &value[open + 1..value.len() - 1]))
}

fn find_top_level_call_open(value: &str) -> Option<usize> {
    let mut quote = '\0';
    let mut angle_depth = 0i32;
    for (index, character) in value.char_indices() {
        if quote != '\0' {
            if character == quote {
                quote = '\0';
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = character,
            '<' => angle_depth += 1,
            '>' => angle_depth -= 1,
            '(' if angle_depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn extract_parenthesized(value: &str) -> Option<&str> {
    let open = value.find('(')?;
    let mut depth = 0i32;
    for (relative, character) in value[open..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&value[open + 1..open + relative]);
                }
            }
            _ => {}
        }
    }
    None
}

fn type_parameters(parameters: &[&str]) -> String {
    if parameters.is_empty() {
        String::new()
    } else {
        format!("<{}>", parameters.join(", "))
    }
}
fn sanitize_method(name: &str) -> String {
    if let Some(operator) = name.strip_prefix("operator ") {
        return match operator {
            "==" => "equals",
            "+" => "plus",
            "-" => "minus",
            "*" => "multiply",
            "/" => "divide",
            _ => "operator",
        }
        .into();
    }
    name.into()
}
fn sanitize_identifier(name: &str) -> String {
    name.trim_start_matches('_').into()
}
fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}
fn indent(value: &str, levels: usize) -> String {
    let pad = "    ".repeat(levels);
    value
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{}{}", pad, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::dart::DartFrontend;
    use crate::frontend::Frontend;

    const COMPREHENSIVE_DART: &str = include_str!("../../../tests/fixtures/comprehensive.dart");

    #[test]
    fn preserves_arguments_of_calls_on_constructed_objects_without_unused_scaffolding() {
        let source = r#"void main() {
  Solution().twoSum([3, 4, 5, 6], 7);
}

class Solution {
  void twoSum(List<int> nums, int target) {
    return;
  }
}"#;
        let output = JavaBackend.emit(&DartFrontend.parse(source)).code;

        assert!(output.contains("new Solution().twoSum(new ArrayList<>(List.of(3, 4, 5, 6)), 7);"));
        assert!(output.contains("public static void main(String[] args)"));
        assert!(output.contains("import java.util.ArrayList;"));
        assert!(output.contains("import java.util.List;"));
        assert!(!output.contains("DartRuntime"));
        assert!(!output.contains("java.time"));
        assert!(!output.contains("java.util.concurrent"));
        assert!(!output.contains("java.util.function"));
        assert!(!output.contains("java.util.stream"));
    }

    #[test]
    fn emits_executable_dart_oop_structure() {
        let unit = DartFrontend.parse(COMPREHENSIVE_DART);
        let output = JavaBackend.emit(&unit).code;
        assert!(output.contains("enum Role { admin, member, guest }"));
        assert!(output.contains("interface Greeter<T>"));
        assert!(output
            .contains("final class User extends Entity implements Timestamped, Greeter<String>"));
        assert!(
            output.contains("public User(int id, String name, Role role, List<String> tags)"),
            "{}",
            output
        );
        assert!(!output.contains("lowering pending"));
        assert!(!output.contains("unsupported statement"));
    }

    #[test]
    fn public_dart_to_java_route_uses_compiler_v2() {
        let output = crate::translate(
            COMPREHENSIVE_DART,
            crate::Language::Dart,
            crate::Language::Java,
        );
        assert!(output.contains("public static void main(String[] args)"));
        assert!(output.contains("CompletableFuture<Integer> loadScore()"));
        assert!(!output.contains("lowering pending"));
    }

    #[test]
    fn comprehensive_dart_and_generated_java_have_identical_output() {
        use std::{fs, path::Path, process::Command};

        let javac = if Path::new("/opt/homebrew/opt/openjdk/bin/javac").exists() {
            "/opt/homebrew/opt/openjdk/bin/javac"
        } else {
            "javac"
        };
        let java = if Path::new("/opt/homebrew/opt/openjdk/bin/java").exists() {
            "/opt/homebrew/opt/openjdk/bin/java"
        } else {
            "java"
        };
        if Command::new(javac).arg("-version").output().is_err() {
            return;
        }

        let root =
            std::env::temp_dir().join(format!("translatecode-v2-e2e-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        let java_source = JavaBackend
            .emit(&DartFrontend.parse(COMPREHENSIVE_DART))
            .code;
        fs::write(root.join("TranslatedProgram.java"), java_source).unwrap();

        let compile = Command::new(javac)
            .arg("-Xlint:all")
            .arg("TranslatedProgram.java")
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "javac failed:\n{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let generated = Command::new(java)
            .args(["-cp", root.to_str().unwrap(), "TranslatedProgram"])
            .output()
            .unwrap();
        assert!(
            generated.status.success(),
            "java failed:\n{}",
            String::from_utf8_lossy(&generated.stderr)
        );

        let expected = include_str!("../../../tests/fixtures/comprehensive.stdout");
        assert_eq!(String::from_utf8_lossy(&generated.stdout), expected);
        fs::remove_dir_all(root).unwrap();
    }
}
