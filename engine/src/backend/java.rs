use crate::backend::{Backend, BackendOutput};
use crate::typed_ir::{
    ClassDeclaration, ClassKind, ClassMember, CompilationUnit, Declaration,
    FunctionDeclaration, Parameter, TypeReference,
};

pub struct JavaBackend;

impl Backend for JavaBackend {
    fn emit(&self, unit: &CompilationUnit) -> BackendOutput {
        let mut sections = vec![
            "import java.util.*;".to_string(),
            "import java.util.concurrent.*;".to_string(),
        ];
        for declaration in &unit.declarations {
            let emitted = match declaration {
                Declaration::Class(value) => emit_class(value, false),
                Declaration::Mixin(value) => emit_class(value, true),
                Declaration::Enum(value) => format!("enum {} {{ {} }}", value.name, value.values.join(", ")),
                Declaration::Extension(value) => format!("final class {}Extensions {{\n    private {}Extensions() {{}}\n}}", value.name, value.name),
                Declaration::TypeAlias(value) => {
                    let params = emit_type_parameters(&value.type_parameters.iter().map(|item| item.name.as_str()).collect::<Vec<_>>());
                    format!("interface {}{} {{ Object apply(Object value); }}", value.name, params)
                }
                Declaration::Function(_) => continue,
            };
            sections.push(emitted);
        }
        let functions = unit.declarations.iter().filter_map(|declaration| match declaration {
            Declaration::Function(value) => Some(emit_top_level_function(value)), _ => None,
        }).collect::<Vec<_>>().join("\n\n");
        sections.push(format!("public final class TranslatedProgram {{\n{}\n}}", indent_text(&functions, 1)));
        BackendOutput { code: sections.join("\n\n"), diagnostics: Vec::new() }
    }
}

fn emit_class(class: &ClassDeclaration, force_interface: bool) -> String {
    let is_interface = force_interface || matches!(class.kind, ClassKind::Interface | ClassKind::AbstractInterface | ClassKind::Mixin);
    let prefix = if is_interface { "interface" }
        else if class.kind == ClassKind::Final { "final class" }
        else if matches!(class.kind, ClassKind::Abstract | ClassKind::Sealed) { "abstract class" }
        else { "class" };
    let generics = emit_type_parameters(&class.type_parameters.iter().map(|item| item.name.as_str()).collect::<Vec<_>>());
    let mut heritage = String::new();
    if is_interface {
        let mut parents = Vec::new();
        if let Some(parent) = &class.extends { parents.push(emit_type(parent)); }
        parents.extend(class.mixins.iter().map(emit_type));
        parents.extend(class.implements.iter().map(emit_type));
        if !parents.is_empty() { heritage = format!(" extends {}", parents.join(", ")); }
    } else {
        if let Some(parent) = &class.extends { heritage.push_str(&format!(" extends {}", emit_type(parent))); }
        let interfaces = class.mixins.iter().chain(class.implements.iter()).map(emit_type).collect::<Vec<_>>();
        if !interfaces.is_empty() { heritage.push_str(&format!(" implements {}", interfaces.join(", "))); }
    }
    let members = class.members.iter().filter_map(|member| emit_member(member, class, is_interface)).collect::<Vec<_>>().join("\n\n");
    format!("{} {}{}{} {{\n{}\n}}", prefix, class.name, generics, heritage, indent_text(&members, 1))
}

fn emit_member(member: &ClassMember, class: &ClassDeclaration, is_interface: bool) -> Option<String> {
    match member {
        ClassMember::Field(field) if !is_interface => {
            let visibility = if field.name.starts_with('_') { "private" } else { "public" };
            let modifiers = format!("{}{}", if field.is_static { " static" } else { "" }, if field.is_final { " final" } else { "" });
            Some(format!("{}{} {} {}{};", visibility, modifiers, emit_type(&field.type_ref), field.name, if field.is_static && field.is_final { " = null" } else { "" }))
        }
        ClassMember::Field(_) => None,
        ClassMember::Method(function) => Some(emit_method(function, is_interface)),
        ClassMember::Getter(function) => Some(emit_accessor(function, true, is_interface)),
        ClassMember::Setter(function) => Some(emit_accessor(function, false, is_interface)),
        ClassMember::Operator(function) => Some(emit_method(function, is_interface)),
        ClassMember::Constructor(constructor) if !is_interface && !constructor.is_factory && constructor.named.is_none() => {
            Some(format!("public {}({}) {{\n{}    throw new UnsupportedOperationException(\"constructor lowering pending\");\n}}", class.name, emit_parameters(&constructor.parameters), if class.extends.is_some() { "    super(null);\n" } else { "" }))
        }
        ClassMember::Constructor(_) | ClassMember::Unlowered { .. } => None,
    }
}

fn emit_method(function: &FunctionDeclaration, is_interface: bool) -> String {
    let generics = emit_type_parameters(&function.type_parameters.iter().map(|item| item.name.as_str()).collect::<Vec<_>>());
    let signature = format!("public {}{}{}{} {}({})", if function.is_static { "static " } else { "" }, if generics.is_empty() { "" } else { &generics }, if generics.is_empty() { "" } else { " " }, emit_type(&function.return_type), sanitize_method_name(&function.name), emit_parameters(&function.parameters));
    if is_interface { format!("{};", signature.trim_start_matches("public ")) }
    else { format!("{} {{\n    throw new UnsupportedOperationException(\"body lowering pending\");\n}}", signature) }
}

fn emit_accessor(function: &FunctionDeclaration, getter: bool, is_interface: bool) -> String {
    let title = function.name.trim_start_matches('_');
    let mut characters = title.chars();
    let capitalized = characters.next().map(|first| first.to_uppercase().collect::<String>() + characters.as_str()).unwrap_or_default();
    let name = format!("{}{}", if getter { "get" } else { "set" }, capitalized);
    let return_type = if getter { emit_type(&function.return_type) } else { "void".into() };
    let signature = format!("{} {}({})", return_type, name, emit_parameters(&function.parameters));
    if is_interface { format!("default {} {{\n    throw new UnsupportedOperationException(\"mixin/accessor lowering pending\");\n}}", signature) } else { format!("public {} {{\n    throw new UnsupportedOperationException(\"accessor lowering pending\");\n}}", signature) }
}

fn emit_top_level_function(function: &FunctionDeclaration) -> String {
    if function.name == "main" { return "public static void main(String[] args) {\n    throw new UnsupportedOperationException(\"main lowering pending\");\n}".into(); }
    format!("public static {} {}({}) {{\n    throw new UnsupportedOperationException(\"body lowering pending\");\n}}", emit_type(&function.return_type), function.name, emit_parameters(&function.parameters))
}

fn emit_parameters(parameters: &[Parameter]) -> String {
    parameters.iter().map(|parameter| format!("{} {}", emit_type(&parameter.type_ref), parameter.name.trim_start_matches('_'))).collect::<Vec<_>>().join(", ")
}

fn emit_type(reference: &TypeReference) -> String {
    if let Some((result, parameters)) = reference.name.split_once(" Function(") {
        let parameter = parameters.trim_end_matches(')').split(',').next().unwrap_or("Object").split_whitespace().next().unwrap_or("Object");
        return format!("java.util.function.Function<{}, {}>", parameter, result.trim());
    }
    let name = match reference.name.as_str() {
        "void" => "void", "bool" => "boolean", "int" => "Long", "double" | "num" => "Double",
        "dynamic" | "Object?" => "Object", "Future" => "CompletableFuture", "Map" => "Map",
        "List" => "List", "Set" => "Set", "Iterable" => "Iterable", other => other,
    };
    if reference.arguments.is_empty() { name.to_string() }
    else { format!("{}<{}>", name, reference.arguments.iter().map(emit_type).collect::<Vec<_>>().join(", ")) }
}

fn emit_type_parameters(parameters: &[&str]) -> String {
    if parameters.is_empty() { String::new() } else { format!("<{}>", parameters.join(", ")) }
}

fn sanitize_method_name(name: &str) -> String {
    if let Some(operator) = name.strip_prefix("operator ") {
        return match operator { "==" => "equals", "+" => "plus", "-" => "minus", "*" => "multiply", "/" => "divide", _ => "operator" }.into();
    }
    name.to_string()
}

fn indent_text(value: &str, levels: usize) -> String {
    let pad = "    ".repeat(levels);
    value.lines().map(|line| if line.is_empty() { String::new() } else { format!("{}{}", pad, line) }).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::dart::DartFrontend;
    use crate::frontend::Frontend;

    const COMPREHENSIVE_DART: &str = include_str!("../../../tests/fixtures/comprehensive.dart");

    #[test]
    fn preserves_dart_oop_structure_in_java_scaffold() {
        let unit = DartFrontend.parse(COMPREHENSIVE_DART);
        let output = JavaBackend.emit(&unit).code;
        assert!(output.contains("enum Role { admin, member, guest }"));
        assert!(output.contains("interface Greeter<T>"));
        assert!(output.contains("class Entity"));
        assert!(output.contains("final class User extends Entity implements Timestamped, Greeter<String>"));
        assert!(output.contains("private String _name;"));
        assert!(output.contains("public String greet(String prefix)"));
        assert!(output.contains("public User(Object id, String name, Role role, List<String> tags)"), "{}", output);
    }
}
