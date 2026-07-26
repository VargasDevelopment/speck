use crate::diagnostic::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueType {
    I32,
    F32,
    Bool,
}

impl ValueType {
    pub const fn name(self) -> &'static str {
        match self {
            Self::I32 => "i32",
            Self::F32 => "f32",
            Self::Bool => "bool",
        }
    }

    pub const fn is_numeric(self) -> bool {
        matches!(self, Self::I32 | Self::F32)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnType {
    Value(ValueType),
    Void,
}

impl ReturnType {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Value(ty) => ty.name(),
            Self::Void => "void",
        }
    }

    pub const fn value_type(self) -> Option<ValueType> {
        match self {
            Self::Value(ty) => Some(ty),
            Self::Void => None,
        }
    }
}

impl From<ValueType> for ReturnType {
    fn from(value: ValueType) -> Self {
        Self::Value(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConstantValue {
    I32(i32),
    F32(f32),
    Bool(bool),
}

impl ConstantValue {
    pub const fn ty(self) -> ValueType {
        match self {
            Self::I32(_) => ValueType::I32,
            Self::F32(_) => ValueType::F32,
            Self::Bool(_) => ValueType::Bool,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub title: String,
    pub title_span: Span,
    pub constants: Vec<Constant>,
    pub globals: Vec<Global>,
    pub functions: Vec<Function>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Constant {
    pub name: String,
    pub ty: ValueType,
    pub init: Expr,
    pub value: Option<ConstantValue>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Global {
    pub name: String,
    pub ty: ValueType,
    pub init: Expr,
    pub value: Option<ConstantValue>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FunctionKind {
    Named,
    Start,
    Update,
    Draw,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub name: String,
    pub kind: FunctionKind,
    pub params: Vec<Param>,
    pub return_type: ReturnType,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: ValueType,
    pub span: Span,
}

pub type Block = Vec<Stmt>;

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Let {
        name: String,
        ty: ValueType,
        init: Expr,
    },
    Assign {
        name: String,
        op: AssignOp,
        value: Expr,
    },
    Expr(Expr),
    If {
        condition: Expr,
        then_block: Block,
        else_block: Option<Block>,
    },
    While {
        condition: Expr,
        body: Block,
    },
    Return(Option<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    I32(i64),
    F32(f32),
    Bool(bool),
    Variable(String),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
    Conversion {
        target: ValueType,
        args: Vec<Expr>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    LogicalAnd,
    LogicalOr,
}
