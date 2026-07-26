use crate::diagnostic::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    I32,
    F32,
    Bool,
    Void,
}

impl Type {
    pub const fn name(self) -> &'static str {
        match self {
            Self::I32 => "i32",
            Self::F32 => "f32",
            Self::Bool => "bool",
            Self::Void => "void",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub title: String,
    pub title_span: Span,
    pub globals: Vec<Global>,
    pub functions: Vec<Function>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Global {
    pub name: String,
    pub ty: Type,
    pub init: Expr,
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
    pub return_type: Type,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
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
        ty: Type,
        init: Expr,
    },
    Assign {
        name: String,
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
}
