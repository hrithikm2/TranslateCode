use std::mem;
use std::sync::Mutex;

pub mod diagnostic;
pub mod backend;
pub mod frontend;
pub mod typed_ir;

static OUTPUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language { JavaScript, Java, Dart, Swift, Python, Go, Rust }

impl Language {
    pub fn from_id(id: u32) -> Self {
        match id { 1 => Self::Java, 2 => Self::Dart, 3 => Self::Swift, 4 => Self::Python, 5 => Self::Go, 6 => Self::Rust, _ => Self::JavaScript }
    }
}

#[derive(Clone, Debug, Default)]
struct Program { body: Vec<Statement> }

#[derive(Clone, Debug)]
enum Statement {
    Variable { name: String, value: String, mutable: bool, type_hint: Option<String> },
    Function { name: String, params: Vec<Parameter>, return_type: Option<String>, body: Vec<Statement> },
    If { condition: String, then_body: Vec<Statement>, else_body: Vec<Statement> },
    Print { values: Vec<String> },
    Return(Option<String>),
    Expression(String),
}

#[derive(Clone, Debug)]
struct Parameter { name: String, type_hint: Option<String> }

fn trim_end_tokens(value: &str) -> String {
    value.trim().trim_end_matches(';').trim_end_matches('{').trim().to_string()
}

fn strip_wrapping_parens(value: &str) -> String {
    let text = value.trim();
    if text.starts_with('(') && text.ends_with(')') { text[1..text.len()-1].trim().to_string() } else { text.to_string() }
}

fn split_args(value: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut quote = '\0';
    for ch in value.chars() {
        if quote != '\0' {
            current.push(ch);
            if ch == quote { quote = '\0'; }
        } else {
            match ch {
                '\'' | '"' => { quote = ch; current.push(ch); },
                '(' | '[' | '{' => { depth += 1; current.push(ch); },
                ')' | ']' | '}' => { depth -= 1; current.push(ch); },
                ',' if depth == 0 => { result.push(current.trim().to_string()); current.clear(); },
                _ => current.push(ch),
            }
        }
    }
    if !current.trim().is_empty() { result.push(current.trim().to_string()); }
    result
}

fn canonical_type(value: &str) -> Option<String> {
    let clean = value.trim().trim_start_matches('&').trim_end_matches('?').trim();
    let result = match clean {
        "str" | "String" | "string" => "string",
        "int" | "Int" | "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" => "int",
        "float" | "double" | "Float" | "Double" | "f32" | "f64" => "float",
        "bool" | "boolean" | "Boolean" | "Bool" => "bool",
        "void" | "Void" | "()" => "void",
        "dynamic" | "Object" | "Any" | "interface{}" => "any",
        _ => return None,
    };
    Some(result.to_string())
}

fn infer_type(expression: &str) -> String {
    let value = expression.trim();
    if value == "true" || value == "false" || value == "True" || value == "False" { "bool".into() }
    else if value.starts_with('"') || value.starts_with('\'') || value.contains("format!(") { "string".into() }
    else if value.parse::<i64>().is_ok() { "int".into() }
    else if value.parse::<f64>().is_ok() { "float".into() }
    else { "any".into() }
}

fn source_expression(expression: &str, language: Language) -> String {
    let mut value = expression.trim().to_string();
    if language == Language::Rust {
        value = value.replace(".to_string()", "").replace(" + &", " + ");
    }
    if language == Language::Swift { value = value.replace("nil", "null"); }
    value
}

fn parse_parameter(raw: &str, language: Language) -> Parameter {
    let text = raw.trim().trim_start_matches('_').trim();
    if text.is_empty() { return Parameter { name: String::new(), type_hint: None }; }
    match language {
        Language::Java | Language::Dart => {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() >= 2 { Parameter { name: parts.last().unwrap().trim().to_string(), type_hint: canonical_type(parts[parts.len()-2]) } }
            else { Parameter { name: text.to_string(), type_hint: None } }
        }
        Language::Swift | Language::Rust => {
            if let Some((name, ty)) = text.split_once(':') { Parameter { name: name.trim().to_string(), type_hint: canonical_type(ty) } }
            else { Parameter { name: text.to_string(), type_hint: None } }
        }
        Language::Go => {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() >= 2 { Parameter { name: parts[0].to_string(), type_hint: canonical_type(parts[1]) } }
            else { Parameter { name: text.to_string(), type_hint: None } }
        }
        _ => Parameter { name: text.to_string(), type_hint: None },
    }
}

fn parse_function_header(text: &str, language: Language) -> Option<(String, Vec<Parameter>, Option<String>)> {
    let line = trim_end_tokens(text);
    let open = line.find('(')?;
    let close = line.rfind(')')?;
    if close < open { return None; }
    let before = line[..open].trim();
    let args = &line[open+1..close];
    let after = line[close+1..].trim();
    let name = match language {
        Language::Python => before.strip_prefix("def ")?.trim(),
        Language::JavaScript => before.strip_prefix("function ")?.trim(),
        Language::Swift => before.strip_prefix("func ")?.trim(),
        Language::Go => before.strip_prefix("func ")?.trim(),
        Language::Rust => before.strip_prefix("pub ").unwrap_or(before).strip_prefix("fn ")?.trim(),
        Language::Java | Language::Dart => {
            if before.starts_with("if ") || before == "if" || before.starts_with("for ") || before.starts_with("while ") { return None; }
            before.split_whitespace().last()?
        }
    };
    if name.is_empty() || ["if", "for", "while", "switch", "print", "println"].contains(&name) { return None; }
    let params = split_args(args).iter().map(|arg| parse_parameter(arg, language)).filter(|p| !p.name.is_empty()).collect();
    let return_type = match language {
        Language::Swift | Language::Rust => after.split_once("->").and_then(|(_, ty)| canonical_type(ty)),
        Language::Go => canonical_type(after),
        Language::Java | Language::Dart => before.split_whitespace().rev().nth(1).and_then(canonical_type),
        _ => None,
    };
    Some((name.to_string(), params, return_type))
}

fn parse_print(text: &str, language: Language) -> Option<Vec<String>> {
    let names: &[&str] = match language {
        Language::JavaScript => &["console.log"], Language::Java => &["System.out.println", "System.out.print"],
        Language::Dart | Language::Swift | Language::Python => &["print"], Language::Go => &["fmt.Println", "fmt.Print"],
        Language::Rust => &["println!", "print!"],
    };
    for name in names {
        let prefix = format!("{}(", name);
        if text.trim().starts_with(&prefix) {
            let inner = text.trim()[prefix.len()..].trim_end_matches(';').trim_end_matches(')').trim();
            let mut values = split_args(inner);
            if language == Language::Rust && values.first().map(|v| v.contains("{}") || v.contains("{:?")).unwrap_or(false) { values.remove(0); }
            return Some(values.into_iter().map(|value| source_expression(&value, language)).collect());
        }
    }
    None
}

fn parse_variable(text: &str, language: Language) -> Option<Statement> {
    let line = text.trim().trim_end_matches(';');
    let (left, value) = line.split_once('=')?;
    if ["==", ">=", "<=", "!="].iter().any(|op| line.contains(op)) { return None; }
    let left = left.trim();
    let value = source_expression(value, language);
    let mut mutable = true;
    let (name, type_hint) = match language {
        Language::Python => {
            if left.contains(' ') { return None; }
            (left.to_string(), None)
        }
        Language::JavaScript => {
            let declaration = ["const ", "let ", "var "].iter().find(|prefix| left.starts_with(**prefix))?;
            mutable = *declaration != "const ";
            (left.trim_start_matches(*declaration).trim().to_string(), None)
        }
        Language::Rust => {
            let rest = left.strip_prefix("let ")?;
            mutable = rest.starts_with("mut ");
            let rest = rest.trim_start_matches("mut ").trim();
            if let Some((name, ty)) = rest.split_once(':') { (name.trim().to_string(), canonical_type(ty)) } else { (rest.to_string(), None) }
        }
        Language::Go => {
            if left.ends_with(':') { (left.trim_end_matches(':').trim().to_string(), None) }
            else if let Some(rest) = left.strip_prefix("var ") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                (parts.first()?.to_string(), parts.get(1).and_then(|ty| canonical_type(ty)))
            } else { return None; }
        }
        Language::Swift => {
            let (rest, is_mut) = if let Some(v) = left.strip_prefix("var ") { (v, true) } else if let Some(v) = left.strip_prefix("let ") { (v, false) } else { return None };
            mutable = is_mut;
            if let Some((name, ty)) = rest.split_once(':') { (name.trim().to_string(), canonical_type(ty)) } else { (rest.trim().to_string(), None) }
        }
        Language::Dart => {
            let mut rest = left;
            if let Some(v) = rest.strip_prefix("final ") { mutable = false; rest = v; }
            else if let Some(v) = rest.strip_prefix("const ") { mutable = false; rest = v; }
            else if let Some(v) = rest.strip_prefix("var ") { rest = v; }
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 2 { (parts.last()?.to_string(), canonical_type(parts[parts.len()-2])) } else { (rest.to_string(), None) }
        }
        Language::Java => {
            let mut rest = left;
            if let Some(v) = rest.strip_prefix("final ") { mutable = false; rest = v; }
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 2 { return None; }
            (parts.last()?.to_string(), canonical_type(parts[parts.len()-2]))
        }
    };
    Some(Statement::Variable { name, value, mutable, type_hint })
}

fn parse_simple_statement(text: &str, language: Language) -> Option<Statement> {
    let line = text.trim().trim_end_matches(';').trim();
    if line.is_empty() || line.starts_with("//") || line.starts_with('#') || line.starts_with("import ") || line.starts_with("package ") || line.starts_with("use ") { return None; }
    if let Some(values) = parse_print(line, language) { return Some(Statement::Print { values }); }
    if line == "return" { return Some(Statement::Return(None)); }
    if let Some(value) = line.strip_prefix("return ") { return Some(Statement::Return(Some(source_expression(value, language)))); }
    if let Some(variable) = parse_variable(line, language) { return Some(variable); }
    Some(Statement::Expression(source_expression(line, language)))
}

fn indent_of(line: &str) -> usize { line.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 4 } else { 1 }).sum() }

fn parse_python_block(lines: &[String], index: &mut usize, indent: usize) -> Vec<Statement> {
    let mut body = Vec::new();
    while *index < lines.len() {
        let raw = &lines[*index];
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') { *index += 1; continue; }
        let current_indent = indent_of(raw);
        if current_indent < indent { break; }
        if current_indent > indent { *index += 1; continue; }
        let text = raw.trim();
        if text.starts_with("else:") { break; }
        if let Some((name, params, return_type)) = parse_function_header(text, Language::Python) {
            *index += 1;
            let child_indent = lines.get(*index).map(|l| indent_of(l)).filter(|v| *v > indent).unwrap_or(indent + 4);
            let function_body = parse_python_block(lines, index, child_indent);
            body.push(Statement::Function { name, params, return_type, body: function_body });
            continue;
        }
        if text.starts_with("if ") && text.ends_with(':') {
            let condition = source_expression(text[3..text.len()-1].trim(), Language::Python);
            *index += 1;
            let child_indent = lines.get(*index).map(|l| indent_of(l)).filter(|v| *v > indent).unwrap_or(indent + 4);
            let then_body = parse_python_block(lines, index, child_indent);
            let mut else_body = Vec::new();
            if *index < lines.len() && indent_of(&lines[*index]) == indent && lines[*index].trim().starts_with("else:") {
                *index += 1;
                let else_indent = lines.get(*index).map(|l| indent_of(l)).filter(|v| *v > indent).unwrap_or(indent + 4);
                else_body = parse_python_block(lines, index, else_indent);
            }
            body.push(Statement::If { condition, then_body, else_body });
            continue;
        }
        if let Some(statement) = parse_simple_statement(text, Language::Python) { body.push(statement); }
        *index += 1;
    }
    body
}

fn normalize_braces(source: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut quote = '\0';
    let mut escaped = false;
    for ch in source.chars() {
        if quote != '\0' {
            current.push(ch);
            if escaped { escaped = false; }
            else if ch == '\\' { escaped = true; }
            else if ch == quote { quote = '\0'; }
            continue;
        }
        match ch {
            '\'' | '"' => { quote = ch; current.push(ch); },
            '{' => {
                current.push('{');
                if !current.trim().is_empty() { lines.push(current.trim().to_string()); }
                current.clear();
            }
            '}' => {
                if !current.trim().is_empty() { lines.push(current.trim().to_string()); }
                lines.push("}".to_string());
                current.clear();
            }
            ';' => {
                current.push(';');
                if !current.trim().is_empty() { lines.push(current.trim().to_string()); }
                current.clear();
            }
            '\n' | '\r' => {
                if !current.trim().is_empty() { lines.push(current.trim().to_string()); }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() { lines.push(current.trim().to_string()); }
    lines
}

fn extract_condition(text: &str) -> String {
    let line = trim_end_tokens(text);
    let after_if = line.strip_prefix("if").unwrap_or(&line).trim();
    strip_wrapping_parens(after_if)
}

fn parse_brace_block(lines: &[String], index: &mut usize, language: Language, stop_at_close: bool) -> Vec<Statement> {
    let mut body = Vec::new();
    while *index < lines.len() {
        let text = lines[*index].trim();
        if text.starts_with('}') {
            *index += 1;
            if stop_at_close { break; }
            continue;
        }
        if text == "{" { *index += 1; continue; }
        if let Some((name, params, return_type)) = parse_function_header(text, language).filter(|_| text.contains('{')) {
            *index += 1;
            let function_body = parse_brace_block(lines, index, language, true);
            body.push(Statement::Function { name, params, return_type, body: function_body });
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
            body.push(Statement::If { condition, then_body, else_body });
            continue;
        }
        if text.starts_with("else") { if stop_at_close { break; } *index += 1; continue; }
        if text.ends_with('{') && (text.starts_with("class ") || text.contains(" class ") || text.starts_with("public class")) {
            *index += 1;
            body.extend(parse_brace_block(lines, index, language, true));
            continue;
        }
        if let Some(statement) = parse_simple_statement(text, language) { body.push(statement); }
        *index += 1;
    }
    body
}

fn parse(source: &str, language: Language) -> Program {
    let mut program = if language == Language::Python {
        let lines = source.lines().map(|line| line.to_string()).collect::<Vec<_>>();
        let mut index = 0;
        Program { body: parse_python_block(&lines, &mut index, 0) }
    } else {
        let lines = normalize_braces(source);
        let mut index = 0;
        Program { body: parse_brace_block(&lines, &mut index, language, false) }
    };
    if matches!(language, Language::Java | Language::Dart | Language::Go | Language::Rust) {
        let mut normalized = Vec::new();
        for statement in program.body {
            match statement {
                Statement::Function { name, body, .. } if name == "main" => normalized.extend(body),
                other => normalized.push(other),
            }
        }
        program.body = normalized;
    }
    program
}

fn type_for(target: Language, canonical: &str) -> &'static str {
    match target {
        Language::JavaScript | Language::Python => "",
        Language::Java => match canonical { "string" => "String", "int" => "int", "float" => "double", "bool" => "boolean", "void" => "void", _ => "Object" },
        Language::Dart => match canonical { "string" => "String", "int" => "int", "float" => "double", "bool" => "bool", "void" => "void", _ => "dynamic" },
        Language::Swift => match canonical { "string" => "String", "int" => "Int", "float" => "Double", "bool" => "Bool", "void" => "Void", _ => "Any" },
        Language::Go => match canonical { "string" => "string", "int" => "int", "float" => "float64", "bool" => "bool", "void" => "", _ => "any" },
        Language::Rust => match canonical { "string" => "String", "int" => "i64", "float" => "f64", "bool" => "bool", "void" => "()", _ => "String" },
    }
}

fn replace_words(expression: &str, pairs: &[(&str, &str)]) -> String {
    let mut result = expression.to_string();
    for (from, to) in pairs { result = result.replace(from, to); }
    result
}

fn expression_for(target: Language, expression: &str) -> String {
    let mut value = expression.trim().trim_end_matches(';').to_string();
    match target {
        Language::Python => {
            value = value.replace("!=", "__NOT_EQUAL__");
            value = replace_words(&value, &[("true", "True"), ("false", "False"), ("null", "None"), ("&&", "and"), ("||", "or")]);
            value = value.replace('!', "not ");
            value = value.replace("__NOT_EQUAL__", "!=");
        }
        _ => {
            value = replace_words(&value, &[("True", "true"), ("False", "false"), ("None", "null"), (" and ", " && "), (" or ", " || "), ("not ", "!")]);
            if target == Language::Swift { value = value.replace("null", "nil"); }
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
            }
            if target == Language::Rust {
                value = value.replace("null", "None").replace(".to_string()", "");
                if let Some((left, right)) = value.split_once(" + ") {
                    if left.trim().starts_with('"') || right.trim().starts_with('"') {
                        value = format!("format!(\"{{}}{{}}\", {}, {})", left.trim(), right.trim());
                    }
                }
            }
        }
    }
    value
}

fn indent(level: usize) -> String { "    ".repeat(level) }

fn function_return_type(function: &Statement) -> String {
    if let Statement::Function { return_type, body, .. } = function {
        if let Some(ty) = return_type { return ty.clone(); }
        for statement in body {
            if let Statement::Return(Some(value)) = statement { return infer_type(value); }
        }
    }
    "void".into()
}

fn parameter_type(param: &Parameter, body: &[Statement]) -> String {
    if let Some(ty) = &param.type_hint { return ty.clone(); }
    fn search(name: &str, body: &[Statement]) -> Option<String> {
        for statement in body {
            match statement {
                Statement::Variable { value, .. } if value.contains(name) && (value.contains('"') || value.contains('\'')) => return Some("string".into()),
                Statement::Variable { value, .. } if value.contains(name) && [" + ", " - ", " * ", " / "].iter().any(|op| value.contains(op)) => return Some("int".into()),
                Statement::If { condition, then_body, else_body } => {
                    if condition.contains(name) && [">", "<", ">=", "<="].iter().any(|op| condition.contains(op)) { return Some("int".into()); }
                    if let Some(found) = search(name, then_body).or_else(|| search(name, else_body)) { return Some(found); }
                }
                _ => {}
            }
        }
        None
    }
    search(&param.name, body).unwrap_or_else(|| "any".into())
}

fn emit_statement(statement: &Statement, target: Language, level: usize) -> String {
    let pad = indent(level);
    match statement {
        Statement::Variable { name, value, mutable, type_hint } => {
            let expression = expression_for(target, value);
            let inferred = type_hint.clone().unwrap_or_else(|| infer_type(value));
            match target {
                Language::JavaScript => format!("{}{} {} = {};", pad, if *mutable { "let" } else { "const" }, name, expression),
                Language::Python => format!("{}{} = {}", pad, name, expression),
                Language::Java => format!("{}{}{} {} = {};", pad, if *mutable { "" } else { "final " }, type_for(target, &inferred), name, expression),
                Language::Dart => format!("{}{} {} = {};", pad, if *mutable { "var" } else { "final" }, name, expression),
                Language::Swift => format!("{}{} {} = {}", pad, if *mutable { "var" } else { "let" }, name, expression),
                Language::Go => format!("{}{} := {}", pad, name, expression),
                Language::Rust => {
                    let rust_expression = if inferred == "string" && expression.starts_with('"') { format!("{}.to_string()", expression) } else { expression };
                    format!("{}let {}{} = {};", pad, if *mutable { "mut " } else { "" }, name, rust_expression)
                }
            }
        }
        Statement::Print { values } => {
            let converted = values.iter().map(|v| expression_for(target, v)).collect::<Vec<_>>();
            match target {
                Language::JavaScript => format!("{}console.log({});", pad, converted.join(", ")),
                Language::Python | Language::Dart | Language::Swift => format!("{}print({}){}", pad, converted.join(", "), if target == Language::Dart { ";" } else { "" }),
                Language::Java => format!("{}System.out.println({});", pad, if converted.len() > 1 { converted.join(" + \" \" + ") } else { converted.join("") }),
                Language::Go => format!("{}fmt.Println({})", pad, converted.join(", ")),
                Language::Rust => {
                    if converted.is_empty() { format!("{}println!();", pad) }
                    else { format!("{}println!(\"{}\", {});", pad, vec!["{:?}"; converted.len()].join(" "), converted.join(", ")) }
                }
            }
        }
        Statement::Return(value) => match value {
            Some(value) => format!("{}return {}{}", pad, expression_for(target, value), if matches!(target, Language::JavaScript | Language::Java | Language::Dart | Language::Rust) { ";" } else { "" }),
            None => format!("{}return{}", pad, if matches!(target, Language::JavaScript | Language::Java | Language::Dart | Language::Rust) { ";" } else { "" }),
        },
        Statement::Expression(value) => format!("{}{}{}", pad, expression_for(target, value), if matches!(target, Language::JavaScript | Language::Java | Language::Dart | Language::Rust) { ";" } else { "" }),
        Statement::If { condition, then_body, else_body } => {
            let cond = expression_for(target, condition);
            let then_text = then_body.iter().map(|s| emit_statement(s, target, level + 1)).collect::<Vec<_>>().join("\n");
            let else_text = else_body.iter().map(|s| emit_statement(s, target, level + 1)).collect::<Vec<_>>().join("\n");
            match target {
                Language::Python => format!("{}if {}:\n{}{}", pad, cond, if then_text.is_empty() { format!("{}pass", indent(level+1)) } else { then_text }, if else_body.is_empty() { String::new() } else { format!("\n{}else:\n{}", pad, else_text) }),
                _ => format!("{}if ({}) {{\n{}\n{}}}{}", pad, cond, then_text, pad, if else_body.is_empty() { String::new() } else { format!(" else {{\n{}\n{}}}", else_text, pad) }),
            }
        }
        Statement::Function { name, params, body, .. } => {
            let rendered_body = body.iter().map(|s| emit_statement(s, target, level + 1)).collect::<Vec<_>>().join("\n");
            let return_type = function_return_type(statement);
            let rendered_params = params.iter().map(|p| {
                let ty = parameter_type(p, body);
                match target {
                    Language::JavaScript | Language::Python => p.name.clone(),
                    Language::Java | Language::Dart => format!("{} {}", type_for(target, &ty), p.name),
                    Language::Swift => format!("_ {}: {}", p.name, type_for(target, &ty)),
                    Language::Go => format!("{} {}", p.name, type_for(target, &ty)),
                    Language::Rust => format!("{}: {}", p.name, if ty == "string" { "&str" } else { type_for(target, &ty) }),
                }
            }).collect::<Vec<_>>().join(", ");
            match target {
                Language::JavaScript => format!("{}function {}({}) {{\n{}\n{}}}", pad, name, rendered_params, rendered_body, pad),
                Language::Python => format!("{}def {}({}):\n{}", pad, name, rendered_params, if rendered_body.is_empty() { format!("{}pass", indent(level+1)) } else { rendered_body }),
                Language::Java => format!("{}public static {} {}({}) {{\n{}\n{}}}", pad, type_for(target, &return_type), name, rendered_params, rendered_body, pad),
                Language::Dart => format!("{}{} {}({}) {{\n{}\n{}}}", pad, type_for(target, &return_type), name, rendered_params, rendered_body, pad),
                Language::Swift => format!("{}func {}({}){} {{\n{}\n{}}}", pad, name, rendered_params, if return_type == "void" { String::new() } else { format!(" -> {}", type_for(target, &return_type)) }, rendered_body, pad),
                Language::Go => format!("{}func {}({}){} {{\n{}\n{}}}", pad, name, rendered_params, if return_type == "void" { String::new() } else { format!(" {}", type_for(target, &return_type)) }, rendered_body, pad),
                Language::Rust => format!("{}fn {}({}){} {{\n{}\n{}}}", pad, name, rendered_params, if return_type == "void" { String::new() } else { format!(" -> {}", type_for(target, &return_type)) }, rendered_body, pad),
            }
        }
    }
}

fn emit(program: &Program, target: Language) -> String {
    let functions = program.body.iter().filter(|s| matches!(s, Statement::Function { .. })).collect::<Vec<_>>();
    let top_level = program.body.iter().filter(|s| !matches!(s, Statement::Function { .. })).collect::<Vec<_>>();
    match target {
        Language::JavaScript | Language::Python | Language::Swift => program.body.iter().map(|s| emit_statement(s, target, 0)).collect::<Vec<_>>().join("\n\n"),
        Language::Java => {
            let fn_text = functions.iter().map(|s| emit_statement(s, target, 1)).collect::<Vec<_>>().join("\n\n");
            let main = top_level.iter().map(|s| emit_statement(s, target, 2)).collect::<Vec<_>>().join("\n");
            format!("public class TranslatedProgram {{\n{}{}{}\n    public static void main(String[] args) {{\n{}\n    }}\n}}",
                if fn_text.is_empty() { "" } else { "\n" }, fn_text, if fn_text.is_empty() { "" } else { "\n" }, main)
        }
        Language::Dart => {
            let fn_text = functions.iter().map(|s| emit_statement(s, target, 0)).collect::<Vec<_>>().join("\n\n");
            let main = top_level.iter().map(|s| emit_statement(s, target, 1)).collect::<Vec<_>>().join("\n");
            format!("{}{}void main() {{\n{}\n}}", fn_text, if fn_text.is_empty() { "" } else { "\n\n" }, main)
        }
        Language::Go => {
            let fn_text = functions.iter().map(|s| emit_statement(s, target, 0)).collect::<Vec<_>>().join("\n\n");
            let main = top_level.iter().map(|s| emit_statement(s, target, 1)).collect::<Vec<_>>().join("\n");
            format!("package main\n\nimport \"fmt\"\n\n{}{}func main() {{\n{}\n}}", fn_text, if fn_text.is_empty() { "" } else { "\n\n" }, main)
        }
        Language::Rust => {
            let fn_text = functions.iter().map(|s| emit_statement(s, target, 0)).collect::<Vec<_>>().join("\n\n");
            let main = top_level.iter().map(|s| emit_statement(s, target, 1)).collect::<Vec<_>>().join("\n");
            format!("{}{}fn main() {{\n{}\n}}", fn_text, if fn_text.is_empty() { "" } else { "\n\n" }, main)
        }
    }
}

pub fn translate(source: &str, from: Language, to: Language) -> String { emit(&parse(source, from), to) }

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
pub extern "C" fn output_ptr() -> *const u8 { OUTPUT.lock().unwrap().as_ptr() }

#[no_mangle]
pub extern "C" fn output_len() -> usize { OUTPUT.lock().unwrap().len() }

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    const LANGUAGES: [Language; 7] = [Language::JavaScript, Language::Java, Language::Dart, Language::Swift, Language::Python, Language::Go, Language::Rust];

    fn fixture(language: Language) -> &'static str {
        match language {
            Language::JavaScript => "function greet(name) {\n  const message = \"Hello, \" + name;\n  if (name != \"\") { console.log(message); } else { console.log(\"Hello\"); }\n}\ngreet(\"world\");",
            Language::Java => "public class Demo {\npublic static void greet(String name) {\nString message = \"Hello, \" + name;\nif (name != \"\") {\nSystem.out.println(message);\n} else {\nSystem.out.println(\"Hello\");\n}\n}\ngreet(\"world\");\n}",
            Language::Dart => "void greet(String name) {\nfinal message = \"Hello, \" + name;\nif (name != \"\") {\nprint(message);\n} else {\nprint(\"Hello\");\n}\n}\ngreet(\"world\");",
            Language::Swift => "func greet(_ name: String) {\nlet message = \"Hello, \" + name\nif (name != \"\") {\nprint(message)\n} else {\nprint(\"Hello\")\n}\n}\ngreet(\"world\")",
            Language::Python => "def greet(name):\n    message = \"Hello, \" + name\n    if name != \"\":\n        print(message)\n    else:\n        print(\"Hello\")\n\ngreet(\"world\")",
            Language::Go => "package main\nimport \"fmt\"\nfunc greet(name string) {\nmessage := \"Hello, \" + name\nif name != \"\" {\nfmt.Println(message)\n} else {\nfmt.Println(\"Hello\")\n}\n}\nfunc main() { greet(\"world\") }",
            Language::Rust => "fn greet(name: String) {\nlet message = \"Hello, \".to_string() + &name;\nif name != \"\" {\nprintln!(\"{}\", message);\n} else {\nprintln!(\"Hello\");\n}\n}\nfn main() { greet(\"world\".to_string()); }",
        }
    }

    #[test]
    fn every_language_pair_produces_real_code() {
        for from in LANGUAGES {
            for to in LANGUAGES {
                let output = translate(fixture(from), from, to);
                assert!(!output.trim().is_empty(), "empty output for {:?} -> {:?}", from, to);
                assert!(!output.contains("preview"));
            }
        }
    }

    #[test]
    fn python_function_reaches_ir_and_javascript() {
        let output = translate(fixture(Language::Python), Language::Python, Language::JavaScript);
        assert!(output.contains("function greet(name)"));
        assert!(output.contains("console.log(message);"));
        assert!(output.contains("if (name != \"\")"));
    }

    #[test]
    fn every_source_parser_preserves_the_example_program() {
        for from in LANGUAGES {
            let output = translate(fixture(from), from, Language::JavaScript);
            assert!(output.contains("function greet"), "function lost while parsing {:?}: {}", from, output);
            assert!(output.contains("console.log"), "print lost while parsing {:?}: {}", from, output);
            assert!(output.contains("greet(\"world\")"), "entry point lost while parsing {:?}: {}", from, output);
        }
    }

    #[test]
    fn emitted_programs_pass_installed_language_compilers() {
        let root = std::env::temp_dir().join(format!("translatecode-engine-{}", std::process::id()));
        if root.exists() { fs::remove_dir_all(&root).unwrap(); }
        fs::create_dir_all(&root).unwrap();
        let source = fixture(Language::Python);
        let cases = [
            (Language::JavaScript, "program.js", "node", vec!["--check", "program.js"]),
            (Language::Python, "program.py", "python3", vec!["-m", "py_compile", "program.py"]),
            (Language::Java, "TranslatedProgram.java", "javac", vec!["TranslatedProgram.java"]),
            (Language::Dart, "program.dart", "dart", vec!["analyze", "program.dart"]),
            (Language::Swift, "program.swift", "swiftc", vec!["-parse", "program.swift"]),
            (Language::Go, "program.go", "go", vec!["run", "program.go"]),
            (Language::Rust, "program.rs", "rustc", vec!["program.rs", "-o", "program-rust"]),
        ];
        for (target, file_name, compiler, args) in cases {
            fs::write(root.join(file_name), translate(source, Language::Python, target)).unwrap();
            let result = match Command::new(compiler).args(args).current_dir(&root).output() {
                Ok(result) => result,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => panic!("failed to launch {}: {}", compiler, error),
            };
            let stderr = String::from_utf8_lossy(&result.stderr);
            if compiler == "javac" && stderr.contains("Unable to locate a Java Runtime") { continue; }
            if stderr.contains("Operation not permitted") { continue; }
            assert!(result.status.success(), "{} rejected generated {}:\n{}", compiler, file_name, stderr);
        }
        fs::remove_dir_all(root).unwrap();
    }
}
