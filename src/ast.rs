use std::fmt;

use crate::source::Spanned;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    IDiv,
    Mod,
    Pow,
    // String
    Concat,
    // Comparison
    EQ,
    NE,
    LT,
    LE,
    GT,
    GE,
    // Logical
    And,
    Or,
    // Bitwise
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub enum Element {
    Indexed(Spanned<Expr>),
    Named {
        name: Spanned<String>,
        expr: Spanned<Expr>,
    },
}

#[derive(Debug, Clone)]
pub struct ArgDef {
    pub name: Spanned<String>,
    pub typ: Spanned<Type>,
}

#[derive(Debug, Clone)]
pub enum Type {
    Named {
        nesting: usize,
        name: Spanned<String>,
    },
    Function {
        nesting: usize,
        args: Vec<Spanned<Type>>,
        ret: Option<Box<Spanned<Type>>>,
    },
}

#[derive(Debug, Clone)]
pub enum FieldConstructor {
    Implicit(String),
    Explicit {
        name: Spanned<String>,
        expr: Spanned<Expr>,
    },
}

#[derive(Clone)]
pub enum Expr {
    // Literals
    Nil,
    True,
    False,
    Float(f64),
    Integer(i64),
    String(String),
    Identifier(String),
    Table {
        elements: Vec<Spanned<Element>>,
    },
    List {
        elements: Spanned<Vec<Spanned<Expr>>>,
    },

    UnOp {
        expr: Box<Spanned<Expr>>,
        op: Spanned<UnOp>,
    },
    BinOp {
        rhs: Box<Spanned<Expr>>,
        lhs: Box<Spanned<Expr>>,
        op: Spanned<BinOp>,
    },

    Call {
        expr: Box<Spanned<Expr>>,
        args: Spanned<Vec<Spanned<Expr>>>,
    },
    TypeConstructor {
        name: Spanned<String>,
        fields: Spanned<Vec<Spanned<FieldConstructor>>>,
    },

    Func {
        body: Spanned<FuncBody>,
    },

    Member {
        expr: Box<Spanned<Expr>>,
        member: Spanned<String>,
    },
    Index {
        expr: Box<Spanned<Expr>>,
        index: Box<Spanned<Expr>>,
    },
}

pub type Block = Vec<Spanned<Stmt>>;

#[derive(Debug, Clone)]
pub struct FuncBody {
    pub args: Spanned<Vec<Spanned<ArgDef>>>,
    pub ret: Option<Spanned<Type>>,
    pub body: Spanned<Block>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Break,
    Return {
        expr: Option<Spanned<Expr>>,
    },

    Call {
        expr: Box<Spanned<Expr>>,
        args: Spanned<Vec<Spanned<Expr>>>,
    },

    Binding {
        lhs: Spanned<Vec<Spanned<String>>>,
        rhs: Spanned<Vec<Spanned<Expr>>>,
    },

    Assign {
        lhs: Spanned<Vec<Spanned<Expr>>>,
        rhs: Spanned<Vec<Spanned<Expr>>>,
    },

    If {
        condition: Box<Spanned<Expr>>,
        then_block: Spanned<Block>,
        else_if_blocks: Vec<(Spanned<Expr>, Spanned<Block>)>,
        else_block: Option<Spanned<Block>>,
    },

    ForNum {
        var: Spanned<String>,
        start: Box<Spanned<Expr>>,
        limit: Box<Spanned<Expr>>,
        step: Box<Spanned<Expr>>,
        body: Spanned<Block>,
    },

    TypeDef {
        name: Spanned<String>,
        fields: Spanned<Vec<Spanned<ArgDef>>>,
    },

    FuncDef {
        is_local: bool,
        name: Spanned<String>,
        body: FuncBody,
    },
}

// ---------------------------------------------------------------------------
// Labels
// ---------------------------------------------------------------------------

impl Expr {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Nil => "Nil",
            Self::True => "True",
            Self::False => "False",
            Self::Float(_) => "Float",
            Self::Integer(_) => "Integer",
            Self::String(_) => "String",
            Self::Identifier(_) => "Identifier",
            Self::Table { .. } => "Table",
            Self::List { .. } => "List",
            Self::UnOp { .. } => "Unary Operation",
            Self::BinOp { .. } => "Binary Operation",
            Self::Member { .. } => "MemberAccess",
            Self::Index { .. } => "IndexAccess",
            Self::Call { .. } => "FunctionCall",
            Self::TypeConstructor { .. } => "TypeConstructor",
            Expr::Func { .. } => "Lambda",
        }
    }
}

impl Stmt {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Break => "Break",
            Self::Return { .. } => "Return",
            Self::Call { .. } => "FunctionCall",
            Self::Assign { .. } => "Assign",
            Self::Binding { .. } => "Binding",
            Self::If { .. } => "If/Else",
            Self::ForNum { .. } => "NumericFor",
            Self::FuncDef { .. } => "FunctionDef",
            Self::TypeDef { .. } => "TypeDef",
        }
    }
}

// ---------------------------------------------------------------------------
// Slightly more compact Debug
// ---------------------------------------------------------------------------

impl fmt::Debug for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nil => f.write_str("Nil"),
            Self::True => f.write_str("True"),
            Self::False => f.write_str("False"),
            Self::Float(x) => f.write_fmt(format_args!("Float({x})")),
            Self::Integer(x) => f.write_fmt(format_args!("Integer({x})")),
            Self::List { elements } => {
                f.write_str("List")?;
                f.debug_list().entries(&elements.data).finish()
            }
            Self::String(x) => f.write_fmt(format_args!("String({x})")),
            Self::Identifier(x) => f.write_fmt(format_args!("Identifier({x:?})")),
            Self::Table { elements } => {
                f.write_str("Table")?;
                let mut l = f.debug_list();
                for e in elements {
                    l.entry(e);
                }
                l.finish()
            }
            Self::Member { expr, member } => f
                .debug_struct("MemberAccess")
                .field("expr", expr)
                .field("member", member)
                .finish(),
            Self::Index { expr, index } => f
                .debug_struct("IndexAccess")
                .field("expr", expr)
                .field("index", index)
                .finish(),
            Self::Call { expr, args } => f
                .debug_struct("FunctionCall")
                .field("expr", expr)
                .field("args", args)
                .finish(),
            Self::TypeConstructor { name, fields } => f
                .debug_struct("TypeConstructor")
                .field("name", name)
                .field("fields", fields)
                .finish(),
            Self::UnOp { expr, op } => f
                .debug_struct("UnOp")
                .field("op", op)
                .field("expr", expr)
                .finish(),
            Self::BinOp { rhs, lhs, op } => f
                .debug_struct("BinOp")
                .field("op", op)
                .field("rhs", rhs)
                .field("lhs", lhs)
                .finish(),
            Expr::Func { body } => f.debug_struct("Func").field("body", body).finish(),
        }
    }
}
