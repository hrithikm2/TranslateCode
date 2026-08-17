use tree_sitter::{Node, Parser};

use crate::diagnostic::{Diagnostic, Severity, SourcePosition, SourceSpan};
use crate::frontend::Frontend;
use crate::typed_ir::{
    ClassDeclaration, ClassKind, ClassMember, CompilationUnit, ConstructorDeclaration,
    Declaration, EnumDeclaration, ExtensionDeclaration, FieldDeclaration,
    FunctionDeclaration, Parameter, ParameterKind, TypeAliasDeclaration, TypeParameter,
    TypeReference, UnloweredBody,
};

pub struct DartFrontend;

impl Frontend for DartFrontend {
    fn parse(&self, source: &str) -> CompilationUnit {
        let mut parser = Parser::new();
        if parser.set_language(&tree_sitter_dart::LANGUAGE.into()).is_err() {
            return CompilationUnit { declarations: Vec::new(), diagnostics: vec![Diagnostic {
                code: "DART0001", severity: Severity::Error,
                message: "Unable to initialize the Dart parser".into(), span: SourceSpan::default(),
            }] };
        }
        let Some(tree) = parser.parse(source, None) else {
            return CompilationUnit { declarations: Vec::new(), diagnostics: vec![Diagnostic {
                code: "DART0002", severity: Severity::Error,
                message: "The Dart parser did not produce a syntax tree".into(), span: SourceSpan::default(),
            }] };
        };
        let root = tree.root_node();
        let mut unit = CompilationUnit::default();
        collect_syntax_errors(root, source, &mut unit.diagnostics);
        let mut cursor = root.walk();
        for node in root.named_children(&mut cursor) {
            if let Some(declaration) = lower_top_level_declaration(node, source) {
                unit.declarations.push(declaration);
            }
        }
        unit
    }
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
    let kind = if is_mixin { ClassKind::Mixin }
        else if header.contains("abstract interface class") { ClassKind::AbstractInterface }
        else if header.contains("abstract class") { ClassKind::Abstract }
        else if header.contains("interface class") { ClassKind::Interface }
        else if header.contains("base class") { ClassKind::Base }
        else if header.contains("final class") { ClassKind::Final }
        else if header.contains("sealed class") { ClassKind::Sealed }
        else { ClassKind::Class };
    let name = field_text(node, "name", source).unwrap_or_default();
    let type_parameters = node.child_by_field_name("type_parameters").map(|child| lower_type_parameters(child, source)).unwrap_or_default();
    let superclass = node.child_by_field_name("superclass").map(|child| node_text(child, source).to_string()).unwrap_or_default();
    let interfaces = node.child_by_field_name("interfaces").map(|child| node_text(child, source).to_string()).unwrap_or_default();
    let extends = clause_after(&superclass, "extends", "with").and_then(|value| parse_type_reference(&value));
    let mixins = clause_from(&superclass, "with", "implements").map(|value| parse_type_list(&value)).unwrap_or_default();
    let implements = interfaces.strip_prefix("implements").map(parse_type_list).unwrap_or_default();
    let mut members = node.child_by_field_name("body").map(|body| lower_members(body, source, &name)).unwrap_or_default();
    resolve_constructor_parameter_types(&mut members);
    ClassDeclaration { name, kind, type_parameters, extends, mixins, implements, members, span: span(node) }
}

fn lower_enum(node: Node<'_>, source: &str) -> EnumDeclaration {
    let name = field_text(node, "name", source).unwrap_or_default();
    let mut values = Vec::new();
    walk_named(node, &mut |child| {
        if child.kind() == "enum_constant" {
            if let Some(value) = field_text(child, "name", source) { values.push(value); }
        }
    });
    EnumDeclaration { name, values, span: span(node) }
}

fn lower_extension(node: Node<'_>, source: &str) -> ExtensionDeclaration {
    let name = field_text(node, "name", source).unwrap_or_default();
    let on_type = node.child_by_field_name("class").and_then(|child| parse_type_reference(node_text(child, source))).unwrap_or_else(TypeReference::dynamic);
    let members = node.child_by_field_name("body").map(|body| lower_members(body, source, "")).unwrap_or_default();
    ExtensionDeclaration { name, on_type, members, span: span(node) }
}

fn lower_members(body: Node<'_>, source: &str, class_name: &str) -> Vec<ClassMember> {
    let mut members = Vec::new();
    let mut cursor = body.walk();
    for class_member in body.named_children(&mut cursor) {
        if class_member.kind() != "class_member" { continue; }
        if let Some(method) = find_first(class_member, "method_declaration") {
            if let Some(member) = lower_method_member(method, source, class_name) { members.push(member); }
            continue;
        }
        let Some(declaration) = find_first(class_member, "declaration") else { continue; };
        if let Some(signature) = ["constructor_signature", "constant_constructor_signature"]
            .iter().find_map(|kind| find_first(declaration, kind)) {
            members.push(ClassMember::Constructor(lower_constructor(signature, declaration, source, class_name, false)));
            continue;
        }
        if let Some(signature) = find_first(declaration, "function_signature") {
            members.push(ClassMember::Method(lower_callable(signature, declaration, source)));
            continue;
        }
        members.extend(lower_fields(declaration, source).into_iter().map(ClassMember::Field));
    }
    members
}

fn lower_method_member(node: Node<'_>, source: &str, class_name: &str) -> Option<ClassMember> {
    let signature = find_first(node, "method_signature")?;
    if let Some(factory) = find_first(signature, "factory_constructor_signature") {
        return Some(ClassMember::Constructor(lower_constructor(factory, node, source, class_name, true)));
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
        return Some(ClassMember::Operator(lower_callable(operator, node, source)));
    }
    Some(ClassMember::Unlowered { syntax_kind: signature.kind().to_string(), span: span(node) })
}

fn lower_callable(signature: Node<'_>, declaration: Node<'_>, source: &str) -> FunctionDeclaration {
    let name = field_text(signature, "name", source)
        .or_else(|| field_text(signature, "operator", source).map(|value| format!("operator {}", value)))
        .unwrap_or_default();
    let return_type = signature.child_by_field_name("return_type")
        .and_then(|node| parse_type_reference(node_text(node, source))).unwrap_or_else(TypeReference::dynamic);
    let body = find_first(declaration, "function_body").map(|node| UnloweredBody {
        source: node_text(node, source).to_string(), syntax_kind: node.kind().to_string(), span: span(node),
    });
    let declaration_text = node_text(declaration, source);
    let type_parameters = find_first(signature, "type_parameters")
        .map(|node| lower_type_parameters(node, source)).unwrap_or_default();
    FunctionDeclaration {
        name, return_type, type_parameters,
        parameters: direct_child_of_kind(signature, "formal_parameter_list").map(|node| lower_parameters(node, source)).unwrap_or_default(),
        is_async: body.as_ref().map(|value| value.source.contains("async")).unwrap_or(false),
        is_static: declaration_text.trim_start().starts_with("static "), body, span: span(declaration),
    }
}

fn lower_constructor(signature: Node<'_>, declaration: Node<'_>, source: &str, class_name: &str, is_factory: bool) -> ConstructorDeclaration {
    let signature_text = node_text(signature, source);
    let before_parameters = signature_text.split('(').next().unwrap_or(signature_text).trim();
    let constructor_token = before_parameters.split_whitespace().last().unwrap_or(class_name);
    let mut names = constructor_token.split('.');
    let parsed_class = names.next().unwrap_or(class_name);
    let named = names.next().map(str::to_string);
    let body = find_first(declaration, "function_body").map(|node| UnloweredBody {
        source: node_text(node, source).to_string(), syntax_kind: node.kind().to_string(), span: span(node),
    });
    ConstructorDeclaration {
        class_name: if parsed_class.is_empty() { class_name.into() } else { parsed_class.into() },
        named,
        parameters: direct_child_of_kind(signature, "formal_parameter_list").map(|node| lower_parameters(node, source)).unwrap_or_default(),
        is_const: signature.kind() == "constant_constructor_signature",
        is_factory, body, span: span(declaration),
    }
}

fn lower_fields(declaration: Node<'_>, source: &str) -> Vec<FieldDeclaration> {
    let declaration_text = node_text(declaration, source);
    let type_ref = find_first(declaration, "type")
        .and_then(|node| parse_type_reference(node_text(node, source))).unwrap_or_else(TypeReference::dynamic);
    let is_static = declaration_text.trim_start().starts_with("static ") || declaration_text.contains(" static ");
    let is_final = declaration_text.contains("final ") || declaration_text.contains("const ");
    let mut fields = Vec::new();
    walk_named(declaration, &mut |node| {
        if !matches!(node.kind(), "initialized_identifier" | "static_final_declaration") { return; }
        let Some(name) = field_text(node, "name", source) else { return; };
        let initializer = node.child_by_field_name("value").map(|value| UnloweredBody {
            source: node_text(value, source).to_string(), syntax_kind: value.kind().to_string(), span: span(value),
        });
        fields.push(FieldDeclaration { name, type_ref: type_ref.clone(), is_static, is_final, initializer, span: span(node) });
    });
    fields
}

fn lower_alias(node: Node<'_>, source: &str) -> TypeAliasDeclaration {
    let mut name = String::new();
    let mut aliased_type = TypeReference::dynamic();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "type_identifier" && name.is_empty() { name = node_text(child, source).to_string(); }
        if child.kind() == "type" { aliased_type = parse_type_reference(node_text(child, source)).unwrap_or_else(TypeReference::dynamic); }
    }
    let type_parameters = find_first(node, "type_parameters").map(|child| lower_type_parameters(child, source)).unwrap_or_default();
    TypeAliasDeclaration { name, type_parameters, aliased_type, span: span(node) }
}

fn lower_function(node: Node<'_>, source: &str) -> FunctionDeclaration {
    let signature = find_first(node, "function_signature");
    let name = signature.and_then(|value| field_text(value, "name", source)).unwrap_or_default();
    let return_type = signature.and_then(|value| value.child_by_field_name("return_type"))
        .and_then(|value| parse_type_reference(node_text(value, source))).unwrap_or_else(TypeReference::dynamic);
    let body = find_first(node, "function_body").map(|value| UnloweredBody {
        source: node_text(value, source).to_string(), syntax_kind: value.kind().to_string(), span: span(value),
    });
    let is_async = body.as_ref().map(|value| value.source.contains("async")).unwrap_or(false);
    let type_parameters = signature.and_then(|value| value.child_by_field_name("type_parameters"))
        .map(|value| lower_type_parameters(value, source)).unwrap_or_default();
    let parameters = signature.and_then(|value| direct_child_of_kind(value, "formal_parameter_list"))
        .map(|value| lower_parameters(value, source)).unwrap_or_default();
    FunctionDeclaration { name, return_type, type_parameters, parameters, is_async, is_static: false, body, span: span(node) }
}

fn lower_parameters(node: Node<'_>, source: &str) -> Vec<Parameter> {
    fn collect(node: Node<'_>, source: &str, kind: ParameterKind, output: &mut Vec<Parameter>) {
        if node.kind() == "formal_parameter" {
            let name = field_text(node, "name", source).or_else(|| {
                ["constructor_param", "super_formal_parameter"].iter()
                    .find_map(|kind| find_first(node, kind))
                    .and_then(|value| find_first(value, "identifier"))
                    .map(|value| node_text(value, source).to_string())
            }).unwrap_or_default();
            let type_ref = node.child_by_field_name("type").or_else(|| find_first(node, "type"))
                .and_then(|value| parse_type_reference(node_text(value, source))).unwrap_or_else(TypeReference::dynamic);
            let text = node_text(node, source);
            output.push(Parameter { name, type_ref, kind, is_required: text.trim_start().starts_with("required "), default_value: None, span: span(node) });
            return;
        }
        let next_kind = if node.kind() == "optional_formal_parameters" {
            if node_text(node, source).trim_start().starts_with('{') { ParameterKind::Named } else { ParameterKind::OptionalPositional }
        } else { kind };
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) { collect(child, source, next_kind, output); }
    }
    let mut parameters = Vec::new();
    collect(node, source, ParameterKind::Positional, &mut parameters);
    parameters
}

fn resolve_constructor_parameter_types(members: &mut [ClassMember]) {
    let field_types = members.iter().filter_map(|member| match member {
        ClassMember::Field(field) => Some((field.name.clone(), field.type_ref.clone())), _ => None,
    }).collect::<Vec<_>>();
    for member in members {
        let ClassMember::Constructor(constructor) = member else { continue; };
        for parameter in &mut constructor.parameters {
            if parameter.type_ref.name != "dynamic" { continue; }
            if let Some((_, field_type)) = field_types.iter().find(|(name, _)| name == &parameter.name) {
                parameter.type_ref = field_type.clone();
            }
        }
    }
}

fn lower_type_parameters(node: Node<'_>, source: &str) -> Vec<TypeParameter> {
    let mut parameters = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() != "type_parameter" { continue; }
        let name = field_text(child, "name", source).unwrap_or_default();
        parameters.push(TypeParameter { name, bound: None, span: span(child) });
    }
    parameters
}

fn parse_type_reference(raw: &str) -> Option<TypeReference> {
    let raw = raw.trim();
    if raw.is_empty() { return None; }
    let nullable = raw.ends_with('?');
    let clean = raw.trim_end_matches('?').trim();
    if let Some(open) = clean.find('<') {
        let close = clean.rfind('>')?;
        let name = clean[..open].trim().to_string();
        let arguments = split_top_level(&clean[open+1..close]).iter().filter_map(|value| parse_type_reference(value)).collect();
        Some(TypeReference { name, arguments, nullable })
    } else {
        Some(TypeReference { name: clean.to_string(), arguments: Vec::new(), nullable })
    }
}

fn parse_type_list(raw: &str) -> Vec<TypeReference> {
    split_top_level(raw).iter().filter_map(|value| parse_type_reference(value)).collect()
}

fn split_top_level(raw: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for character in raw.chars() {
        match character {
            '<' | '(' | '[' => { depth += 1; current.push(character); }
            '>' | ')' | ']' => { depth -= 1; current.push(character); }
            ',' if depth == 0 => { values.push(current.trim().to_string()); current.clear(); }
            _ => current.push(character),
        }
    }
    if !current.trim().is_empty() { values.push(current.trim().to_string()); }
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
    node.child_by_field_name(field).map(|child| node_text(child, source).to_string())
}

fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or("")
}

fn find_first<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind { return Some(node); }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = find_first(child, kind) { return Some(found); }
    }
    None
}

fn direct_child_of_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).find(|child| child.kind() == kind);
    found
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
        diagnostics.push(Diagnostic { code: "DART1001", severity: Severity::Error,
            message: format!("Unexpected Dart syntax near `{}`", node_text(node, source).chars().take(32).collect::<String>()), span: span(node) });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) { collect_syntax_errors(child, source, diagnostics); }
}

fn span(node: Node<'_>) -> SourceSpan {
    let start = node.start_position();
    let end = node.end_position();
    SourceSpan {
        start: SourcePosition { byte: node.start_byte(), line: start.row + 1, column: start.column + 1 },
        end: SourcePosition { byte: node.end_byte(), line: end.row + 1, column: end.column + 1 },
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
        let names = unit.declarations.iter().map(Declaration::name).collect::<Vec<_>>();
        assert_eq!(names, vec!["Mapper", "Role", "Timestamped", "Greeter", "Entity", "User", "Result", "Success", "Failure", "IntegerIterableX", "describeResult", "main"]);
    }

    #[test]
    fn preserves_dart_class_relationships_and_modifiers() {
        let unit = DartFrontend.parse(COMPREHENSIVE_DART);
        let user = unit.declarations.iter().find_map(|declaration| match declaration {
            Declaration::Class(value) if value.name == "User" => Some(value), _ => None,
        }).expect("User class missing");
        assert_eq!(user.kind, ClassKind::Final);
        assert_eq!(user.extends.as_ref().map(|value| value.name.as_str()), Some("Entity"));
        assert_eq!(user.mixins.iter().map(|value| value.name.as_str()).collect::<Vec<_>>(), vec!["Timestamped"]);
        assert_eq!(user.implements.iter().map(|value| value.name.as_str()).collect::<Vec<_>>(), vec!["Greeter"]);
        assert_eq!(user.implements[0].arguments[0].name, "String");
        assert_eq!(user.members.iter().filter(|member| matches!(member, ClassMember::Field(_))).count(), 5);
        assert_eq!(user.members.iter().filter(|member| matches!(member, ClassMember::Constructor(_))).count(), 3);
        assert_eq!(user.members.iter().filter(|member| matches!(member, ClassMember::Getter(_))).count(), 3);
        assert_eq!(user.members.iter().filter(|member| matches!(member, ClassMember::Setter(_))).count(), 2);
        assert_eq!(user.members.iter().filter(|member| matches!(member, ClassMember::Operator(_))).count(), 1);
        assert_eq!(user.members.iter().filter(|member| matches!(member, ClassMember::Method(_))).count(), 5);
        let greet = user.members.iter().find_map(|member| match member {
            ClassMember::Method(value) if value.name == "greet" => Some(value), _ => None,
        }).expect("greet method missing");
        assert_eq!(greet.parameters.len(), 1);
        assert_eq!(greet.parameters[0].name, "prefix");
        assert_eq!(greet.parameters[0].type_ref.name, "String");
        let primary_constructor = user.members.iter().find_map(|member| match member {
            ClassMember::Constructor(value) if value.named.is_none() && !value.is_factory => Some(value), _ => None,
        }).expect("primary constructor missing");
        assert_eq!(primary_constructor.parameters.iter().map(|value| value.name.as_str()).collect::<Vec<_>>(), vec!["id", "_name", "role", "tags"]);
        assert_eq!(primary_constructor.parameters[2].kind, ParameterKind::Named);
    }
}
