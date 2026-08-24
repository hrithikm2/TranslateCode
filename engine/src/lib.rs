use std::mem;
use std::sync::Mutex;

pub mod backend;
pub mod diagnostic;
pub mod frontend;
pub mod semantic;
pub mod typed_ir;

static OUTPUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    JavaScript,
    Java,
    Dart,
    Swift,
    Python,
    Go,
    Rust,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LanguageProfile {
    pub language: Language,
    pub version: &'static str,
    pub edition: Option<&'static str>,
    pub preview_features: bool,
}

impl Language {
    pub fn from_id(id: u32) -> Self {
        match id {
            1 => Self::Java,
            2 => Self::Dart,
            3 => Self::Swift,
            4 => Self::Python,
            5 => Self::Go,
            6 => Self::Rust,
            _ => Self::JavaScript,
        }
    }

    pub const fn profile(self) -> LanguageProfile {
        match self {
            Self::JavaScript => LanguageProfile {
                language: self,
                version: "ECMAScript 2026",
                edition: None,
                preview_features: false,
            },
            Self::Java => LanguageProfile {
                language: self,
                version: "Java SE 26",
                edition: None,
                preview_features: false,
            },
            Self::Dart => LanguageProfile {
                language: self,
                version: "Dart 3.12",
                edition: None,
                preview_features: false,
            },
            Self::Swift => LanguageProfile {
                language: self,
                version: "Swift 6.3",
                edition: None,
                preview_features: false,
            },
            Self::Python => LanguageProfile {
                language: self,
                version: "Python 3.14",
                edition: None,
                preview_features: false,
            },
            Self::Go => LanguageProfile {
                language: self,
                version: "Go 1.26",
                edition: None,
                preview_features: false,
            },
            Self::Rust => LanguageProfile {
                language: self,
                version: "Rust 1.92",
                edition: Some("2024"),
                preview_features: false,
            },
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Program {
    comments: Vec<crate::typed_ir::Comment>,
    body: Vec<Statement>,
    preserve_classes: bool,
}

#[derive(Clone, Debug)]
enum Statement {
    Comment(String),
    Class {
        name: String,
        body: Vec<Statement>,
    },
    Variable {
        name: String,
        value: String,
        mutable: bool,
        type_hint: Option<String>,
    },
    Function {
        name: String,
        params: Vec<Parameter>,
        return_type: Option<String>,
        body: Vec<Statement>,
    },
    If {
        condition: String,
        then_body: Vec<Statement>,
        else_body: Vec<Statement>,
    },
    For {
        initializer: Option<Box<Statement>>,
        condition: String,
        update: String,
        body: Vec<Statement>,
    },
    ForEach {
        variable: String,
        type_hint: Option<String>,
        iterable: String,
        body: Vec<Statement>,
    },
    While {
        condition: String,
        body: Vec<Statement>,
    },
    Break,
    Continue,
    Print {
        values: Vec<String>,
    },
    Return(Option<String>),
    Expression(String),
}

#[derive(Clone, Debug)]
struct Parameter {
    name: String,
    type_hint: Option<String>,
}

fn trim_end_tokens(value: &str) -> String {
    value
        .trim()
        .trim_end_matches(';')
        .trim_end_matches('{')
        .trim()
        .to_string()
}

fn strip_wrapping_parens(value: &str) -> String {
    let text = value.trim();
    if text.starts_with('(') && text.ends_with(')') {
        text[1..text.len() - 1].trim().to_string()
    } else {
        text.to_string()
    }
}

fn split_args(value: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut quote = '\0';
    for ch in value.chars() {
        if quote != '\0' {
            current.push(ch);
            if ch == quote {
                quote = '\0';
            }
        } else {
            match ch {
                '\'' | '"' => {
                    quote = ch;
                    current.push(ch);
                }
                '(' | '[' | '{' => {
                    depth += 1;
                    current.push(ch);
                }
                ')' | ']' | '}' => {
                    depth -= 1;
                    current.push(ch);
                }
                ',' if depth == 0 => {
                    result.push(current.trim().to_string());
                    current.clear();
                }
                _ => current.push(ch),
            }
        }
    }
    if !current.trim().is_empty() {
        result.push(current.trim().to_string());
    }
    result
}

fn canonical_type(value: &str) -> Option<String> {
    crate::semantic::SemanticType::parse(value).map(|value| value.canonical())
}

fn infer_type(expression: &str) -> String {
    crate::semantic::SemanticType::infer(expression).canonical()
}

fn source_expression(expression: &str, language: Language) -> String {
    let mut value = expression.trim().to_string();
    if language == Language::Java {
        if let Some(receiver) = value
            .strip_prefix('!')
            .and_then(|value| value.strip_suffix(".isEmpty()"))
        {
            value = format!("{} != \"\"", receiver);
        } else if let Some(receiver) = value.strip_suffix(".isEmpty()") {
            value = format!("{} == \"\"", receiver);
        }
    }
    if language == Language::Dart {
        if let Some(receiver) = value.strip_suffix(".isNotEmpty") {
            value = format!("{} != \"\"", receiver);
        } else if let Some(receiver) = value.strip_suffix(".isEmpty") {
            value = format!("{} == \"\"", receiver);
        }
    }
    if language == Language::Rust {
        value = value
            .replace(".to_string()", "")
            .replace(" + &", " + ")
            .replace("vec![", "[");
    }
    if language == Language::Swift {
        value = value.replace("nil", "null");
    }
    if language == Language::JavaScript {
        value = value.replace("!==", "!=").replace("===", "==");
    }
    if language == Language::Java && value.starts_with("List.of(") && value.ends_with(')') {
        value = format!("[{}]", &value[8..value.len() - 1]);
    }
    if language == Language::Go && value.starts_with("[]") {
        if let Some(open) = value.find('{') {
            if value.ends_with('}') {
                value = format!("[{}]", &value[open + 1..value.len() - 1]);
            }
        }
    }
    value
}

fn split_for_header(header: &str) -> Option<(String, String, String)> {
    let inner = header.trim().strip_prefix("for")?.trim();
    let inner = inner.strip_prefix('(')?.strip_suffix(')')?;
    let parts = split_args_with_separator(inner, ';');
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].trim().into(),
        parts[1].trim().into(),
        parts[2].trim().into(),
    ))
}

fn parse_for_each_header(
    header: &str,
    language: Language,
) -> Option<(String, Option<String>, String)> {
    let text = header
        .trim()
        .trim_end_matches('{')
        .trim()
        .trim_end_matches(':')
        .trim();
    let inner = match language {
        Language::Python => text.strip_prefix("for ")?,
        Language::Swift | Language::Rust => text.strip_prefix("for ")?,
        Language::Go => text.strip_prefix("for ")?,
        _ => text
            .strip_prefix("for")?
            .trim()
            .strip_prefix('(')?
            .strip_suffix(')')?,
    };
    match language {
        Language::Python | Language::Swift | Language::Rust => {
            let (variable, iterable) = inner.split_once(" in ")?;
            Some((
                variable.trim().trim_start_matches('&').to_string(),
                None,
                source_expression(iterable, language),
            ))
        }
        Language::JavaScript => {
            let (left, iterable) = inner.split_once(" of ")?;
            let variable = left
                .trim()
                .trim_start_matches("const ")
                .trim_start_matches("let ")
                .trim();
            Some((
                variable.to_string(),
                None,
                source_expression(iterable, language),
            ))
        }
        Language::Java | Language::Dart => {
            let separator = if inner.contains(" in ") {
                " in "
            } else {
                " : "
            };
            let (left, iterable) = inner.split_once(separator)?;
            let left = left
                .trim()
                .trim_start_matches("final ")
                .trim_start_matches("var ")
                .trim();
            let pieces = left.split_whitespace().collect::<Vec<_>>();
            let variable = pieces.last()?.to_string();
            let type_hint = pieces
                .iter()
                .rev()
                .nth(1)
                .and_then(|value| canonical_type(value));
            Some((variable, type_hint, source_expression(iterable, language)))
        }
        Language::Go => {
            let (left, iterable) = inner.split_once(" range ")?;
            let left = left.trim().trim_end_matches(":=").trim();
            let variable = left.split(',').next_back()?.trim();
            Some((
                variable.to_string(),
                None,
                source_expression(iterable, language),
            ))
        }
    }
}

fn replace_dart_length(value: &str) -> String {
    let mut result = String::new();
    let mut rest = value;
    while let Some(index) = rest.find(".length") {
        let (before, after) = rest.split_at(index);
        let mut start = before.len();
        while start > 0 {
            let ch = before[..start].chars().last().unwrap();
            if ch.is_alphanumeric() || ch == '_' {
                start -= ch.len_utf8();
            } else {
                break;
            }
        }
        result.push_str(&before[..start]);
        result.push_str("len(");
        result.push_str(&before[start..]);
        result.push(')');
        rest = &after[".length".len()..];
    }
    result.push_str(rest);
    result
}

fn split_args_with_separator(value: &str, separator: char) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut quote = '\0';
    for ch in value.chars() {
        if quote != '\0' {
            current.push(ch);
            if ch == quote {
                quote = '\0';
            }
        } else {
            match ch {
                '\'' | '"' => {
                    quote = ch;
                    current.push(ch);
                }
                '(' | '[' | '{' => {
                    depth += 1;
                    current.push(ch);
                }
                ')' | ']' | '}' => {
                    depth -= 1;
                    current.push(ch);
                }
                value if value == separator && depth == 0 => {
                    result.push(current.trim().to_string());
                    current.clear();
                }
                _ => current.push(ch),
            }
        }
    }
    result.push(current.trim().to_string());
    result
}

fn parse_parameter(raw: &str, language: Language) -> Parameter {
    let text = raw.trim().trim_start_matches('_').trim();
    if text.is_empty() {
        return Parameter {
            name: String::new(),
            type_hint: None,
        };
    }
    match language {
        Language::Java | Language::Dart => {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() >= 2 {
                Parameter {
                    name: parts.last().unwrap().trim().to_string(),
                    type_hint: canonical_type(parts[parts.len() - 2]),
                }
            } else {
                Parameter {
                    name: text.to_string(),
                    type_hint: None,
                }
            }
        }
        Language::Swift | Language::Rust => {
            if let Some((name, ty)) = text.split_once(':') {
                Parameter {
                    name: name.trim().to_string(),
                    type_hint: canonical_type(ty),
                }
            } else {
                Parameter {
                    name: text.to_string(),
                    type_hint: None,
                }
            }
        }
        Language::Go => {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() >= 2 {
                Parameter {
                    name: parts[0].to_string(),
                    type_hint: canonical_type(parts[1]),
                }
            } else {
                Parameter {
                    name: text.to_string(),
                    type_hint: None,
                }
            }
        }
        Language::Python => {
            if let Some((name, ty)) = text.split_once(':') {
                Parameter {
                    name: name.trim().to_string(),
                    type_hint: canonical_type(ty),
                }
            } else {
                Parameter {
                    name: text.to_string(),
                    type_hint: None,
                }
            }
        }
        Language::JavaScript => Parameter {
            name: text.to_string(),
            type_hint: None,
        },
    }
}

fn parse_function_header(
    text: &str,
    language: Language,
) -> Option<(String, Vec<Parameter>, Option<String>)> {
    let line = trim_end_tokens(text);
    let open = line.find('(')?;
    let close = line.rfind(')')?;
    if close < open {
        return None;
    }
    let before = line[..open].trim();
    let args = &line[open + 1..close];
    let after = line[close + 1..].trim();
    let name = match language {
        Language::Python => before.strip_prefix("def ")?.trim(),
        Language::JavaScript => before.strip_prefix("function ")?.trim(),
        Language::Swift => before.strip_prefix("func ")?.trim(),
        Language::Go => before.strip_prefix("func ")?.trim(),
        Language::Rust => before
            .strip_prefix("pub ")
            .unwrap_or(before)
            .strip_prefix("fn ")?
            .trim(),
        Language::Java | Language::Dart => {
            if before.starts_with("if ")
                || before == "if"
                || before.starts_with("for ")
                || before.starts_with("while ")
            {
                return None;
            }
            before.split_whitespace().last()?
        }
    };
    if name.is_empty() || ["if", "for", "while", "switch", "print", "println"].contains(&name) {
        return None;
    }
    let params = split_args(args)
        .iter()
        .map(|arg| parse_parameter(arg, language))
        .filter(|p| !p.name.is_empty())
        .collect();
    let return_type = match language {
        Language::Swift | Language::Rust | Language::Python => after
            .split_once("->")
            .and_then(|(_, ty)| canonical_type(ty.trim_end_matches(':'))),
        Language::Go => canonical_type(after),
        Language::Java | Language::Dart => before
            .split_whitespace()
            .rev()
            .nth(1)
            .and_then(canonical_type),
        _ => None,
    };
    Some((name.to_string(), params, return_type))
}

fn parse_print(text: &str, language: Language) -> Option<Vec<String>> {
    let names: &[&str] = match language {
        Language::JavaScript => &["console.log"],
        Language::Java => &["System.out.println", "System.out.print"],
        Language::Dart | Language::Swift | Language::Python => &["print"],
        Language::Go => &["fmt.Println", "fmt.Print"],
        Language::Rust => &["println!", "print!"],
    };
    for name in names {
        let prefix = format!("{}(", name);
        if text.trim().starts_with(&prefix) {
            let call = text.trim().trim_end_matches(';').trim();
            let inner = call[prefix.len()..].strip_suffix(')')?.trim();
            let mut values = split_args(inner);
            if language == Language::Rust
                && values
                    .first()
                    .map(|v| v.contains("{}") || v.contains("{:?"))
                    .unwrap_or(false)
            {
                values.remove(0);
            }
            return Some(
                values
                    .into_iter()
                    .map(|value| source_expression(&value, language))
                    .collect(),
            );
        }
    }
    None
}

fn parse_variable(text: &str, language: Language) -> Option<Statement> {
    let line = text.trim().trim_end_matches(';');
    if ["+=", "-=", "*=", "/=", "%="]
        .iter()
        .any(|operator| line.contains(operator))
    {
        return None;
    }
    let (left, value) = line.split_once('=')?;
    if ["==", ">=", "<=", "!="].iter().any(|op| line.contains(op)) {
        return None;
    }
    let left = left.trim();
    let value = source_expression(value, language);
    let mut mutable = true;
    let (name, type_hint) = match language {
        Language::Python => {
            if let Some((name, ty)) = left.split_once(':') {
                (name.trim().to_string(), canonical_type(ty))
            } else {
                if left.contains(' ') {
                    return None;
                }
                (left.to_string(), None)
            }
        }
        Language::JavaScript => {
            let declaration = ["const ", "let ", "var "]
                .iter()
                .find(|prefix| left.starts_with(**prefix))?;
            mutable = *declaration != "const ";
            (
                left.trim_start_matches(*declaration).trim().to_string(),
                None,
            )
        }
        Language::Rust => {
            let rest = left.strip_prefix("let ")?;
            mutable = rest.starts_with("mut ");
            let rest = rest.trim_start_matches("mut ").trim();
            if let Some((name, ty)) = rest.split_once(':') {
                (name.trim().to_string(), canonical_type(ty))
            } else {
                (rest.to_string(), None)
            }
        }
        Language::Go => {
            if left.ends_with(':') {
                (left.trim_end_matches(':').trim().to_string(), None)
            } else if let Some(rest) = left.strip_prefix("var ") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                (
                    parts.first()?.to_string(),
                    parts.get(1).and_then(|ty| canonical_type(ty)),
                )
            } else {
                return None;
            }
        }
        Language::Swift => {
            let (rest, is_mut) = if let Some(v) = left.strip_prefix("var ") {
                (v, true)
            } else if let Some(v) = left.strip_prefix("let ") {
                (v, false)
            } else {
                return None;
            };
            mutable = is_mut;
            if let Some((name, ty)) = rest.split_once(':') {
                (name.trim().to_string(), canonical_type(ty))
            } else {
                (rest.trim().to_string(), None)
            }
        }
        Language::Dart => {
            let mut rest = left;
            if let Some(v) = rest.strip_prefix("final ") {
                mutable = false;
                rest = v;
            } else if let Some(v) = rest.strip_prefix("const ") {
                mutable = false;
                rest = v;
            } else if let Some(v) = rest.strip_prefix("var ") {
                rest = v;
            }
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 2 {
                (
                    parts.last()?.to_string(),
                    canonical_type(parts[parts.len() - 2]),
                )
            } else {
                (rest.to_string(), None)
            }
        }
        Language::Java => {
            let mut rest = left;
            if let Some(v) = rest.strip_prefix("final ") {
                mutable = false;
                rest = v;
            }
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 2 {
                return None;
            }
            (
                parts.last()?.to_string(),
                canonical_type(parts[parts.len() - 2]),
            )
        }
    };
    Some(Statement::Variable {
        name,
        value,
        mutable,
        type_hint,
    })
}

fn parse_simple_statement(text: &str, language: Language) -> Option<Statement> {
    let line = text.trim().trim_end_matches(';').trim();
    if line.is_empty()
        || line.starts_with("import ")
        || line.starts_with("package ")
        || line.starts_with("use ")
    {
        return None;
    }
    if line.starts_with("//") || line.starts_with('#') {
        return Some(Statement::Comment(line.to_string()));
    }
    if let Some(values) = parse_print(line, language) {
        return Some(Statement::Print { values });
    }
    if line == "return" {
        return Some(Statement::Return(None));
    }
    if line == "break" {
        return Some(Statement::Break);
    }
    if line == "continue" {
        return Some(Statement::Continue);
    }
    if ["yield ", "raise ", "await ", "del ", "global ", "nonlocal "]
        .iter()
        .any(|prefix| line.starts_with(prefix))
    {
        return Some(unsupported_statement(line, language));
    }
    if let Some(value) = line.strip_prefix("return ") {
        return Some(Statement::Return(Some(source_expression(value, language))));
    }
    if let Some(variable) = parse_variable(line, language) {
        return Some(variable);
    }
    Some(Statement::Expression(source_expression(line, language)))
}

fn parse_python_tuple_assignment(text: &str) -> Option<Vec<Statement>> {
    let line = text.trim().trim_end_matches(';');
    let (left, right) = line.split_once('=')?;
    let names = split_args(left);
    let values = split_args(right);
    if names.len() < 2 || names.len() != values.len() {
        return None;
    }
    if names.iter().any(|name| {
        name.trim().is_empty()
            || !name
                .trim()
                .chars()
                .all(|character| character == '_' || character.is_alphanumeric())
    }) {
        return None;
    }
    Some(
        names
            .into_iter()
            .zip(values)
            .map(|(name, value)| {
                parse_variable(
                    &format!("{} = {}", name.trim(), value.trim()),
                    Language::Python,
                )
                .expect("validated Python tuple assignment")
            })
            .collect(),
    )
}

fn unsupported_statement(source: &str, language: Language) -> Statement {
    let message = format!(
        "Translation stopped at unsupported construct: {}",
        source.trim()
    );
    Statement::Comment(if language == Language::Python {
        format!("# {}", message)
    } else {
        format!("// {}", message)
    })
}

fn indent_of(line: &str) -> usize {
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

fn python_child_indent(lines: &[String], index: usize, parent: usize) -> usize {
    lines
        .iter()
        .skip(index)
        .find(|line| !line.trim().is_empty())
        .map(|line| indent_of(line))
        .filter(|value| *value > parent)
        .unwrap_or(parent + 4)
}

fn parse_python_block(lines: &[String], index: &mut usize, indent: usize) -> Vec<Statement> {
    let mut body = Vec::new();
    while *index < lines.len() {
        let raw = &lines[*index];
        if raw.trim().is_empty() {
            *index += 1;
            continue;
        }
        let current_indent = indent_of(raw);
        if current_indent < indent {
            break;
        }
        if current_indent > indent {
            *index += 1;
            continue;
        }
        let text = raw.trim();
        if text.starts_with("else:") {
            break;
        }
        if text.ends_with(':')
            && [
                "try:", "except", "finally:", "with ", "match ", "case ", "async ",
            ]
            .iter()
            .any(|prefix| text.starts_with(prefix))
        {
            body.push(unsupported_statement(text, Language::Python));
            *index += 1;
            let child_indent = python_child_indent(lines, *index, indent);
            let _ = parse_python_block(lines, index, child_indent);
            continue;
        }
        if text.starts_with("if __name__") && text.ends_with(':') {
            *index += 1;
            let child_indent = python_child_indent(lines, *index, indent);
            body.extend(parse_python_block(lines, index, child_indent));
            continue;
        }
        if let Some(name) = text
            .strip_prefix("class ")
            .and_then(|value| value.strip_suffix(':'))
            .map(|value| value.split(['(', ':']).next().unwrap_or(value).trim())
        {
            *index += 1;
            let child_indent = python_child_indent(lines, *index, indent);
            let class_body = parse_python_block(lines, index, child_indent);
            body.push(Statement::Class {
                name: name.to_string(),
                body: class_body,
            });
            continue;
        }
        if let Some((name, params, return_type)) = parse_function_header(text, Language::Python) {
            *index += 1;
            let child_indent = python_child_indent(lines, *index, indent);
            let function_body = parse_python_block(lines, index, child_indent);
            body.push(Statement::Function {
                name,
                params,
                return_type,
                body: function_body,
            });
            continue;
        }
        if text.starts_with("for ") && text.ends_with(':') {
            if let Some((variable, type_hint, iterable)) =
                parse_for_each_header(text, Language::Python)
            {
                *index += 1;
                let child_indent = python_child_indent(lines, *index, indent);
                let loop_body = parse_python_block(lines, index, child_indent);
                body.push(Statement::ForEach {
                    variable,
                    type_hint,
                    iterable,
                    body: loop_body,
                });
                continue;
            }
        }
        if text.starts_with("while ") && text.ends_with(':') {
            let condition = source_expression(text[6..text.len() - 1].trim(), Language::Python);
            *index += 1;
            let child_indent = python_child_indent(lines, *index, indent);
            let loop_body = parse_python_block(lines, index, child_indent);
            body.push(Statement::While {
                condition,
                body: loop_body,
            });
            continue;
        }
        if text.starts_with("if ") && text.ends_with(':') {
            let condition = source_expression(text[3..text.len() - 1].trim(), Language::Python);
            *index += 1;
            let child_indent = python_child_indent(lines, *index, indent);
            let then_body = parse_python_block(lines, index, child_indent);
            let mut else_body = Vec::new();
            if *index < lines.len()
                && indent_of(&lines[*index]) == indent
                && lines[*index].trim().starts_with("elif ")
            {
                let elif = lines[*index].trim();
                let condition =
                    source_expression(elif[5..].trim_end_matches(':').trim(), Language::Python);
                *index += 1;
                let elif_indent = python_child_indent(lines, *index, indent);
                let elif_body = parse_python_block(lines, index, elif_indent);
                let mut nested_else_body = Vec::new();
                if *index < lines.len()
                    && indent_of(&lines[*index]) == indent
                    && lines[*index].trim().starts_with("else:")
                {
                    *index += 1;
                    let else_indent = python_child_indent(lines, *index, indent);
                    nested_else_body = parse_python_block(lines, index, else_indent);
                }
                else_body.push(Statement::If {
                    condition,
                    then_body: elif_body,
                    else_body: nested_else_body,
                });
            } else if *index < lines.len()
                && indent_of(&lines[*index]) == indent
                && lines[*index].trim().starts_with("else:")
            {
                *index += 1;
                let else_indent = python_child_indent(lines, *index, indent);
                else_body = parse_python_block(lines, index, else_indent);
            }
            body.push(Statement::If {
                condition,
                then_body,
                else_body,
            });
            continue;
        }
        if let Some(statements) = parse_python_tuple_assignment(text) {
            body.extend(statements);
            *index += 1;
            continue;
        }
        if let Some(statement) = parse_simple_statement(text, Language::Python) {
            body.push(statement);
        }
        *index += 1;
    }
    body
}

fn normalize_braces(source: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut quote = '\0';
    let mut escaped = false;
    let mut paren_depth = 0i32;
    let mut map_literal_depth = 0i32;
    let mut line_comment = false;
    for ch in source.chars() {
        if line_comment {
            current.push(ch);
            if ch == '\n' || ch == '\r' {
                line_comment = false;
                if !current.trim().is_empty() {
                    lines.push(current.trim().to_string());
                }
                current.clear();
            }
            continue;
        }
        if quote != '\0' {
            current.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                quote = '\0';
            }
            continue;
        }
        match ch {
            '\'' | '"' => {
                quote = ch;
                current.push(ch);
            }
            '(' => {
                paren_depth += 1;
                current.push(ch);
            }
            ')' => {
                paren_depth -= 1;
                current.push(ch);
            }
            '{' => {
                let prefix = current.trim_end();
                if prefix.ends_with('=')
                    || (prefix.contains(":=")
                        && prefix
                            .rsplit_once(":=")
                            .is_some_and(|(_, value)| value.trim_start().starts_with("[]")))
                {
                    map_literal_depth += 1;
                    current.push('{');
                    continue;
                }
                current.push('{');
                if !current.trim().is_empty() {
                    lines.push(current.trim().to_string());
                }
                current.clear();
            }
            '}' => {
                if map_literal_depth > 0 {
                    map_literal_depth -= 1;
                    current.push('}');
                    continue;
                }
                if !current.trim().is_empty() {
                    lines.push(current.trim().to_string());
                }
                lines.push("}".to_string());
                current.clear();
            }
            ';' => {
                current.push(';');
                if paren_depth == 0 && !current.trim().is_empty() {
                    lines.push(current.trim().to_string());
                }
                if paren_depth == 0 {
                    current.clear();
                }
            }
            '#' if current.trim().is_empty() => {
                line_comment = true;
                current.push(ch);
            }
            '\n' | '\r' => {
                if !current.trim().is_empty() {
                    lines.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => {
                if ch == '/' && current.ends_with('/') {
                    line_comment = true;
                }
                current.push(ch);
            }
        }
    }
    if !current.trim().is_empty() {
        lines.push(current.trim().to_string());
    }
    lines
}

fn extract_condition(text: &str) -> String {
    let line = trim_end_tokens(text);
    let after_if = line.strip_prefix("if").unwrap_or(&line).trim();
    strip_wrapping_parens(after_if)
}

fn parse_brace_block(
    lines: &[String],
    index: &mut usize,
    language: Language,
    stop_at_close: bool,
) -> Vec<Statement> {
    let mut body = Vec::new();
    while *index < lines.len() {
        let text = lines[*index].trim();
        if text.starts_with('}') {
            *index += 1;
            if stop_at_close {
                break;
            }
            continue;
        }
        if text == "{" {
            *index += 1;
            continue;
        }
        if text.ends_with('{') && text.starts_with("for") {
            if let Some((variable, type_hint, iterable)) = parse_for_each_header(text, language) {
                *index += 1;
                let loop_body = parse_brace_block(lines, index, language, true);
                body.push(Statement::ForEach {
                    variable,
                    type_hint,
                    iterable,
                    body: loop_body,
                });
                continue;
            }
        }
        if text.ends_with('{') && text.starts_with("for") {
            if let Some((initializer, condition, update)) =
                split_for_header(text.trim_end_matches('{').trim())
            {
                *index += 1;
                let initializer = parse_simple_statement(&initializer, language).map(Box::new);
                let loop_body = parse_brace_block(lines, index, language, true);
                body.push(Statement::For {
                    initializer,
                    condition,
                    update,
                    body: loop_body,
                });
                continue;
            }
        }
        if text.ends_with('{') && (text.starts_with("while ") || text.starts_with("while(")) {
            let condition = source_expression(
                &strip_wrapping_parens(trim_end_tokens(text).trim_start_matches("while").trim()),
                language,
            );
            *index += 1;
            let loop_body = parse_brace_block(lines, index, language, true);
            body.push(Statement::While {
                condition,
                body: loop_body,
            });
            continue;
        }
        if let Some((name, params, return_type)) =
            parse_function_header(text, language).filter(|_| text.contains('{'))
        {
            *index += 1;
            let function_body = parse_brace_block(lines, index, language, true);
            body.push(Statement::Function {
                name,
                params,
                return_type,
                body: function_body,
            });
            continue;
        }
        if (text.starts_with("if ") || text.starts_with("if(")) && text.contains('{') {
            let condition = source_expression(&extract_condition(text), language);
            *index += 1;
            let then_body = parse_brace_block(lines, index, language, true);
            let mut else_body = Vec::new();
            if *index < lines.len() && lines[*index].trim().starts_with("else") {
                *index += 1;
                else_body = parse_brace_block(lines, index, language, true);
            }
            body.push(Statement::If {
                condition,
                then_body,
                else_body,
            });
            continue;
        }
        if text.starts_with("else") {
            if stop_at_close {
                break;
            }
            *index += 1;
            continue;
        }
        if text.ends_with('{')
            && (text.starts_with("class ")
                || text.contains(" class ")
                || text.starts_with("public class"))
        {
            let name = text
                .trim_end_matches('{')
                .split_whitespace()
                .last()
                .unwrap_or("TranslatedClass")
                .to_string();
            *index += 1;
            let class_body = parse_brace_block(lines, index, language, true);
            body.push(Statement::Class {
                name,
                body: class_body,
            });
            continue;
        }
        if text.ends_with('{') {
            body.push(unsupported_statement(text, language));
            *index += 1;
            let _ = parse_brace_block(lines, index, language, true);
            continue;
        }
        if let Some(statement) = parse_simple_statement(text, language) {
            body.push(statement);
        }
        *index += 1;
    }
    body
}

fn parse(source: &str, language: Language, preserve_classes: bool) -> Program {
    let comments = crate::frontend::parse_source(source, language).comments;
    let stripped_source = strip_source_comments(source, &comments);
    let source = stripped_source.as_str();
    let mut program = if language == Language::Python {
        let lines = source
            .lines()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        let mut index = 0;
        Program {
            comments: comments.clone(),
            body: parse_python_block(&lines, &mut index, 0),
            preserve_classes,
        }
    } else {
        let lines = normalize_braces(source);
        let mut index = 0;
        Program {
            comments,
            body: parse_brace_block(&lines, &mut index, language, false),
            preserve_classes,
        }
    };
    if language != Language::Python && !preserve_classes {
        let mut flattened = Vec::new();
        for statement in program.body {
            if let Statement::Class { body, .. } = statement {
                flattened.extend(body);
            } else {
                flattened.push(statement);
            }
        }
        program.body = flattened;
    }
    if matches!(
        language,
        Language::Java | Language::Dart | Language::Go | Language::Rust
    ) {
        let mut normalized = Vec::new();
        for statement in program.body {
            match statement {
                Statement::Function { name, body, .. } if name == "main" => normalized.extend(body),
                other => normalized.push(other),
            }
        }
        program.body = normalized;
    }
    normalize_scope_mutability(&mut program.body);
    program
}

fn strip_source_comments(source: &str, comments: &[crate::typed_ir::Comment]) -> String {
    let mut bytes = source.as_bytes().to_vec();
    for comment in comments {
        let end = comment.span.end.byte.min(bytes.len());
        for byte in &mut bytes[comment.span.start.byte.min(end)..end] {
            if !matches!(*byte, b'\n' | b'\r') {
                *byte = b' ';
            }
        }
    }
    String::from_utf8(bytes).expect("comment stripping preserves UTF-8")
}

fn normalize_scope_mutability(body: &mut [Statement]) {
    use std::collections::HashSet;

    fn normalize_body(body: &mut [Statement], outer: &HashSet<String>) {
        let mut visible = outer.clone();
        let mut declarations = HashSet::new();
        for statement in body.iter_mut() {
            if let Statement::Variable { name, value, .. } = statement {
                if visible.contains(name) || !declarations.insert(name.clone()) {
                    *statement = Statement::Expression(format!("{} = {}", name, value));
                } else {
                    visible.insert(name.clone());
                }
            }
        }

        for statement in body.iter_mut() {
            match statement {
                Statement::Class { body, .. } | Statement::Function { body, .. } => {
                    normalize_body(body, &HashSet::new())
                }
                Statement::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    normalize_body(then_body, &visible);
                    normalize_body(else_body, &visible);
                }
                Statement::For { body, .. }
                | Statement::ForEach { body, .. }
                | Statement::While { body, .. } => normalize_body(body, &visible),
                _ => {}
            }
        }

        fn collect_assignments(body: &[Statement], names: &mut HashSet<String>) {
            for statement in body {
                match statement {
                    Statement::Expression(value) => {
                        let trimmed = value.trim();
                        for operator in ["+=", "-=", "*=", "/=", "="] {
                            if let Some((left, _)) = trimmed.split_once(operator) {
                                let candidate = left.trim();
                                if candidate
                                    .chars()
                                    .all(|value| value == '_' || value.is_alphanumeric())
                                {
                                    names.insert(candidate.to_string());
                                }
                                break;
                            }
                        }
                        for suffix in ["++", "--"] {
                            if let Some(candidate) = trimmed.strip_suffix(suffix) {
                                names.insert(candidate.trim().to_string());
                            }
                        }
                    }
                    Statement::If {
                        then_body,
                        else_body,
                        ..
                    } => {
                        collect_assignments(then_body, names);
                        collect_assignments(else_body, names);
                    }
                    Statement::For { body, .. }
                    | Statement::ForEach { body, .. }
                    | Statement::While { body, .. } => collect_assignments(body, names),
                    _ => {}
                }
            }
        }

        let mut assignments = HashSet::new();
        collect_assignments(body, &mut assignments);
        for statement in body {
            if let Statement::Variable { name, mutable, .. } = statement {
                *mutable = assignments.contains(name);
            }
        }
    }

    normalize_body(body, &HashSet::new());
}

fn type_for(target: Language, canonical: &str) -> String {
    if let Some(open) = canonical.find('<') {
        let close = canonical.rfind('>').unwrap_or(canonical.len());
        let container = &canonical[..open];
        let arguments = split_args(&canonical[open + 1..close]);
        let values = arguments
            .iter()
            .map(|argument| {
                if target == Language::Java {
                    type_for_java_boxed(argument)
                } else {
                    type_for(target, argument)
                }
            })
            .collect::<Vec<_>>();
        return match (target, container) {
            (Language::Python, "list") => format!("list[{}]", values.join(", ")),
            (Language::Python, "set") => format!("set[{}]", values.join(", ")),
            (Language::Python, "map") => format!("dict[{}]", values.join(", ")),
            (Language::Java, "list") => format!("List<{}>", values.join(", ")),
            (Language::Java, "set") => format!("Set<{}>", values.join(", ")),
            (Language::Java, "map") => format!("Map<{}>", values.join(", ")),
            (Language::Dart, "list") => format!("List<{}>", values.join(", ")),
            (Language::Dart, "set") => format!("Set<{}>", values.join(", ")),
            (Language::Dart, "map") => format!("Map<{}>", values.join(", ")),
            (Language::Swift, "list") => format!("[{}]", values.join(", ")),
            (Language::Swift, "set") => format!("Set<{}>", values.join(", ")),
            (Language::Swift, "map") => format!("[{}]", values.join(": ")),
            (Language::Go, "list") => format!("[]{}", values.join("")),
            (Language::Go, "set") => format!("map[{}]struct{{}}", values.join("")),
            (Language::Go, "map") => format!(
                "map[{}]{}",
                values.first().cloned().unwrap_or_else(|| "any".into()),
                values.get(1).cloned().unwrap_or_else(|| "any".into())
            ),
            (Language::Rust, "list") => format!("Vec<{}>", values.join(", ")),
            (Language::Rust, "set") => format!("HashSet<{}>", values.join(", ")),
            (Language::Rust, "map") => format!("HashMap<{}>", values.join(", ")),
            _ => "object".into(),
        };
    }
    match target {
        Language::JavaScript => "".into(),
        Language::Python => match canonical {
            "string" => "str",
            "int" => "int",
            "float" => "float",
            "bool" => "bool",
            "void" => "None",
            _ => "object",
        }
        .into(),
        Language::Java => match canonical {
            "string" => "String",
            "int" => "int",
            "float" => "double",
            "bool" => "boolean",
            "void" => "void",
            _ => "Object",
        }
        .into(),
        Language::Dart => match canonical {
            "string" => "String",
            "int" => "int",
            "float" => "double",
            "bool" => "bool",
            "void" => "void",
            _ => "dynamic",
        }
        .into(),
        Language::Swift => match canonical {
            "string" => "String",
            "int" => "Int",
            "float" => "Double",
            "bool" => "Bool",
            "void" => "Void",
            _ => "Any",
        }
        .into(),
        Language::Go => match canonical {
            "string" => "string",
            "int" => "int",
            "float" => "float64",
            "bool" => "bool",
            "void" => "",
            _ => "any",
        }
        .into(),
        Language::Rust => match canonical {
            "string" => "String",
            "int" => "i64",
            "float" => "f64",
            "bool" => "bool",
            "void" => "()",
            _ => "String",
        }
        .into(),
    }
}

fn type_for_java_boxed(canonical: &str) -> String {
    if canonical.contains('<') {
        return type_for(Language::Java, canonical);
    }
    match canonical {
        "int" => "Integer".into(),
        "float" => "Double".into(),
        "bool" => "Boolean".into(),
        "void" => "Void".into(),
        other => type_for(Language::Java, other),
    }
}

fn replace_words(expression: &str, pairs: &[(&str, &str)]) -> String {
    let mut result = expression.to_string();
    for (from, to) in pairs {
        result = result.replace(from, to);
    }
    result
}

fn snake_case_identifier(value: &str) -> String {
    let mut result = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index > 0 {
                result.push('_');
            }
            result.push(character.to_ascii_lowercase());
        } else {
            result.push(character);
        }
    }
    result
}

fn lower_camel_case_identifier(value: &str) -> String {
    let mut result = String::new();
    let mut uppercase_next = false;
    for character in value.chars() {
        if character == '_' {
            uppercase_next = true;
        } else if uppercase_next {
            result.push(character.to_ascii_uppercase());
            uppercase_next = false;
        } else {
            result.push(character);
        }
    }
    result
}

fn rewrite_swift_identifiers(value: &str) -> String {
    let mut result = String::new();
    let mut token = String::new();
    let mut quote = '\0';
    let flush = |result: &mut String, token: &mut String| {
        if !token.is_empty() {
            if token.contains('_') {
                result.push_str(&lower_camel_case_identifier(token));
            } else {
                result.push_str(token);
            }
            token.clear();
        }
    };
    for character in value.chars() {
        if quote != '\0' {
            result.push(character);
            if character == quote {
                quote = '\0';
            }
        } else if character == '"' || character == '\'' {
            flush(&mut result, &mut token);
            quote = character;
            result.push(character);
        } else if character == '_' || character.is_alphanumeric() {
            token.push(character);
        } else {
            flush(&mut result, &mut token);
            result.push(character);
        }
    }
    flush(&mut result, &mut token);
    result
}

fn rewrite_rust_call_names(value: &str) -> String {
    let chars = value.char_indices().collect::<Vec<_>>();
    let mut result = String::new();
    let mut cursor = 0usize;
    for (index, character) in &chars {
        if *character != '(' {
            continue;
        }
        let mut start = *index;
        while start > 0 {
            let previous = value[..start].chars().next_back().unwrap();
            if previous == '_' || previous.is_ascii_alphanumeric() {
                start -= previous.len_utf8();
            } else {
                break;
            }
        }
        let name = &value[start..*index];
        if name.is_empty()
            || name
                .chars()
                .next()
                .is_some_and(|item| item.is_ascii_uppercase())
            || !name.chars().any(|item| item.is_ascii_uppercase())
        {
            continue;
        }
        result.push_str(&value[cursor..start]);
        result.push_str(&snake_case_identifier(name));
        cursor = *index;
    }
    result.push_str(&value[cursor..]);
    result
}

fn rewrite_java_index_accesses(value: &str) -> String {
    let mut result = value.to_string();
    let mut search_from = 0usize;
    loop {
        let Some(relative_open) = result[search_from..].find('[') else {
            break;
        };
        let open = search_from + relative_open;
        let mut start = open;
        while start > 0 {
            let previous = result[..start].chars().next_back().unwrap();
            if previous == '_' || previous.is_ascii_alphanumeric() || previous == '.' {
                start -= previous.len_utf8();
            } else {
                break;
            }
        }
        if start == open {
            search_from = open + 1;
            continue;
        }
        let Some(relative_close) = result[open + 1..].find(']') else {
            break;
        };
        let close = open + 1 + relative_close;
        let object = result[start..open].to_string();
        let index = result[open + 1..close].to_string();
        let replacement = format!("{}.get({})", object, index);
        result.replace_range(start..=close, &replacement);
        search_from = start + replacement.len();
    }
    result
}

fn expression_for(target: Language, expression: &str) -> String {
    let mut value = expression.trim().trim_end_matches(';').to_string();
    match target {
        Language::Python => {
            if value.contains(".every(") && value.contains("=>") {
                if let Some((receiver, closure)) = value.split_once(".every(") {
                    let closure = closure.trim_end_matches(')').trim();
                    if let Some((parameter, predicate)) = closure.split_once("=>") {
                        let receiver = receiver.trim().trim_end_matches(".values");
                        value = format!(
                            "all({} for {} in {}.values())",
                            predicate.trim(),
                            parameter
                                .trim()
                                .trim_start_matches('(')
                                .trim_end_matches(')'),
                            receiver
                        );
                    }
                }
            }
            value = replace_dart_length(&value);
            value = value.replace("??", " or ");
            value = value.replace("=>", ":");
            value = value.replace("!=", "__NOT_EQUAL__");
            value = replace_words(
                &value,
                &[
                    ("true", "True"),
                    ("false", "False"),
                    ("null", "None"),
                    ("&&", "and"),
                    ("||", "or"),
                ],
            );
            value = value.replace('!', "not ");
            value = value.replace("__NOT_EQUAL__", "!=");
        }
        _ => {
            value = replace_words(
                &value,
                &[
                    ("True", "true"),
                    ("False", "false"),
                    ("None", "null"),
                    (" and ", " && "),
                    (" or ", " || "),
                    ("not ", "!"),
                ],
            );
            if target == Language::Swift {
                value = value.replace("null", "nil");
                value = value.replace("//", "/");
                while let Some(start) = value.find("len(") {
                    let Some(relative_close) = value[start + 4..].find(')') else {
                        break;
                    };
                    let close = start + 4 + relative_close;
                    let argument = value[start + 4..close].trim();
                    value.replace_range(start..=close, &format!("{}.count", argument));
                }
                value = rewrite_swift_identifiers(&value);
            }
            if target == Language::Java {
                if let Some((left, right)) = value.split_once(" != ") {
                    if left.trim().starts_with('"') || right.trim().starts_with('"') {
                        value = format!("!{}.equals({})", left.trim(), right.trim());
                    }
                } else if let Some((left, right)) = value.split_once(" == ") {
                    if left.trim().starts_with('"') || right.trim().starts_with('"') {
                        value = format!("{}.equals({})", left.trim(), right.trim());
                    }
                }
                value = rewrite_java_index_accesses(&value);
            }
            if target == Language::Rust {
                value = value.replace("null", "None").replace(".to_string()", "");
                if let Some((left, right)) = value.split_once(" + ") {
                    if left.trim().starts_with('"') || right.trim().starts_with('"') {
                        value = format!("format!(\"{{}}{{}}\", {}, {})", left.trim(), right.trim());
                    }
                }
                value = rewrite_rust_call_names(&value);
            }
        }
    }
    if matches!(
        target,
        Language::JavaScript | Language::Java | Language::Go | Language::Rust
    ) {
        value = rewrite_default_constructors(&value, target);
    }
    if matches!(target, Language::Java | Language::Go | Language::Rust) {
        value = rewrite_numeric_list_literals(&value, target);
    }
    value
}

fn rewrite_default_constructors(value: &str, target: Language) -> String {
    let chars = value.char_indices().collect::<Vec<_>>();
    let mut result = String::new();
    let mut cursor = 0usize;
    for (position, (index, character)) in chars.iter().enumerate() {
        if *character != '(' || value[*index..].get(..2) != Some("()") {
            continue;
        }
        let mut start = *index;
        while start > 0 {
            let previous = value[..start].chars().next_back().unwrap();
            if previous == '_' || previous.is_ascii_alphanumeric() {
                start -= previous.len_utf8();
            } else {
                break;
            }
        }
        let name = &value[start..*index];
        if name
            .chars()
            .next()
            .is_none_or(|first| !first.is_ascii_uppercase())
        {
            continue;
        }
        result.push_str(&value[cursor..start]);
        match target {
            Language::JavaScript => result.push_str(&format!("new {}()", name)),
            Language::Java => result.push_str(&format!("new {}()", name)),
            Language::Go => result.push_str(&format!("{}{{}}", name)),
            Language::Rust => result.push_str(name),
            _ => result.push_str(&value[start..*index + 2]),
        }
        cursor = *index + 2;
        let _ = position;
    }
    result.push_str(&value[cursor..]);
    result
}

fn rewrite_numeric_list_literals(value: &str, target: Language) -> String {
    let mut result = String::new();
    let mut cursor = 0usize;
    let bytes = value.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }
        let previous = value[..index].chars().next_back();
        if previous.is_some_and(|character| character.is_ascii_alphanumeric() || character == '_') {
            index += 1;
            continue;
        }
        let Some(relative_close) = value[index + 1..].find(']') else {
            break;
        };
        let close = index + 1 + relative_close;
        let contents = &value[index + 1..close];
        if !contents.chars().all(|character| {
            character.is_ascii_digit()
                || character.is_whitespace()
                || character == ','
                || character == '-'
        }) {
            index = close + 1;
            continue;
        }
        result.push_str(&value[cursor..index]);
        match target {
            Language::Java => result.push_str(&format!("List.of({})", contents)),
            Language::Go => result.push_str(&format!("[]int{{{}}}", contents)),
            Language::Rust => result.push_str(&format!("vec![{}]", contents)),
            _ => result.push_str(&value[index..=close]),
        }
        cursor = close + 1;
        index = close + 1;
    }
    result.push_str(&value[cursor..]);
    result
}

fn indent(level: usize) -> String {
    "    ".repeat(level)
}

fn function_return_type(function: &Statement) -> String {
    if let Statement::Function {
        return_type, body, ..
    } = function
    {
        if let Some(ty) = return_type {
            return ty.clone();
        }
        fn find_return(body: &[Statement], scope: &[Statement]) -> Option<String> {
            for statement in body {
                match statement {
                    Statement::Return(Some(value)) => {
                        let inferred = infer_type(value);
                        if inferred != "any" {
                            return Some(inferred);
                        }
                        return scope
                            .iter()
                            .find_map(|statement| match statement {
                                Statement::Variable {
                                    name,
                                    value: initializer,
                                    type_hint,
                                    ..
                                } if name == value.trim() => Some(
                                    type_hint.clone().unwrap_or_else(|| infer_type(initializer)),
                                ),
                                _ => None,
                            })
                            .or(Some(inferred));
                    }
                    Statement::If {
                        then_body,
                        else_body,
                        ..
                    } => {
                        if let Some(value) =
                            find_return(then_body, scope).or_else(|| find_return(else_body, scope))
                        {
                            return Some(value);
                        }
                    }
                    Statement::For { body, .. }
                    | Statement::ForEach { body, .. }
                    | Statement::While { body, .. } => {
                        if let Some(value) = find_return(body, scope) {
                            return Some(value);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        if let Some(value) = find_return(body, body) {
            return value;
        }
    }
    "void".into()
}

fn parameter_type(param: &Parameter, body: &[Statement]) -> String {
    if let Some(ty) = &param.type_hint {
        return ty.clone();
    }
    fn search(name: &str, body: &[Statement]) -> Option<String> {
        for statement in body {
            match statement {
                Statement::Expression(value)
                    if value.contains(name)
                        && [" + ", " - ", " * ", " / ", " % ", "+=", "-=", "*=", "/="]
                            .iter()
                            .any(|operator| value.contains(operator)) =>
                {
                    return Some("int".into())
                }
                Statement::Variable { value, .. }
                    if value.contains(name) && (value.contains('"') || value.contains('\'')) =>
                {
                    return Some("string".into())
                }
                Statement::Variable { value, .. }
                    if value.contains(name)
                        && [" + ", " - ", " * ", " / "]
                            .iter()
                            .any(|op| value.contains(op)) =>
                {
                    return Some("int".into())
                }
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } => {
                    let boolean_condition = condition.replace("||", "&&");
                    let boolean_use = boolean_condition.split("&&").any(|part| {
                        let part = part.trim().trim_start_matches('!').trim();
                        part == name
                    });
                    if boolean_use {
                        return Some("bool".into());
                    }
                    let numeric_comparison = [">=", "<=", ">", "<"].iter().any(|operator| {
                        condition
                            .split_once(operator)
                            .map(|(left, right)| {
                                left.split_whitespace().last() == Some(name)
                                    || right.split_whitespace().next() == Some(name)
                            })
                            .unwrap_or(false)
                    });
                    if numeric_comparison {
                        return Some("int".into());
                    }
                    if let Some(found) = search(name, then_body).or_else(|| search(name, else_body))
                    {
                        return Some(found);
                    }
                }
                Statement::ForEach {
                    variable,
                    type_hint,
                    iterable,
                    body,
                } if iterable.trim() == name => {
                    let element = type_hint
                        .clone()
                        .or_else(|| search(variable, body))
                        .unwrap_or_else(|| "any".into());
                    return Some(format!("list<{}>", element));
                }
                Statement::For { body, .. }
                | Statement::ForEach { body, .. }
                | Statement::While { body, .. } => {
                    if let Some(found) = search(name, body) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }
    search(&param.name, body).unwrap_or_else(|| "any".into())
}

fn foreach_element_type(variable: &str, explicit: &Option<String>, body: &[Statement]) -> String {
    explicit.clone().unwrap_or_else(|| {
        parameter_type(
            &Parameter {
                name: variable.into(),
                type_hint: None,
            },
            body,
        )
    })
}

fn emit_statement(statement: &Statement, target: Language, level: usize) -> String {
    let pad = match target {
        Language::Go => "\t".repeat(level),
        Language::Dart => "  ".repeat(level),
        _ => indent(level),
    };
    match statement {
        Statement::Comment(value) => {
            let comment = if target == Language::Python && value.starts_with("///") {
                format!("###{}", &value[3..])
            } else if target == Language::Python && value.starts_with("//") {
                format!("#{}", &value[2..])
            } else if target != Language::Python && value.starts_with('#') {
                format!("//{}", &value[1..])
            } else {
                value.clone()
            };
            format!("{}{}", pad, comment)
        }
        Statement::Variable {
            name,
            value,
            mutable,
            type_hint,
        } => {
            let expression = expression_for(target, value);
            let inferred = type_hint.clone().unwrap_or_else(|| infer_type(value));
            match target {
                Language::JavaScript => format!(
                    "{}{} {} = {};",
                    pad,
                    if *mutable { "let" } else { "const" },
                    name,
                    expression
                ),
                Language::Python => format!("{}{} = {}", pad, name, expression),
                Language::Java => format!(
                    "{}{}{} {} = {};",
                    pad,
                    if *mutable { "" } else { "final " },
                    type_for(target, &inferred),
                    name,
                    expression
                ),
                Language::Dart => format!(
                    "{}{} {} = {};",
                    pad,
                    if *mutable { "var" } else { "final" },
                    name,
                    expression
                ),
                Language::Swift => format!(
                    "{}{} {} = {}",
                    pad,
                    if *mutable { "var" } else { "let" },
                    lower_camel_case_identifier(name),
                    expression
                ),
                Language::Go => format!("{}{} := {}", pad, name, expression),
                Language::Rust => {
                    let rust_expression = if inferred == "string" && expression.starts_with('"') {
                        format!("{}.to_string()", expression)
                    } else {
                        expression
                    };
                    format!(
                        "{}let {}{} = {};",
                        pad,
                        if *mutable { "mut " } else { "" },
                        name,
                        rust_expression
                    )
                }
            }
        }
        Statement::Class { name, body } => emit_class_statement(name, body, target, level),
        Statement::Print { values } => {
            let converted = values
                .iter()
                .map(|v| expression_for(target, v))
                .collect::<Vec<_>>();
            match target {
                Language::JavaScript => format!("{}console.log({});", pad, converted.join(", ")),
                Language::Python | Language::Dart | Language::Swift => format!(
                    "{}print({}){}",
                    pad,
                    converted.join(", "),
                    if target == Language::Dart { ";" } else { "" }
                ),
                Language::Java => format!(
                    "{}System.out.println({});",
                    pad,
                    if converted.len() > 1 {
                        converted.join(" + \" \" + ")
                    } else {
                        converted.join("")
                    }
                ),
                Language::Go => format!("{}fmt.Println({})", pad, converted.join(", ")),
                Language::Rust => {
                    if converted.is_empty() {
                        format!("{}println!();", pad)
                    } else {
                        format!(
                            "{}println!(\"{}\", {});",
                            pad,
                            vec!["{}"; converted.len()].join(" "),
                            converted.join(", ")
                        )
                    }
                }
            }
        }
        Statement::Return(value) => match value {
            Some(value) => {
                let mut expression = expression_for(target, value);
                if target == Language::Rust
                    && infer_type(value) == "string"
                    && expression.starts_with('"')
                {
                    expression.push_str(".to_string()");
                }
                format!(
                    "{}return {}{}",
                    pad,
                    expression,
                    if matches!(
                        target,
                        Language::JavaScript | Language::Java | Language::Dart | Language::Rust
                    ) {
                        ";"
                    } else {
                        ""
                    }
                )
            }
            None => format!(
                "{}return{}",
                pad,
                if matches!(
                    target,
                    Language::JavaScript | Language::Java | Language::Dart | Language::Rust
                ) {
                    ";"
                } else {
                    ""
                }
            ),
        },
        Statement::Expression(value) => format!(
            "{}{}{}",
            pad,
            expression_for(target, value),
            if matches!(
                target,
                Language::JavaScript | Language::Java | Language::Dart | Language::Rust
            ) {
                ";"
            } else {
                ""
            }
        ),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            let cond = expression_for(target, condition);
            let then_text = then_body
                .iter()
                .map(|s| emit_statement(s, target, level + 1))
                .collect::<Vec<_>>()
                .join("\n");
            let else_text = else_body
                .iter()
                .map(|s| emit_statement(s, target, level + 1))
                .collect::<Vec<_>>()
                .join("\n");
            match target {
                Language::Python => format!(
                    "{}if {}:\n{}{}",
                    pad,
                    cond,
                    if then_text.is_empty() {
                        format!("{}pass", indent(level + 1))
                    } else {
                        then_text
                    },
                    if else_body.is_empty() {
                        String::new()
                    } else {
                        format!("\n{}else:\n{}", pad, else_text)
                    }
                ),
                Language::Rust | Language::Go => format!(
                    "{}if {} {{\n{}\n{}}}{}",
                    pad,
                    cond,
                    then_text,
                    pad,
                    if else_body.is_empty() {
                        String::new()
                    } else {
                        format!(" else {{\n{}\n{}}}", else_text, pad)
                    }
                ),
                _ => format!(
                    "{}if ({}) {{\n{}\n{}}}{}",
                    pad,
                    cond,
                    then_text,
                    pad,
                    if else_body.is_empty() {
                        String::new()
                    } else {
                        format!(" else {{\n{}\n{}}}", else_text, pad)
                    }
                ),
            }
        }
        Statement::ForEach {
            variable,
            type_hint,
            iterable,
            body,
        } => {
            let rendered_body = body
                .iter()
                .map(|statement| emit_statement(statement, target, level + 1))
                .collect::<Vec<_>>()
                .join("\n");
            let iterable = expression_for(target, iterable);
            let element_type = foreach_element_type(variable, type_hint, body);
            match target {
                Language::JavaScript => format!(
                    "{}for (const {} of {}) {{\n{}\n{}}}",
                    pad, variable, iterable, rendered_body, pad
                ),
                Language::Python => format!(
                    "{}for {} in {}:\n{}",
                    pad,
                    variable,
                    iterable,
                    if rendered_body.is_empty() {
                        format!("{}pass", indent(level + 1))
                    } else {
                        rendered_body
                    }
                ),
                Language::Java => format!(
                    "{}for ({} {} : {}) {{\n{}\n{}}}",
                    pad,
                    type_for(target, &element_type),
                    variable,
                    iterable,
                    rendered_body,
                    pad
                ),
                Language::Dart => format!(
                    "{}for (final {} {} in {}) {{\n{}\n{}}}",
                    pad,
                    type_for(target, &element_type),
                    variable,
                    iterable,
                    rendered_body,
                    pad
                ),
                Language::Swift => format!(
                    "{}for {} in {} {{\n{}\n{}}}",
                    pad, variable, iterable, rendered_body, pad
                ),
                Language::Go => format!(
                    "{}for _, {} := range {} {{\n{}\n{}}}",
                    pad, variable, iterable, rendered_body, pad
                ),
                Language::Rust => format!(
                    "{}for {} in {} {{\n{}\n{}}}",
                    pad, variable, iterable, rendered_body, pad
                ),
            }
        }
        Statement::While { condition, body } => {
            let rendered_body = body
                .iter()
                .map(|statement| emit_statement(statement, target, level + 1))
                .collect::<Vec<_>>()
                .join("\n");
            let condition = expression_for(target, condition);
            match target {
                Language::Python => format!(
                    "{}while {}:\n{}",
                    pad,
                    condition,
                    if rendered_body.is_empty() {
                        format!("{}pass", indent(level + 1))
                    } else {
                        rendered_body
                    }
                ),
                Language::Go | Language::Rust | Language::Swift => format!(
                    "{}while {} {{\n{}\n{}}}",
                    pad, condition, rendered_body, pad
                ),
                _ => format!(
                    "{}while ({}) {{\n{}\n{}}}",
                    pad, condition, rendered_body, pad
                ),
            }
        }
        Statement::For {
            initializer,
            condition,
            update,
            body,
        } => {
            let rendered_body = body
                .iter()
                .map(|statement| emit_statement(statement, target, level + 1))
                .collect::<Vec<_>>()
                .join("\n");
            if target == Language::Python {
                let init = initializer
                    .as_deref()
                    .and_then(|statement| match statement {
                        Statement::Variable { name, value, .. } => {
                            Some((name.as_str(), value.as_str()))
                        }
                        _ => None,
                    });
                let range = init.and_then(|(name, start)| {
                    let (operator, end) = condition
                        .split_once("<")
                        .or_else(|| condition.split_once("<="))?;
                    if operator.trim() != name {
                        return None;
                    }
                    let step = if update.trim() == format!("{}++", name) {
                        "1"
                    } else {
                        "-1"
                    };
                    Some(format!(
                        "range({}, {}{}, {})",
                        start,
                        expression_for(target, end.trim()),
                        if condition.contains("<=") { " + 1" } else { "" },
                        step
                    ))
                });
                if let Some(range) = range {
                    return format!(
                        "{}for {} in {}:\n{}",
                        pad,
                        init.unwrap().0,
                        range,
                        rendered_body
                    );
                }
                return format!(
                    "{}while {}:\n{}",
                    pad,
                    expression_for(target, condition),
                    rendered_body
                );
            }
            let initializer_text = initializer
                .as_deref()
                .map(|value| {
                    emit_statement(value, target, 0)
                        .trim()
                        .trim_end_matches(';')
                        .to_string()
                })
                .unwrap_or_default();
            let condition_text = expression_for(target, condition);
            let update_text = expression_for(target, update);
            match target {
                Language::JavaScript | Language::Java | Language::Dart => format!(
                    "{}for ({}; {}; {}) {{\n{}\n{}}}",
                    pad, initializer_text, condition_text, update_text, rendered_body, pad
                ),
                Language::Go => format!(
                    "{}for {}; {}; {} {{\n{}\n{}}}",
                    pad, initializer_text, condition_text, update_text, rendered_body, pad
                ),
                Language::Swift | Language::Rust => {
                    let update =
                        emit_statement(&Statement::Expression(update.clone()), target, level + 1);
                    format!(
                        "{}{}\n{}while {} {{\n{}\n{}\n{}}}",
                        pad, initializer_text, pad, condition_text, rendered_body, update, pad
                    )
                }
                Language::Python => unreachable!(),
            }
        }
        Statement::Break => format!(
            "{}break{}",
            pad,
            if matches!(
                target,
                Language::JavaScript | Language::Java | Language::Dart | Language::Rust
            ) {
                ";"
            } else {
                ""
            }
        ),
        Statement::Continue => format!(
            "{}continue{}",
            pad,
            if matches!(
                target,
                Language::JavaScript | Language::Java | Language::Dart | Language::Rust
            ) {
                ";"
            } else {
                ""
            }
        ),
        Statement::Function {
            name, params, body, ..
        } => {
            let rendered_body = body
                .iter()
                .map(|s| emit_statement(s, target, level + 1))
                .collect::<Vec<_>>()
                .join("\n");
            let return_type = function_return_type(statement);
            let rendered_params = params
                .iter()
                .filter(|parameter| parameter.name != "self")
                .map(|p| {
                    let ty = parameter_type(p, body);
                    match target {
                        Language::JavaScript => p.name.clone(),
                        Language::Python => format!("{}: {}", p.name, type_for(target, &ty)),
                        Language::Java | Language::Dart => {
                            format!("{} {}", type_for(target, &ty), p.name)
                        }
                        Language::Swift => format!("_ {}: {}", p.name, type_for(target, &ty)),
                        Language::Go => format!("{} {}", p.name, type_for(target, &ty)),
                        Language::Rust => format!(
                            "{}: {}",
                            p.name,
                            if ty == "string" {
                                "&str".into()
                            } else {
                                type_for(target, &ty)
                            }
                        ),
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            let rendered_params = if target == Language::Python && level > 0 {
                if rendered_params.is_empty() {
                    "self".into()
                } else {
                    format!("self, {}", rendered_params)
                }
            } else {
                rendered_params
            };
            match target {
                Language::JavaScript => format!(
                    "{}function {}({}) {{\n{}\n{}}}",
                    pad, name, rendered_params, rendered_body, pad
                ),
                Language::Python => format!(
                    "{}def {}({}) -> {}:\n{}",
                    pad,
                    name,
                    rendered_params,
                    type_for(target, &return_type),
                    if rendered_body.is_empty() {
                        format!("{}pass", indent(level + 1))
                    } else {
                        rendered_body
                    }
                ),
                Language::Java => format!(
                    "{}public {}{} {}({}) {{\n{}\n{}}}",
                    pad,
                    if level > 1 { "" } else { "static " },
                    type_for(target, &return_type),
                    name,
                    rendered_params,
                    rendered_body,
                    pad
                ),
                Language::Dart => format!(
                    "{}{} {}({}) {{\n{}\n{}}}",
                    pad,
                    type_for(target, &return_type),
                    name,
                    rendered_params,
                    rendered_body,
                    pad
                ),
                Language::Swift => format!(
                    "{}func {}({}){} {{\n{}\n{}}}",
                    pad,
                    lower_camel_case_identifier(name),
                    rendered_params,
                    if return_type == "void" {
                        String::new()
                    } else {
                        format!(" -> {}", type_for(target, &return_type))
                    },
                    rendered_body,
                    pad
                ),
                Language::Go => format!(
                    "{}func {}({}){} {{\n{}\n{}}}",
                    pad,
                    name,
                    rendered_params,
                    if return_type == "void" {
                        String::new()
                    } else {
                        format!(" {}", type_for(target, &return_type))
                    },
                    rendered_body,
                    pad
                ),
                Language::Rust => format!(
                    "{}fn {}({}){} {{\n{}\n{}}}",
                    pad,
                    snake_case_identifier(name),
                    rendered_params,
                    if return_type == "void" {
                        String::new()
                    } else {
                        format!(" -> {}", type_for(target, &return_type))
                    },
                    rendered_body,
                    pad
                ),
            }
        }
    }
}

fn emit_class_statement(
    name: &str,
    members: &[Statement],
    target: Language,
    level: usize,
) -> String {
    let pad = match target {
        Language::Go => "\t".repeat(level),
        Language::Dart => "  ".repeat(level),
        _ => indent(level),
    };
    match target {
        Language::JavaScript => {
            let methods = members
                .iter()
                .filter_map(|member| {
                    let Statement::Function {
                        name, params, body, ..
                    } = member
                    else {
                        return None;
                    };
                    let body = body
                        .iter()
                        .map(|statement| emit_statement(statement, target, level + 2))
                        .collect::<Vec<_>>()
                        .join("\n");
                    Some(format!(
                        "{}{}({}) {{\n{}\n{}}}",
                        indent(level + 1),
                        name,
                        params
                            .iter()
                            .filter(|parameter| parameter.name != "self")
                            .map(|parameter| parameter.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                        body,
                        indent(level + 1)
                    ))
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            format!("{}class {} {{\n{}\n{}}}", pad, name, methods, pad)
        }
        Language::Python => {
            let rendered = members
                .iter()
                .map(|statement| emit_statement(statement, target, level + 1))
                .collect::<Vec<_>>()
                .join("\n\n");
            format!(
                "{}class {}:\n{}",
                pad,
                name,
                if rendered.is_empty() {
                    format!("{}pass", indent(level + 1))
                } else {
                    rendered
                }
            )
        }
        Language::Java => {
            let rendered = members
                .iter()
                .map(|statement| emit_statement(statement, target, level + 1))
                .collect::<Vec<_>>()
                .join("\n\n");
            format!("{}static class {} {{\n{}\n{}}}", pad, name, rendered, pad)
        }
        Language::Dart | Language::Swift => {
            let rendered = members
                .iter()
                .map(|statement| emit_statement(statement, target, level + 1))
                .collect::<Vec<_>>()
                .join("\n\n");
            format!("{}class {} {{\n{}\n{}}}", pad, name, rendered, pad)
        }
        Language::Go => {
            let methods = members
                .iter()
                .filter_map(|member| {
                    let Statement::Function {
                        name: method,
                        params,
                        body,
                        ..
                    } = member
                    else {
                        return None;
                    };
                    let parameters = params
                        .iter()
                        .filter(|parameter| parameter.name != "self")
                        .map(|parameter| {
                            let ty = parameter_type(parameter, body);
                            format!("{} {}", parameter.name, type_for(target, &ty))
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    let return_type = function_return_type(member);
                    let body = body
                        .iter()
                        .map(|statement| emit_statement(statement, target, 1))
                        .collect::<Vec<_>>()
                        .join("\n");
                    Some(format!(
                        "func (_self {}) {}({}){} {{\n{}\n}}",
                        name,
                        method,
                        parameters,
                        if return_type == "void" {
                            String::new()
                        } else {
                            format!(" {}", type_for(target, &return_type))
                        },
                        body
                    ))
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            format!(
                "type {} struct {{}}{}{}",
                name,
                if methods.is_empty() { "" } else { "\n\n" },
                methods
            )
        }
        Language::Rust => {
            let methods = members
                .iter()
                .filter_map(|member| {
                    let Statement::Function {
                        name: method,
                        params,
                        body,
                        ..
                    } = member
                    else {
                        return None;
                    };
                    let parameters = params
                        .iter()
                        .filter(|parameter| parameter.name != "self")
                        .map(|parameter| {
                            let ty = parameter_type(parameter, body);
                            format!("{}: {}", parameter.name, type_for(target, &ty))
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    let return_type = function_return_type(member);
                    let body = body
                        .iter()
                        .map(|statement| emit_statement(statement, target, 2))
                        .collect::<Vec<_>>()
                        .join("\n");
                    Some(format!(
                        "    fn {}(&self{}{}){} {{\n{}\n    }}",
                        method,
                        if parameters.is_empty() { "" } else { ", " },
                        parameters,
                        if return_type == "void" {
                            String::new()
                        } else {
                            format!(" -> {}", type_for(target, &return_type))
                        },
                        body
                    ))
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            format!("#[allow(dead_code, non_snake_case, unused_variables)]\nstruct {};\n\n#[allow(dead_code, non_snake_case, unused_variables)]\nimpl {} {{\n{}\n}}", name, name, methods)
        }
    }
}

fn emit(program: &Program, target: Language) -> String {
    let program = if target != Language::Python && !program.preserve_classes {
        let mut body = Vec::new();
        for statement in &program.body {
            if let Statement::Class { body: members, .. } = statement {
                body.extend(members.iter().cloned());
            } else {
                body.push(statement.clone());
            }
        }
        Program {
            comments: program.comments.clone(),
            body,
            preserve_classes: program.preserve_classes,
        }
    } else {
        program.clone()
    };
    let canonical_entrypoint = matches!(
        target,
        Language::Java | Language::Dart | Language::Go | Language::Rust
    );
    let explicit_main = program.body.iter().find_map(|statement| match statement {
        Statement::Function { name, body, .. } if name == "main" => Some(body.as_slice()),
        _ => None,
    });
    let top_level = program
        .body
        .iter()
        .filter(|s| !matches!(s, Statement::Function { .. } | Statement::Class { .. }))
        .filter(|statement| {
            !canonical_entrypoint
                || !matches!(statement, Statement::Expression(value) if value.trim() == "main()")
        })
        .collect::<Vec<_>>();
    let declarations = program
        .body
        .iter()
        .filter(|s| matches!(s, Statement::Function { .. } | Statement::Class { .. }))
        .filter(|statement| {
            !canonical_entrypoint
                || !matches!(statement, Statement::Function { name, .. } if name == "main")
        })
        .collect::<Vec<_>>();
    let entry_body = explicit_main
        .into_iter()
        .flat_map(|body| body.iter())
        .chain(top_level.iter().copied())
        .collect::<Vec<_>>();
    let code = match target {
        Language::JavaScript => format!(
            "\"use strict\";\n\n{}",
            declarations
                .iter()
                .chain(top_level.iter())
                .map(|s| emit_statement(s, target, 0))
                .collect::<Vec<_>>()
                .join("\n\n")
        ),
        Language::Python | Language::Swift => declarations
            .iter()
            .chain(top_level.iter())
            .map(|s| emit_statement(s, target, 0))
            .collect::<Vec<_>>()
            .join("\n\n"),
        Language::Java => {
            let fn_text = declarations
                .iter()
                .map(|s| emit_statement(s, target, 1))
                .collect::<Vec<_>>()
                .join("\n\n");
            let main = entry_body
                .iter()
                .map(|s| emit_statement(s, target, 2))
                .collect::<Vec<_>>()
                .join("\n");
            let code = format!("public final class TranslatedProgram {{\n    private TranslatedProgram() {{}}\n{}{}{}\n    public static void main(String[] args) {{\n{}\n    }}\n}}",
                if fn_text.is_empty() { "" } else { "\n" }, fn_text, if fn_text.is_empty() { "" } else { "\n" }, main)
            ;
            if ["List<", "Set<", "Map<", "List.of("]
                .iter()
                .any(|symbol| code.contains(symbol))
            {
                format!("import java.util.*;\n\n{}", code)
            } else {
                code
            }
        }
        Language::Dart => {
            let fn_text = declarations
                .iter()
                .map(|s| emit_statement(s, target, 0))
                .collect::<Vec<_>>()
                .join("\n\n");
            let main = entry_body
                .iter()
                .map(|s| emit_statement(s, target, 1))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{}{}void main() {{\n{}\n}}",
                fn_text,
                if fn_text.is_empty() { "" } else { "\n\n" },
                main
            )
        }
        Language::Go => {
            let fn_text = declarations
                .iter()
                .map(|s| emit_statement(s, target, 0))
                .collect::<Vec<_>>()
                .join("\n\n");
            let main = entry_body
                .iter()
                .map(|s| emit_statement(s, target, 1))
                .collect::<Vec<_>>()
                .join("\n");
            let body = format!(
                "{}{}func main() {{\n{}\n}}",
                fn_text,
                if fn_text.is_empty() { "" } else { "\n\n" },
                main
            );
            format!(
                "package main\n\n{}{}",
                if body.contains("fmt.") {
                    "import \"fmt\"\n\n"
                } else {
                    ""
                },
                body
            )
        }
        Language::Rust => {
            let fn_text = declarations
                .iter()
                .map(|s| emit_statement(s, target, 0))
                .collect::<Vec<_>>()
                .join("\n\n");
            let main = entry_body
                .iter()
                .map(|s| emit_statement(s, target, 1))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{}{}fn main() {{\n{}\n}}",
                fn_text,
                if fn_text.is_empty() { "" } else { "\n\n" },
                main
            )
        }
    };
    let comments = crate::backend::emit_comments(&program.comments, target);
    if comments.is_empty() {
        code
    } else if code.is_empty() {
        comments
    } else {
        format!("{}\n\n{}", comments, code)
    }
}

pub fn translate(source: &str, from: Language, to: Language) -> String {
    let mut output = if from == Language::Dart && to == Language::Java {
        use crate::backend::Backend;
        use crate::frontend::Frontend;
        let unit = crate::frontend::dart::DartFrontend.parse(source);
        crate::backend::java::JavaBackend.emit(&unit).code
    } else if from == Language::Dart && to == Language::Python {
        use crate::backend::Backend;
        use crate::frontend::Frontend;
        let unit = crate::frontend::dart::DartFrontend.parse(source);
        crate::backend::python::PythonBackend.emit(&unit).code
    } else if from == Language::Python && to == Language::Dart {
        use crate::backend::Backend;
        let unit = crate::frontend::parse_source(source, from);
        let has_explicit_entrypoint = unit.declarations.iter().any(|declaration| {
            matches!(
                declaration,
                crate::typed_ir::Declaration::Function(function) if function.name == "main"
            )
        });
        if has_explicit_entrypoint {
            crate::backend::dart::DartBackend.emit(&unit).code
        } else {
            emit(&parse(source, from, true), to)
        }
    } else {
        emit(
            &parse(
                source,
                from,
                matches!(from, Language::Dart | Language::Python),
            ),
            to,
        )
    };
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

pub fn translate_by_id(source: &str, from: u32, to: u32) -> String {
    translate(source, Language::from_id(from), Language::from_id(to))
}

#[no_mangle]
pub extern "C" fn alloc(length: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(length);
    let pointer = buffer.as_mut_ptr();
    mem::forget(buffer);
    pointer
}

#[no_mangle]
pub unsafe extern "C" fn transpile(pointer: *mut u8, length: usize, from: u32, to: u32) {
    let input = Vec::from_raw_parts(pointer, length, length);
    let source = String::from_utf8_lossy(&input);
    let result = translate(&source, Language::from_id(from), Language::from_id(to));
    *OUTPUT.lock().unwrap() = result.into_bytes();
}

#[no_mangle]
pub extern "C" fn output_ptr() -> *const u8 {
    OUTPUT.lock().unwrap().as_ptr()
}

#[no_mangle]
pub extern "C" fn output_len() -> usize {
    OUTPUT.lock().unwrap().len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    const LANGUAGES: [Language; 7] = [
        Language::JavaScript,
        Language::Java,
        Language::Dart,
        Language::Swift,
        Language::Python,
        Language::Go,
        Language::Rust,
    ];

    fn comment_fixture(language: Language) -> &'static str {
        match language {
            Language::JavaScript => "// COMMENT_LEADING\nfunction main() {\n  /* COMMENT_BLOCK */\n  return; // COMMENT_TRAILING\n}",
            Language::Java => "// COMMENT_LEADING\nfinal class Demo {\n  /* COMMENT_BLOCK */\n  static void main(String[] args) { return; } // COMMENT_TRAILING\n}",
            Language::Dart => "// COMMENT_LEADING\nvoid main() {\n  /* COMMENT_BLOCK */\n  return; // COMMENT_TRAILING\n}",
            Language::Swift => "// COMMENT_LEADING\nfunc main() {\n  /* COMMENT_BLOCK */\n  return // COMMENT_TRAILING\n}",
            Language::Python => "# COMMENT_LEADING\ndef main():\n    # COMMENT_BLOCK\n    return  # COMMENT_TRAILING\n",
            Language::Go => "package main\n// COMMENT_LEADING\nfunc main() {\n  /* COMMENT_BLOCK */\n  return // COMMENT_TRAILING\n}",
            Language::Rust => "// COMMENT_LEADING\nfn main() {\n  /* COMMENT_BLOCK */\n  return; // COMMENT_TRAILING\n}",
        }
    }

    #[test]
    fn every_language_pair_preserves_all_ast_comments() {
        for from in LANGUAGES {
            let source = comment_fixture(from);
            let unit = crate::frontend::parse_source(source, from);
            assert_eq!(unit.comments.len(), 3, "{:?}: {:#?}", from, unit.comments);
            for marker in ["COMMENT_LEADING", "COMMENT_BLOCK", "COMMENT_TRAILING"] {
                assert!(
                    unit.comments
                        .iter()
                        .any(|comment| comment.text.contains(marker)),
                    "{:?} IR lost {}",
                    from,
                    marker
                );
            }

            for to in LANGUAGES {
                let output = translate(source, from, to);
                for marker in ["COMMENT_LEADING", "COMMENT_BLOCK", "COMMENT_TRAILING"] {
                    assert!(
                        output.contains(marker),
                        "{:?} -> {:?} lost {}:\n{}",
                        from,
                        to,
                        marker,
                        output
                    );
                }
                if to == Language::Python {
                    assert!(
                        output
                            .lines()
                            .filter(|line| line.trim_start().starts_with('#'))
                            .count()
                            >= 3,
                        "{:?} -> Python did not normalize comments:\n{}",
                        from,
                        output
                    );
                    assert!(!output.contains("/* COMMENT_BLOCK */"), "{}", output);
                } else {
                    assert!(
                        !output
                            .lines()
                            .any(|line| line.trim_start().starts_with("# COMMENT_")),
                        "{:?} -> {:?} retained Python-only comment syntax:\n{}",
                        from,
                        to,
                        output
                    );
                }
            }
        }
    }

    fn fixture(language: Language) -> &'static str {
        match language {
            Language::JavaScript => "function greet(name) {\n  const message = \"Hello, \" + name;\n  if (name != \"\") { console.log(message); } else { console.log(\"Hello\"); }\n}\ngreet(\"world\");",
            Language::Java => "public final class Demo {\npublic static void greet(String name) {\nfinal String message = \"Hello, \" + name;\nif (!name.isEmpty()) {\nSystem.out.println(message);\n} else {\nSystem.out.println(\"Hello\");\n}\n}\npublic static void main(String[] args) { greet(\"world\"); }\n}",
            Language::Dart => "void greet(String name) {\nfinal message = \"Hello, \" + name;\nif (name.isNotEmpty) {\nprint(message);\n} else {\nprint(\"Hello\");\n}\n}\nvoid main() { greet(\"world\"); }",
            Language::Swift => "func greet(_ name: String) {\nlet message = \"Hello, \" + name\nif (name != \"\") {\nprint(message)\n} else {\nprint(\"Hello\")\n}\n}\ngreet(\"world\")",
            Language::Python => "def greet(name):\n    message = \"Hello, \" + name\n    if name != \"\":\n        print(message)\n    else:\n        print(\"Hello\")\n\ngreet(\"world\")",
            Language::Go => "package main\nimport \"fmt\"\nfunc greet(name string) {\nmessage := \"Hello, \" + name\nif name != \"\" {\nfmt.Println(message)\n} else {\nfmt.Println(\"Hello\")\n}\n}\nfunc main() { greet(\"world\") }",
            Language::Rust => "fn greet(name: String) {\nlet message = \"Hello, \".to_string() + &name;\nif name != \"\" {\nprintln!(\"{}\", message);\n} else {\nprintln!(\"Hello\");\n}\n}\nfn main() { greet(\"world\".to_string()); }",
        }
    }

    fn numeric_fixture(language: Language) -> &'static str {
        match language {
            Language::JavaScript => "function compute(base) {\nlet total = base + 2;\ntotal = total * 3;\nif (total >= 9) { console.log(\"large\"); } else { console.log(\"small\"); }\nreturn total;\n}\nconsole.log(compute(1));",
            Language::Java => "public final class Demo {\npublic static int compute(int base) {\nint total = base + 2;\ntotal = total * 3;\nif (total >= 9) { System.out.println(\"large\"); } else { System.out.println(\"small\"); }\nreturn total;\n}\npublic static void main(String[] args) { System.out.println(compute(1)); }\n}",
            Language::Dart => "int compute(int base) {\nvar total = base + 2;\ntotal = total * 3;\nif (total >= 9) { print(\"large\"); } else { print(\"small\"); }\nreturn total;\n}\nvoid main() { print(compute(1)); }",
            Language::Swift => "func compute(_ base: Int) -> Int {\nvar total = base + 2\ntotal = total * 3\nif total >= 9 { print(\"large\") } else { print(\"small\") }\nreturn total\n}\nprint(compute(1))",
            Language::Python => "def compute(base: int) -> int:\n    total = base + 2\n    total = total * 3\n    if total >= 9:\n        print(\"large\")\n    else:\n        print(\"small\")\n    return total\n\nprint(compute(1))",
            Language::Go => "package main\nimport \"fmt\"\nfunc compute(base int) int {\ntotal := base + 2\ntotal = total * 3\nif total >= 9 { fmt.Println(\"large\") } else { fmt.Println(\"small\") }\nreturn total\n}\nfunc main() { fmt.Println(compute(1)) }",
            Language::Rust => "fn compute(base: i64) -> i64 {\nlet mut total = base + 2;\ntotal = total * 3;\nif total >= 9 { println!(\"large\"); } else { println!(\"small\"); }\nreturn total;\n}\nfn main() { println!(\"{}\", compute(1)); }",
        }
    }

    fn boolean_fixture(language: Language) -> &'static str {
        match language {
            Language::JavaScript => "function classify(score, active) {\nif (active && score > 0) { return \"active\"; } else { return \"inactive\"; }\n}\nconsole.log(classify(3, true));",
            Language::Java => "public final class Demo {\npublic static String classify(int score, boolean active) {\nif (active && score > 0) { return \"active\"; } else { return \"inactive\"; }\n}\npublic static void main(String[] args) { System.out.println(classify(3, true)); }\n}",
            Language::Dart => "String classify(int score, bool active) {\nif (active && score > 0) { return \"active\"; } else { return \"inactive\"; }\n}\nvoid main() { print(classify(3, true)); }",
            Language::Swift => "func classify(_ score: Int, _ active: Bool) -> String {\nif active && score > 0 { return \"active\" } else { return \"inactive\" }\n}\nprint(classify(3, true))",
            Language::Python => "def classify(score: int, active: bool) -> str:\n    if active and score > 0:\n        return \"active\"\n    else:\n        return \"inactive\"\n\nprint(classify(3, True))",
            Language::Go => "package main\nimport \"fmt\"\nfunc classify(score int, active bool) string {\nif active && score > 0 { return \"active\" } else { return \"inactive\" }\n}\nfunc main() { fmt.Println(classify(3, true)) }",
            Language::Rust => "fn classify(score: i64, active: bool) -> String {\nif active && score > 0 { return \"active\".to_string(); } else { return \"inactive\".to_string(); }\n}\nfn main() { println!(\"{}\", classify(3, true)); }",
        }
    }

    fn collection_fixture(language: Language) -> &'static str {
        match language {
            Language::JavaScript => "function sumEven(values) {\nlet total = 0;\nfor (const value of values) {\nif (value % 2 == 0) { total += value; }\n}\nreturn total;\n}\nconst numbers = [1, 2, 3, 4];\nconsole.log(sumEven(numbers));",
            Language::Java => "public final class Demo {\npublic static int sumEven(List<Integer> values) {\nint total = 0;\nfor (int value : values) {\nif (value % 2 == 0) { total += value; }\n}\nreturn total;\n}\npublic static void main(String[] args) {\nfinal List<Integer> numbers = List.of(1, 2, 3, 4);\nSystem.out.println(sumEven(numbers));\n}\n}",
            Language::Dart => "int sumEven(List<int> values) {\nvar total = 0;\nfor (final int value in values) {\nif (value % 2 == 0) { total += value; }\n}\nreturn total;\n}\nvoid main() {\nfinal numbers = [1, 2, 3, 4];\nprint(sumEven(numbers));\n}",
            Language::Swift => "func sumEven(_ values: [Int]) -> Int {\nvar total = 0\nfor value in values {\nif value % 2 == 0 { total += value }\n}\nreturn total\n}\nlet numbers = [1, 2, 3, 4]\nprint(sumEven(numbers))",
            Language::Python => "def sum_even(values: list[int]) -> int:\n    total = 0\n    for value in values:\n        if value % 2 == 0:\n            total += value\n    return total\n\nnumbers = [1, 2, 3, 4]\nprint(sum_even(numbers))",
            Language::Go => "package main\nimport \"fmt\"\nfunc sumEven(values []int) int {\ntotal := 0\nfor _, value := range values {\nif value % 2 == 0 { total += value }\n}\nreturn total\n}\nfunc main() {\nnumbers := []int{1, 2, 3, 4}\nfmt.Println(sumEven(numbers))\n}",
            Language::Rust => "fn sum_even(values: Vec<i64>) -> i64 {\nlet mut total = 0;\nfor value in values {\nif value % 2 == 0 { total += value; }\n}\nreturn total;\n}\nfn main() {\nlet numbers = vec![1, 2, 3, 4];\nprintln!(\"{}\", sum_even(numbers));\n}",
        }
    }

    fn compile_and_run(
        root: &std::path::Path,
        target: Language,
        source: &str,
    ) -> std::process::Output {
        let (file_name, compile, run): (&str, Option<(&str, Vec<&str>)>, (&str, Vec<&str>)) =
            match target {
                Language::JavaScript => ("program.js", None, ("node", vec!["program.js"])),
                Language::Java => (
                    "TranslatedProgram.java",
                    Some((
                        "/opt/homebrew/opt/openjdk/bin/javac",
                        vec!["-Werror", "-Xlint:all", "TranslatedProgram.java"],
                    )),
                    (
                        "/opt/homebrew/opt/openjdk/bin/java",
                        vec!["-cp", ".", "TranslatedProgram"],
                    ),
                ),
                Language::Dart => (
                    "program.dart",
                    None,
                    (
                        "/opt/homebrew/share/flutter/bin/cache/dart-sdk/bin/dart",
                        vec!["run", "program.dart"],
                    ),
                ),
                Language::Swift => (
                    "program.swift",
                    Some((
                        "swiftc",
                        vec![
                            "-warnings-as-errors",
                            "-module-cache-path",
                            ".swift-module-cache",
                            "program.swift",
                            "-o",
                            "program",
                        ],
                    )),
                    ("./program", vec![]),
                ),
                Language::Python => ("program.py", None, ("python3", vec!["program.py"])),
                Language::Go => ("program.go", None, ("go", vec!["run", "program.go"])),
                Language::Rust => (
                    "program.rs",
                    Some((
                        "rustc",
                        vec![
                            "--edition=2024",
                            "-Dwarnings",
                            "program.rs",
                            "-o",
                            "program",
                        ],
                    )),
                    ("./program", vec![]),
                ),
            };
        fs::write(root.join(file_name), source).unwrap();
        if let Some((compiler, arguments)) = compile {
            let result = Command::new(compiler)
                .args(arguments)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{:?} compile failed:\n{}\nGenerated:\n{}",
                target,
                String::from_utf8_lossy(&result.stderr),
                source
            );
        }
        let mut command = Command::new(run.0);
        command.args(run.1).current_dir(root);
        if target == Language::Dart {
            command
                .env("DART_DISABLE_ANALYTICS", "true")
                .env("CI", "true");
        }
        command.output().unwrap()
    }

    #[test]
    fn every_language_pair_produces_real_code() {
        for from in LANGUAGES {
            for to in LANGUAGES {
                let output = translate(fixture(from), from, to);
                assert!(
                    !output.trim().is_empty(),
                    "empty output for {:?} -> {:?}",
                    from,
                    to
                );
                assert!(!output.contains("preview"));
            }
        }
    }

    #[test]
    fn every_language_pair_preserves_typed_collection_iteration() {
        let root = std::env::temp_dir().join(format!(
            "translatecode-collection-matrix-{}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        for from in LANGUAGES {
            for target in LANGUAGES {
                let pair = root.join(format!("{:?}-{:?}", from, target).to_lowercase());
                fs::create_dir_all(&pair).unwrap();
                let generated = translate(collection_fixture(from), from, target);
                let result = compile_and_run(&pair, target, &generated);
                assert!(
                    result.status.success(),
                    "{:?} -> {:?} run failed:\n{}\nGenerated:\n{}",
                    from,
                    target,
                    String::from_utf8_lossy(&result.stderr),
                    generated
                );
                assert_eq!(
                    String::from_utf8_lossy(&result.stdout),
                    "6\n",
                    "{:?} -> {:?} changed collection behavior",
                    from,
                    target
                );
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn python_classes_main_guards_and_call_arguments_survive_every_target() {
        let source = r#"class Solution:
    def check(self, nums: list[int], target: int) -> None:
        print(nums[0] + nums[1] + target)

def main() -> None:
    Solution().check([3, 4], 7)

if __name__ == "__main__":
    main()
"#;
        let root = std::env::temp_dir().join(format!(
            "translatecode-python-oop-matrix-{}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        for target in LANGUAGES {
            let pair = root.join(format!("{:?}", target).to_lowercase());
            fs::create_dir_all(&pair).unwrap();
            let generated = translate(source, Language::Python, target);
            let result = compile_and_run(&pair, target, &generated);
            assert!(
                result.status.success(),
                "Python -> {:?} run failed:\n{}\nGenerated:\n{}",
                target,
                String::from_utf8_lossy(&result.stderr),
                generated
            );
            assert_eq!(
                String::from_utf8_lossy(&result.stdout),
                "14\n",
                "Python -> {:?} changed class/main behavior",
                target
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsupported_constructs_emit_explicit_target_valid_fallbacks() {
        let source = "try:\n    print(\"unsafe\")\nexcept Exception:\n    print(\"fallback\")\n";
        let root = std::env::temp_dir().join(format!(
            "translatecode-fallback-matrix-{}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        for target in LANGUAGES {
            let pair = root.join(format!("{:?}", target).to_lowercase());
            fs::create_dir_all(&pair).unwrap();
            let generated = translate(source, Language::Python, target);
            assert!(generated.contains("Translation stopped at unsupported construct"));
            let result = compile_and_run(&pair, target, &generated);
            assert!(
                result.status.success(),
                "fallback for {:?} was not target-valid:\n{}\nGenerated:\n{}",
                target,
                String::from_utf8_lossy(&result.stderr),
                generated
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn language_profiles_track_current_stable_editions() {
        assert_eq!(Language::JavaScript.profile().version, "ECMAScript 2026");
        assert_eq!(Language::Java.profile().version, "Java SE 26");
        assert_eq!(Language::Dart.profile().version, "Dart 3.12");
        assert_eq!(Language::Swift.profile().version, "Swift 6.3");
        assert_eq!(Language::Python.profile().version, "Python 3.14");
        assert_eq!(Language::Go.profile().version, "Go 1.26");
        assert_eq!(Language::Rust.profile().edition, Some("2024"));
        assert!(LANGUAGES
            .iter()
            .all(|language| !language.profile().preview_features));
    }

    #[test]
    fn target_emitters_use_modern_safe_defaults() {
        let source = numeric_fixture(Language::Python);
        assert!(translate(source, Language::Python, Language::JavaScript)
            .starts_with("\"use strict\";"));
        assert!(translate(source, Language::Python, Language::Java)
            .contains("public final class TranslatedProgram"));
        assert!(translate(source, Language::Python, Language::Python)
            .contains("def compute(base: int) -> int:"));
        assert!(translate(source, Language::Python, Language::Rust).contains("let mut total"));
        assert!(
            !translate(fixture(Language::Python), Language::Python, Language::Rust)
                .contains("let mut message")
        );
    }

    #[test]
    fn generated_go_rust_and_dart_are_formatter_clean() {
        let root =
            std::env::temp_dir().join(format!("translatecode-formatters-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        let source = numeric_fixture(Language::Python);
        let cases = [
            (
                Language::Go,
                "program.go",
                "gofmt",
                vec!["-d", "program.go"],
            ),
            (
                Language::Rust,
                "program.rs",
                "rustfmt",
                vec!["--edition", "2024", "--check", "program.rs"],
            ),
            (
                Language::Dart,
                "program.dart",
                "/opt/homebrew/share/flutter/bin/cache/dart-sdk/bin/dart",
                vec![
                    "format",
                    "--output=none",
                    "--set-exit-if-changed",
                    "program.dart",
                ],
            ),
        ];
        for (target, file_name, formatter, arguments) in cases {
            fs::write(
                root.join(file_name),
                translate(source, Language::Python, target),
            )
            .unwrap();
            let mut command = Command::new(formatter);
            command.args(arguments).current_dir(&root);
            if target == Language::Dart {
                command
                    .env("DART_DISABLE_ANALYTICS", "true")
                    .env("CI", "true");
            }
            let result = match command.output() {
                Ok(result) => result,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => panic!("failed to launch {}: {}", formatter, error),
            };
            let formatter_clean = if target == Language::Dart {
                String::from_utf8_lossy(&result.stdout).contains("(0 changed)")
            } else {
                result.stdout.is_empty()
            };
            assert!(
                result.status.success() && formatter_clean,
                "{:?} formatter rejected output:\n{}{}",
                target,
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn python_function_reaches_ir_and_javascript() {
        let output = translate(
            fixture(Language::Python),
            Language::Python,
            Language::JavaScript,
        );
        assert!(output.contains("function greet(name)"));
        assert!(output.contains("console.log(message);"));
        assert!(output.contains("if (name != \"\")"));
    }

    #[test]
    fn python_binary_search_lowers_python_syntax_to_valid_swift() {
        let source = r#"def binary_search(nums: list[int], target: int) -> int:
    left, right = 0, len(nums) - 1
    while left <= right:
        middle = (left + right) // 2
        if nums[middle] == target:
            return middle
        elif nums[middle] < target:
            left = middle + 1
        else:
            right = middle - 1
    return -1
"#;
        let output = translate(source, Language::Python, Language::Swift);
        assert!(
            output.contains("func binarySearch(_ nums: [Int], _ target: Int) -> Int"),
            "{}",
            output
        );
        assert!(output.contains("var left = 0"), "{}", output);
        assert!(output.contains("var right = nums.count - 1"), "{}", output);
        assert!(
            output.contains("let middle = (left + right) / 2"),
            "{}",
            output
        );
        assert!(output.contains("left = middle + 1"), "{}", output);
        assert!(output.contains("right = middle - 1"), "{}", output);
        assert!(!output.contains("len(nums)"));
        assert!(!output.contains("// 2"));
    }

    #[test]
    fn dart_to_python_preserves_generic_method_parameters_and_main_call_arguments() {
        let source = r#"void main(){
  Solution().twoSum([3,4,5,6], 7);
}

class Solution{
  void twoSum(List<int> nums, int target){
    return;
  }
}"#;
        let output = translate(source, Language::Dart, Language::Python);
        assert!(output.contains("class Solution:"), "{}", output);
        assert!(
            output.contains("def twoSum(self, nums: list[int], target: int) -> None:"),
            "{}",
            output
        );
        assert!(
            output.contains("Solution().twoSum([3, 4, 5, 6], 7)"),
            "{}",
            output
        );
    }

    #[test]
    fn dart_oop_entrypoint_compiles_and_runs_on_every_target() {
        let source = r#"void main() {
  Solution().check([3, 4], 7);
}

class Solution {
  void check(List<int> nums, int target) {
    print(nums[0] + nums[1] + target);
  }
}"#;
        let root = std::env::temp_dir().join(format!(
            "translatecode-dart-oop-matrix-{}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();

        for target in LANGUAGES {
            let pair = root.join(format!("{:?}", target).to_lowercase());
            fs::create_dir_all(&pair).unwrap();
            let (file_name, compile, run): (&str, Option<(&str, Vec<&str>)>, (&str, Vec<&str>)) =
                match target {
                    Language::JavaScript => ("program.js", None, ("node", vec!["program.js"])),
                    Language::Java => (
                        "TranslatedProgram.java",
                        Some((
                            "/opt/homebrew/opt/openjdk/bin/javac",
                            vec!["-Werror", "-Xlint:all", "TranslatedProgram.java"],
                        )),
                        (
                            "/opt/homebrew/opt/openjdk/bin/java",
                            vec!["-cp", ".", "TranslatedProgram"],
                        ),
                    ),
                    Language::Dart => (
                        "program.dart",
                        None,
                        (
                            "/opt/homebrew/share/flutter/bin/cache/dart-sdk/bin/dart",
                            vec!["run", "program.dart"],
                        ),
                    ),
                    Language::Swift => (
                        "program.swift",
                        Some((
                            "swiftc",
                            vec![
                                "-warnings-as-errors",
                                "-module-cache-path",
                                ".swift-module-cache",
                                "program.swift",
                                "-o",
                                "program",
                            ],
                        )),
                        ("./program", vec![]),
                    ),
                    Language::Python => ("program.py", None, ("python3", vec!["program.py"])),
                    Language::Go => ("program.go", None, ("go", vec!["run", "program.go"])),
                    Language::Rust => (
                        "program.rs",
                        Some((
                            "rustc",
                            vec![
                                "--edition=2024",
                                "-Dwarnings",
                                "program.rs",
                                "-o",
                                "program",
                            ],
                        )),
                        ("./program", vec![]),
                    ),
                };
            fs::write(
                pair.join(file_name),
                translate(source, Language::Dart, target),
            )
            .unwrap();
            if let Some((compiler, arguments)) = compile {
                let result = match Command::new(compiler)
                    .args(arguments)
                    .current_dir(&pair)
                    .output()
                {
                    Ok(result) => result,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => panic!("failed to launch {}: {}", compiler, error),
                };
                assert!(
                    result.status.success(),
                    "Dart -> {:?} compile failed:\n{}\nGenerated:\n{}",
                    target,
                    String::from_utf8_lossy(&result.stderr),
                    fs::read_to_string(pair.join(file_name)).unwrap()
                );
            }
            let mut command = Command::new(run.0);
            command.args(run.1).current_dir(&pair);
            if target == Language::Dart {
                command
                    .env("DART_DISABLE_ANALYTICS", "true")
                    .env("CI", "true");
            }
            let result = match command.output() {
                Ok(result) => result,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => panic!("failed to launch {}: {}", run.0, error),
            };
            assert!(
                result.status.success(),
                "Dart -> {:?} run failed:\n{}",
                target,
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&result.stdout),
                "14\n",
                "Dart -> {:?} changed OOP call behavior",
                target
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn every_source_parser_preserves_the_example_program() {
        for from in LANGUAGES {
            let output = translate(fixture(from), from, Language::JavaScript);
            assert!(
                output.contains("function greet"),
                "function lost while parsing {:?}: {}",
                from,
                output
            );
            assert!(
                output.contains("console.log"),
                "print lost while parsing {:?}: {}",
                from,
                output
            );
            assert!(
                output.contains("greet(\"world\")"),
                "entry point lost while parsing {:?}: {}",
                from,
                output
            );
        }
    }

    #[test]
    fn emitted_programs_pass_installed_language_compilers() {
        let root =
            std::env::temp_dir().join(format!("translatecode-engine-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        let source = fixture(Language::Python);
        let cases = [
            (
                Language::JavaScript,
                "program.js",
                "node",
                vec!["--check", "program.js"],
            ),
            (
                Language::Python,
                "program.py",
                "python3",
                vec!["-m", "py_compile", "program.py"],
            ),
            (
                Language::Java,
                "TranslatedProgram.java",
                "javac",
                vec!["TranslatedProgram.java"],
            ),
            (
                Language::Dart,
                "program.dart",
                "dart",
                vec!["analyze", "program.dart"],
            ),
            (
                Language::Swift,
                "program.swift",
                "swiftc",
                vec!["-parse", "program.swift"],
            ),
            (Language::Go, "program.go", "go", vec!["run", "program.go"]),
            (
                Language::Rust,
                "program.rs",
                "rustc",
                vec!["program.rs", "-o", "program-rust"],
            ),
        ];
        for (target, file_name, compiler, args) in cases {
            fs::write(
                root.join(file_name),
                translate(source, Language::Python, target),
            )
            .unwrap();
            let result = match Command::new(compiler)
                .args(args)
                .current_dir(&root)
                .output()
            {
                Ok(result) => result,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => panic!("failed to launch {}: {}", compiler, error),
            };
            let stderr = String::from_utf8_lossy(&result.stderr);
            if compiler == "javac" && stderr.contains("Unable to locate a Java Runtime") {
                continue;
            }
            if stderr.contains("Operation not permitted") {
                continue;
            }
            assert!(
                result.status.success(),
                "{} rejected generated {}:\n{}",
                compiler,
                file_name,
                stderr
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn every_language_pair_passes_its_target_compiler() {
        fn slug(language: Language) -> &'static str {
            match language {
                Language::JavaScript => "javascript",
                Language::Java => "java",
                Language::Dart => "dart",
                Language::Swift => "swift",
                Language::Python => "python",
                Language::Go => "go",
                Language::Rust => "rust",
            }
        }
        let root =
            std::env::temp_dir().join(format!("translatecode-matrix-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();

        for from in LANGUAGES {
            for target in LANGUAGES {
                let pair = root.join(format!("{}-{}", slug(from), slug(target)));
                fs::create_dir_all(&pair).unwrap();
                let (file_name, compiler, arguments): (&str, &str, Vec<&str>) = match target {
                    Language::JavaScript => ("program.js", "node", vec!["--check", "program.js"]),
                    Language::Java => (
                        "TranslatedProgram.java",
                        "/opt/homebrew/opt/openjdk/bin/javac",
                        vec!["-Werror", "-Xlint:all", "TranslatedProgram.java"],
                    ),
                    Language::Dart => (
                        "program.dart",
                        "/opt/homebrew/share/flutter/bin/cache/dart-sdk/bin/dart",
                        vec!["analyze", "--fatal-infos", "program.dart"],
                    ),
                    Language::Swift => (
                        "program.swift",
                        "swiftc",
                        vec!["-warnings-as-errors", "-parse", "program.swift"],
                    ),
                    Language::Python => (
                        "program.py",
                        "python3",
                        vec!["-m", "py_compile", "program.py"],
                    ),
                    Language::Go => ("program.go", "go", vec!["run", "program.go"]),
                    Language::Rust => (
                        "program.rs",
                        "rustc",
                        vec![
                            "--edition=2024",
                            "-Dwarnings",
                            "program.rs",
                            "-o",
                            "program",
                        ],
                    ),
                };
                fs::write(pair.join(file_name), translate(fixture(from), from, target)).unwrap();
                let mut command = Command::new(compiler);
                command.args(arguments).current_dir(&pair);
                if target == Language::Dart {
                    command
                        .env("DART_DISABLE_ANALYTICS", "true")
                        .env("CI", "true");
                }
                let result = match command.output() {
                    Ok(result) => result,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => panic!("failed to launch {}: {}", compiler, error),
                };
                assert!(
                    result.status.success(),
                    "{:?} -> {:?} failed:\n{}\nGenerated:\n{}",
                    from,
                    target,
                    String::from_utf8_lossy(&result.stderr),
                    fs::read_to_string(pair.join(file_name)).unwrap()
                );
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn every_language_pair_preserves_observable_output() {
        fn slug(language: Language) -> &'static str {
            match language {
                Language::JavaScript => "javascript",
                Language::Java => "java",
                Language::Dart => "dart",
                Language::Swift => "swift",
                Language::Python => "python",
                Language::Go => "go",
                Language::Rust => "rust",
            }
        }
        let root = std::env::temp_dir().join(format!(
            "translatecode-runtime-matrix-{}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();

        for from in LANGUAGES {
            for target in LANGUAGES {
                for (case, source, expected) in [
                    ("greeting", fixture(from), "Hello, world\n"),
                    ("numeric", numeric_fixture(from), "large\n9\n"),
                    ("boolean", boolean_fixture(from), "active\n"),
                ] {
                    let pair = root.join(format!("{}-{}-{}", case, slug(from), slug(target)));
                    fs::create_dir_all(&pair).unwrap();
                    let (file_name, compile, run): (
                        &str,
                        Option<(&str, Vec<&str>)>,
                        (&str, Vec<&str>),
                    ) = match target {
                        Language::JavaScript => ("program.js", None, ("node", vec!["program.js"])),
                        Language::Java => (
                            "TranslatedProgram.java",
                            Some((
                                "/opt/homebrew/opt/openjdk/bin/javac",
                                vec!["-Werror", "-Xlint:all", "TranslatedProgram.java"],
                            )),
                            (
                                "/opt/homebrew/opt/openjdk/bin/java",
                                vec!["-cp", ".", "TranslatedProgram"],
                            ),
                        ),
                        Language::Dart => (
                            "program.dart",
                            None,
                            (
                                "/opt/homebrew/share/flutter/bin/cache/dart-sdk/bin/dart",
                                vec!["run", "program.dart"],
                            ),
                        ),
                        Language::Swift => (
                            "program.swift",
                            Some((
                                "swiftc",
                                vec![
                                    "-warnings-as-errors",
                                    "-module-cache-path",
                                    ".swift-module-cache",
                                    "program.swift",
                                    "-o",
                                    "program",
                                ],
                            )),
                            ("./program", vec![]),
                        ),
                        Language::Python => ("program.py", None, ("python3", vec!["program.py"])),
                        Language::Go => ("program.go", None, ("go", vec!["run", "program.go"])),
                        Language::Rust => (
                            "program.rs",
                            Some((
                                "rustc",
                                vec![
                                    "--edition=2024",
                                    "-Dwarnings",
                                    "program.rs",
                                    "-o",
                                    "program",
                                ],
                            )),
                            ("./program", vec![]),
                        ),
                    };
                    fs::write(pair.join(file_name), translate(source, from, target)).unwrap();
                    if let Some((compiler, arguments)) = compile {
                        let result = match Command::new(compiler)
                            .args(arguments)
                            .current_dir(&pair)
                            .output()
                        {
                            Ok(result) => result,
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                            Err(error) => panic!("failed to launch {}: {}", compiler, error),
                        };
                        assert!(
                            result.status.success(),
                            "{} {:?} -> {:?} compile failed:\n{}\nGenerated:\n{}",
                            case,
                            from,
                            target,
                            String::from_utf8_lossy(&result.stderr),
                            fs::read_to_string(pair.join(file_name)).unwrap()
                        );
                    }
                    let mut command = Command::new(run.0);
                    command.args(run.1).current_dir(&pair);
                    if target == Language::Dart {
                        command
                            .env("DART_DISABLE_ANALYTICS", "true")
                            .env("CI", "true");
                    }
                    let result = match command.output() {
                        Ok(result) => result,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                        Err(error) => panic!("failed to launch {}: {}", run.0, error),
                    };
                    assert!(
                        result.status.success(),
                        "{} {:?} -> {:?} run failed:\n{}",
                        case,
                        from,
                        target,
                        String::from_utf8_lossy(&result.stderr)
                    );
                    assert_eq!(
                        String::from_utf8_lossy(&result.stdout),
                        expected,
                        "{} {:?} -> {:?} changed behavior",
                        case,
                        from,
                        target
                    );
                }
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
}
