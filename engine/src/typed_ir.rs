use crate::diagnostic::{Diagnostic, SourceSpan};

#[derive(Clone, Debug, Default)]
pub struct CompilationUnit {
    pub declarations: Vec<Declaration>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug)]
pub enum Declaration {
    Class(ClassDeclaration),
    Mixin(ClassDeclaration),
    Enum(EnumDeclaration),
    Extension(ExtensionDeclaration),
    TypeAlias(TypeAliasDeclaration),
    Function(FunctionDeclaration),
}

impl Declaration {
    pub fn name(&self) -> &str {
        match self {
            Self::Class(value) | Self::Mixin(value) => &value.name,
            Self::Enum(value) => &value.name,
            Self::Extension(value) => &value.name,
            Self::TypeAlias(value) => &value.name,
            Self::Function(value) => &value.name,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClassKind {
    #[default]
    Class,
    Abstract,
    Interface,
    AbstractInterface,
    Base,
    Final,
    Sealed,
    Mixin,
}

#[derive(Clone, Debug, Default)]
pub struct ClassDeclaration {
    pub name: String,
    pub kind: ClassKind,
    pub type_parameters: Vec<TypeParameter>,
    pub extends: Option<TypeReference>,
    pub mixins: Vec<TypeReference>,
    pub implements: Vec<TypeReference>,
    pub members: Vec<ClassMember>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug)]
pub enum ClassMember {
    Field(FieldDeclaration),
    Method(FunctionDeclaration),
    Constructor(ConstructorDeclaration),
    Getter(FunctionDeclaration),
    Setter(FunctionDeclaration),
    Operator(FunctionDeclaration),
    Unlowered { syntax_kind: String, span: SourceSpan },
}

#[derive(Clone, Debug, Default)]
pub struct FieldDeclaration {
    pub name: String,
    pub type_ref: TypeReference,
    pub is_static: bool,
    pub is_final: bool,
    pub initializer: Option<UnloweredBody>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Default)]
pub struct ConstructorDeclaration {
    pub class_name: String,
    pub named: Option<String>,
    pub parameters: Vec<Parameter>,
    pub is_const: bool,
    pub is_factory: bool,
    pub body: Option<UnloweredBody>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Default)]
pub struct FunctionDeclaration {
    pub name: String,
    pub return_type: TypeReference,
    pub type_parameters: Vec<TypeParameter>,
    pub parameters: Vec<Parameter>,
    pub is_async: bool,
    pub is_static: bool,
    pub body: Option<UnloweredBody>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Default)]
pub struct Parameter {
    pub name: String,
    pub type_ref: TypeReference,
    pub kind: ParameterKind,
    pub is_required: bool,
    pub default_value: Option<UnloweredBody>,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParameterKind { #[default] Positional, OptionalPositional, Named }

#[derive(Clone, Debug, Default)]
pub struct EnumDeclaration {
    pub name: String,
    pub values: Vec<String>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Default)]
pub struct ExtensionDeclaration {
    pub name: String,
    pub on_type: TypeReference,
    pub members: Vec<ClassMember>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Default)]
pub struct TypeAliasDeclaration {
    pub name: String,
    pub type_parameters: Vec<TypeParameter>,
    pub aliased_type: TypeReference,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Default)]
pub struct TypeParameter {
    pub name: String,
    pub bound: Option<TypeReference>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypeReference {
    pub name: String,
    pub arguments: Vec<TypeReference>,
    pub nullable: bool,
}

impl TypeReference {
    pub fn dynamic() -> Self { Self { name: "dynamic".into(), arguments: Vec::new(), nullable: false } }
}

#[derive(Clone, Debug, Default)]
pub struct UnloweredBody {
    pub source: String,
    pub syntax_kind: String,
    pub span: SourceSpan,
}
