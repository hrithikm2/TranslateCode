use crate::diagnostic::{Diagnostic, SourceSpan};

#[derive(Clone, Debug, Default)]
pub struct CompilationUnit {
    /// Source comments are retained verbatim with their original spans. They
    /// remain language-neutral IR metadata until a backend selects the target
    /// language's comment syntax.
    pub comments: Vec<Comment>,
    /// Imports are retained separately from declarations because they describe
    /// the source project's external API surface.
    pub imports: Vec<ImportDeclaration>,
    pub declarations: Vec<Declaration>,
    /// Executable source-level statements in their original order. Keeping
    /// these separate from declarations lets script-oriented frontends retain
    /// module initialization without pretending it is a declaration.
    pub top_level_statements: Vec<Statement>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub text: String,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportDeclaration {
    pub uri: String,
    pub prefix: Option<String>,
    pub show: Vec<String>,
    pub hide: Vec<String>,
    pub span: SourceSpan,
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
    Unlowered {
        syntax_kind: String,
        span: SourceSpan,
    },
}

#[derive(Clone, Debug, Default)]
pub struct FieldDeclaration {
    pub name: String,
    pub type_ref: TypeReference,
    pub is_static: bool,
    pub is_final: bool,
    pub initializer: Option<Body>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Default)]
pub struct ConstructorDeclaration {
    pub class_name: String,
    pub named: Option<String>,
    pub parameters: Vec<Parameter>,
    pub is_const: bool,
    pub is_factory: bool,
    pub body: Option<Body>,
    /// Lossless constructor text retained for initializer-list and redirect lowering.
    pub source: String,
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
    pub body: Option<Body>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, Default)]
pub struct Parameter {
    pub name: String,
    pub type_ref: TypeReference,
    pub kind: ParameterKind,
    pub is_required: bool,
    pub default_value: Option<Expression>,
    pub span: SourceSpan,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParameterKind {
    #[default]
    Positional,
    OptionalPositional,
    Named,
}

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
    pub fn dynamic() -> Self {
        Self {
            name: "dynamic".into(),
            arguments: Vec::new(),
            nullable: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Body {
    pub kind: BodyKind,
    pub source: String,
    pub syntax_kind: String,
    pub span: SourceSpan,
}

impl Default for Body {
    fn default() -> Self {
        Self {
            kind: BodyKind::Empty,
            source: String::new(),
            syntax_kind: String::new(),
            span: SourceSpan::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum BodyKind {
    Empty,
    Block(Vec<Statement>),
    Expression(Expression),
    Unlowered,
}

#[derive(Clone, Debug)]
pub struct Statement {
    pub kind: StatementKind,
    pub source: String,
    pub span: SourceSpan,
}

#[derive(Clone, Debug)]
pub enum StatementKind {
    Block(Vec<Statement>),
    Variable {
        name: String,
        type_ref: TypeReference,
        is_final: bool,
        initializer: Option<Expression>,
    },
    Expression(Expression),
    If {
        condition: Expression,
        then_branch: Box<Statement>,
        else_branch: Option<Box<Statement>>,
    },
    ForEach {
        variable: String,
        iterable: Expression,
        body: Box<Statement>,
    },
    /// A conventional three-part loop. Initializers and updates are vectors
    /// because Java and Dart allow comma-separated clauses.
    For {
        initializers: Vec<Statement>,
        condition: Option<Expression>,
        updates: Vec<Expression>,
        body: Box<Statement>,
    },
    While {
        condition: Expression,
        body: Box<Statement>,
    },
    DoWhile {
        body: Box<Statement>,
        condition: Expression,
    },
    Switch {
        expression: Expression,
        cases: Vec<SwitchCase>,
    },
    Try {
        body: Box<Statement>,
        catches: Vec<CatchClause>,
        finally_body: Option<Box<Statement>>,
    },
    Return(Option<Expression>),
    Throw(Expression),
    Assert(Expression),
    Break,
    Continue,
    Unlowered {
        syntax_kind: String,
    },
}

#[derive(Clone, Debug)]
pub struct SwitchCase {
    pub pattern: Pattern,
    pub statements: Vec<Statement>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug)]
pub struct CatchClause {
    pub exception_type: Option<TypeReference>,
    pub exception_name: Option<String>,
    pub stack_name: Option<String>,
    pub body: Box<Statement>,
    pub span: SourceSpan,
}

#[derive(Clone, Debug)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub source: String,
    pub span: SourceSpan,
}

#[derive(Clone, Debug)]
pub enum ExpressionKind {
    Identifier(String),
    Literal(Literal),
    StringInterpolation(Vec<StringPart>),
    Binary {
        operator: String,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    Unary {
        operator: String,
        operand: Box<Expression>,
    },
    Assignment {
        target: Box<Expression>,
        operator: String,
        value: Box<Expression>,
    },
    Member {
        object: Box<Expression>,
        property: String,
        null_aware: bool,
    },
    Index {
        object: Box<Expression>,
        index: Box<Expression>,
        null_aware: bool,
    },
    Call {
        callee: Box<Expression>,
        arguments: Vec<Argument>,
        type_arguments: Vec<TypeReference>,
    },
    IntrinsicCall {
        operation: IntrinsicOperation,
        receiver: Box<Expression>,
        arguments: Vec<Expression>,
    },
    ObjectCreation {
        type_ref: TypeReference,
        constructor: Option<String>,
        arguments: Vec<Argument>,
        is_const: bool,
    },
    ListLiteral {
        element_type: Option<TypeReference>,
        elements: Vec<CollectionElement>,
    },
    MapLiteral {
        key_type: Option<TypeReference>,
        value_type: Option<TypeReference>,
        entries: Vec<(Expression, Expression)>,
    },
    Closure {
        parameters: Vec<Parameter>,
        body: Box<Body>,
    },
    IfNull {
        left: Box<Expression>,
        right: Box<Expression>,
    },
    Await(Box<Expression>),
    Cast {
        expression: Box<Expression>,
        type_ref: TypeReference,
    },
    TypeTest {
        expression: Box<Expression>,
        type_ref: TypeReference,
        negated: bool,
    },
    Cascade {
        target: Box<Expression>,
        sections: Vec<Expression>,
    },
    Switch {
        expression: Box<Expression>,
        cases: Vec<SwitchExpressionCase>,
    },
    Raw {
        syntax_kind: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntrinsicOperation {
    CollectionContains,
    CollectionIndexOf,
}

#[derive(Clone, Debug)]
pub enum Literal {
    Null,
    Bool(bool),
    Integer(String),
    Float(String),
    String(String),
    Symbol(String),
}

#[derive(Clone, Debug)]
pub enum StringPart {
    Text(String),
    Expression(Expression),
}

#[derive(Clone, Debug)]
pub struct Argument {
    pub name: Option<String>,
    pub value: Expression,
}

#[derive(Clone, Debug)]
pub enum CollectionElement {
    Expression(Expression),
    Spread {
        expression: Expression,
        null_aware: bool,
    },
}

#[derive(Clone, Debug)]
pub struct SwitchExpressionCase {
    pub pattern: Pattern,
    pub value: Expression,
    pub span: SourceSpan,
}

#[derive(Clone, Debug)]
pub struct Pattern {
    pub kind: PatternKind,
    pub source: String,
    pub span: SourceSpan,
}

#[derive(Clone, Debug)]
pub enum PatternKind {
    Constant(Expression),
    Object {
        type_ref: TypeReference,
        fields: Vec<PatternField>,
    },
    Variable {
        name: String,
        is_final: bool,
    },
    Wildcard,
    Default,
    Raw {
        syntax_kind: String,
    },
}

#[derive(Clone, Debug)]
pub struct PatternField {
    pub name: String,
    pub binding: String,
}
