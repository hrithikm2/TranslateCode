pub mod dart;
pub mod java;
pub mod python;

use crate::diagnostic::{Diagnostic, Severity};
use crate::typed_ir::{
    Body, BodyKind, ClassMember, CollectionElement, Comment, CompilationUnit, Declaration,
    Expression, ExpressionKind, Pattern, PatternKind, Statement, StatementKind,
};
use crate::Language;

#[derive(Clone, Debug, Default)]
pub struct BackendOutput {
    pub code: String,
    pub diagnostics: Vec<Diagnostic>,
}

pub trait Backend {
    fn emit(&self, unit: &CompilationUnit) -> BackendOutput;
}

pub(crate) fn is_python_main_guard(statement: &Statement) -> bool {
    let source = statement.source.replace(' ', "");
    source.starts_with("if__name__==") && source.contains("__main__")
}

/// Returns a warning for every IR node that could not be structurally lowered.
/// Backends call this even when they also emit a placeholder, so API consumers
/// can reliably distinguish complete translations from partial ones.
pub(crate) fn unsupported_diagnostics(unit: &CompilationUnit) -> Vec<Diagnostic> {
    let mut diagnostics = unit.diagnostics.clone();
    for declaration in &unit.declarations {
        match declaration {
            Declaration::Class(value) | Declaration::Mixin(value) => {
                visit_members(&value.members, &mut diagnostics)
            }
            Declaration::Extension(value) => visit_members(&value.members, &mut diagnostics),
            Declaration::Function(value) => {
                if let Some(body) = &value.body {
                    visit_body(body, &mut diagnostics);
                }
            }
            Declaration::Enum(_) | Declaration::TypeAlias(_) => {}
        }
    }
    for statement in &unit.top_level_statements {
        visit_statement(statement, &mut diagnostics);
    }
    diagnostics
}

fn visit_members(members: &[ClassMember], diagnostics: &mut Vec<Diagnostic>) {
    for member in members {
        match member {
            ClassMember::Field(value) => {
                if let Some(body) = &value.initializer {
                    visit_body(body, diagnostics);
                }
            }
            ClassMember::Method(value)
            | ClassMember::Getter(value)
            | ClassMember::Setter(value)
            | ClassMember::Operator(value) => {
                if let Some(body) = &value.body {
                    visit_body(body, diagnostics);
                }
            }
            ClassMember::Constructor(value) => {
                if let Some(body) = &value.body {
                    visit_body(body, diagnostics);
                }
            }
            ClassMember::Unlowered { syntax_kind, span } => diagnostics.push(Diagnostic {
                code: "TC_UNSUPPORTED_MEMBER",
                severity: Severity::Warning,
                message: format!(
                    "Unsupported class member `{syntax_kind}` was preserved as a placeholder"
                ),
                span: *span,
            }),
        }
    }
}

fn visit_body(body: &Body, diagnostics: &mut Vec<Diagnostic>) {
    match &body.kind {
        BodyKind::Block(values) => {
            for statement in values {
                visit_statement(statement, diagnostics);
            }
        }
        BodyKind::Expression(value) => visit_expression(value, diagnostics),
        BodyKind::Unlowered => diagnostics.push(Diagnostic {
            code: "TC_UNSUPPORTED_BODY",
            severity: Severity::Warning,
            message: format!(
                "Unsupported body `{}` was preserved as a placeholder",
                body.syntax_kind
            ),
            span: body.span,
        }),
        BodyKind::Empty => {}
    }
}

fn visit_statement(statement: &Statement, diagnostics: &mut Vec<Diagnostic>) {
    match &statement.kind {
        StatementKind::Block(values) => {
            for value in values {
                visit_statement(value, diagnostics);
            }
        }
        StatementKind::Variable { initializer, .. } => {
            if let Some(value) = initializer {
                visit_expression(value, diagnostics);
            }
        }
        StatementKind::Expression(value)
        | StatementKind::Throw(value)
        | StatementKind::Assert(value) => visit_expression(value, diagnostics),
        StatementKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expression(condition, diagnostics);
            visit_statement(then_branch, diagnostics);
            if let Some(value) = else_branch {
                visit_statement(value, diagnostics);
            }
        }
        StatementKind::ForEach { iterable, body, .. } => {
            visit_expression(iterable, diagnostics);
            visit_statement(body, diagnostics);
        }
        StatementKind::For {
            initializers,
            condition,
            updates,
            body,
        } => {
            for value in initializers {
                visit_statement(value, diagnostics);
            }
            if let Some(value) = condition {
                visit_expression(value, diagnostics);
            }
            for value in updates {
                visit_expression(value, diagnostics);
            }
            visit_statement(body, diagnostics);
        }
        StatementKind::While { condition, body } | StatementKind::DoWhile { condition, body } => {
            visit_expression(condition, diagnostics);
            visit_statement(body, diagnostics);
        }
        StatementKind::Switch { expression, cases } => {
            visit_expression(expression, diagnostics);
            for case in cases {
                visit_pattern(&case.pattern, diagnostics);
                for value in &case.statements {
                    visit_statement(value, diagnostics);
                }
            }
        }
        StatementKind::Try {
            body,
            catches,
            finally_body,
        } => {
            visit_statement(body, diagnostics);
            for value in catches {
                visit_statement(&value.body, diagnostics);
            }
            if let Some(value) = finally_body {
                visit_statement(value, diagnostics);
            }
        }
        StatementKind::Return(value) => {
            if let Some(value) = value {
                visit_expression(value, diagnostics);
            }
        }
        StatementKind::Unlowered { syntax_kind } => diagnostics.push(Diagnostic {
            code: "TC_UNSUPPORTED_STATEMENT",
            severity: Severity::Warning,
            message: format!(
                "Unsupported statement `{syntax_kind}` was preserved as a placeholder"
            ),
            span: statement.span,
        }),
        StatementKind::Break | StatementKind::Continue => {}
    }
}

fn visit_expression(expression: &Expression, diagnostics: &mut Vec<Diagnostic>) {
    match &expression.kind {
        ExpressionKind::Binary { left, right, .. } | ExpressionKind::IfNull { left, right } => {
            visit_expression(left, diagnostics);
            visit_expression(right, diagnostics);
        }
        ExpressionKind::Unary { operand, .. } | ExpressionKind::Await(operand) => {
            visit_expression(operand, diagnostics)
        }
        ExpressionKind::Assignment { target, value, .. } => {
            visit_expression(target, diagnostics);
            visit_expression(value, diagnostics);
        }
        ExpressionKind::Member { object, .. } => visit_expression(object, diagnostics),
        ExpressionKind::Index { object, index, .. } => {
            visit_expression(object, diagnostics);
            visit_expression(index, diagnostics);
        }
        ExpressionKind::Call {
            callee, arguments, ..
        } => {
            visit_expression(callee, diagnostics);
            for value in arguments {
                visit_expression(&value.value, diagnostics);
            }
        }
        ExpressionKind::IntrinsicCall {
            receiver,
            arguments,
            ..
        } => {
            visit_expression(receiver, diagnostics);
            for value in arguments {
                visit_expression(value, diagnostics);
            }
        }
        ExpressionKind::ObjectCreation { arguments, .. } => {
            for value in arguments {
                visit_expression(&value.value, diagnostics);
            }
        }
        ExpressionKind::ListLiteral { elements, .. } => {
            for value in elements {
                match value {
                    CollectionElement::Expression(value)
                    | CollectionElement::Spread {
                        expression: value, ..
                    } => visit_expression(value, diagnostics),
                }
            }
        }
        ExpressionKind::MapLiteral { entries, .. } => {
            for (key, value) in entries {
                visit_expression(key, diagnostics);
                visit_expression(value, diagnostics);
            }
        }
        ExpressionKind::Closure { body, .. } => visit_body(body, diagnostics),
        ExpressionKind::Cast {
            expression: value, ..
        }
        | ExpressionKind::TypeTest {
            expression: value, ..
        } => visit_expression(value, diagnostics),
        ExpressionKind::Cascade { target, sections } => {
            visit_expression(target, diagnostics);
            for value in sections {
                visit_expression(value, diagnostics);
            }
        }
        ExpressionKind::Switch { expression, cases } => {
            visit_expression(expression, diagnostics);
            for case in cases {
                visit_pattern(&case.pattern, diagnostics);
                visit_expression(&case.value, diagnostics);
            }
        }
        ExpressionKind::Raw { syntax_kind } => diagnostics.push(Diagnostic {
            code: "TC_UNSUPPORTED_EXPRESSION",
            severity: Severity::Warning,
            message: format!(
                "Unsupported expression `{syntax_kind}` was preserved as a placeholder"
            ),
            span: expression.span,
        }),
        ExpressionKind::StringInterpolation(parts) => {
            for part in parts {
                if let crate::typed_ir::StringPart::Expression(value) = part {
                    visit_expression(value, diagnostics);
                }
            }
        }
        ExpressionKind::Identifier(_) | ExpressionKind::Literal(_) => {}
    }
}

fn visit_pattern(pattern: &Pattern, diagnostics: &mut Vec<Diagnostic>) {
    match &pattern.kind {
        PatternKind::Constant(value) => visit_expression(value, diagnostics),
        PatternKind::Raw { syntax_kind } => diagnostics.push(Diagnostic {
            code: "TC_UNSUPPORTED_PATTERN",
            severity: Severity::Warning,
            message: format!("Unsupported pattern `{syntax_kind}` was preserved as a placeholder"),
            span: pattern.span,
        }),
        PatternKind::Object { .. }
        | PatternKind::Variable { .. }
        | PatternKind::Wildcard
        | PatternKind::Default => {}
    }
}

pub(crate) fn emit_comments(comments: &[Comment], target: Language) -> String {
    comments
        .iter()
        .map(|comment| emit_comment(&comment.text, target))
        .filter(|comment| !comment.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn emit_comment(source: &str, target: Language) -> String {
    if target == Language::Python {
        python_comment(source)
    } else {
        slash_comment(source)
    }
}

fn python_comment(source: &str) -> String {
    let trimmed = source.trim();
    if trimmed.starts_with("/*") {
        let body = trimmed
            .strip_prefix("/*")
            .unwrap_or(trimmed)
            .strip_suffix("*/")
            .unwrap_or(trimmed);
        return body
            .lines()
            .map(|line| {
                let content = line.trim().trim_start_matches('*').trim_start();
                if content.is_empty() {
                    "#".into()
                } else {
                    format!("# {}", content)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    source
        .lines()
        .map(|line| {
            let content = line
                .trim_start()
                .trim_start_matches('/')
                .trim_start_matches('#')
                .trim_start_matches('!')
                .trim_start();
            if content.is_empty() {
                "#".into()
            } else {
                format!("# {}", content)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn slash_comment(source: &str) -> String {
    let trimmed = source.trim();
    if trimmed.starts_with("//") || trimmed.starts_with("/*") {
        return trimmed.into();
    }
    source
        .lines()
        .map(|line| {
            let content = line.trim_start().trim_start_matches('#').trim_start();
            if content.is_empty() {
                "//".into()
            } else {
                format!("// {}", content)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::SourceSpan;
    use crate::typed_ir::{Expression, ExpressionKind, Statement, StatementKind};

    #[test]
    fn every_typed_backend_reports_and_marks_unlowered_statements() {
        let mut unit = CompilationUnit::default();
        unit.top_level_statements.push(Statement {
            kind: StatementKind::Unlowered {
                syntax_kind: "mystery_statement".into(),
            },
            source: "mystery();".into(),
            span: SourceSpan::default(),
        });

        let outputs = [
            crate::backend::python::PythonBackend.emit(&unit),
            crate::backend::dart::DartBackend.emit(&unit),
            crate::backend::java::JavaBackend.emit(&unit),
        ];
        for output in outputs {
            assert!(
                output
                    .diagnostics
                    .iter()
                    .any(|value| value.code == "TC_UNSUPPORTED_STATEMENT"),
                "{output:#?}"
            );
            assert!(output.code.contains("unsupported statement"), "{output:#?}");
        }
    }

    #[test]
    fn raw_expressions_never_copy_source_language_text() {
        let mut unit = CompilationUnit::default();
        unit.top_level_statements.push(Statement {
            kind: StatementKind::Expression(Expression {
                kind: ExpressionKind::Raw {
                    syntax_kind: "mystery_expression".into(),
                },
                source: "sourceOnlySyntax()".into(),
                span: SourceSpan::default(),
            }),
            source: "sourceOnlySyntax();".into(),
            span: SourceSpan::default(),
        });

        let outputs = [
            crate::backend::python::PythonBackend.emit(&unit),
            crate::backend::dart::DartBackend.emit(&unit),
            crate::backend::java::JavaBackend.emit(&unit),
        ];
        for output in outputs {
            assert!(!output.code.contains("sourceOnlySyntax"), "{output:#?}");
            assert!(
                output
                    .diagnostics
                    .iter()
                    .any(|value| value.code == "TC_UNSUPPORTED_EXPRESSION"),
                "{output:#?}"
            );
        }
    }
}
