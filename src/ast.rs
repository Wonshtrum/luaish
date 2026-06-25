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

    UnOp {
        val: Box<Spanned<Expr>>,
        op: Spanned<UnOp>,
    },
    BinOp {
        rhs: Box<Spanned<Expr>>,
        lhs: Box<Spanned<Expr>>,
        op: Spanned<BinOp>,
    },

    Call {
        name: Spanned<String>,
        args: Spanned<Vec<Spanned<Expr>>>,
    },
    Func {
        body: Spanned<FuncBody>,
    },

    Member {
        val: Box<Spanned<Expr>>,
        member: Spanned<String>,
    },
}

pub type Block = Vec<Spanned<Stmt>>;

#[derive(Debug, Clone)]
pub struct FuncBody {
    pub args: Vec<Spanned<String>>,
    pub body: Spanned<Block>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Break,
    Return {
        values: Spanned<Vec<Spanned<Expr>>>,
    },

    Call {
        name: Spanned<String>,
        args: Spanned<Vec<Spanned<Expr>>>,
    },

    Assigns {
        is_local: bool,
        lhs: Spanned<Vec<Spanned<String>>>,
        rhs: Spanned<Vec<Spanned<Expr>>>,
    },

    Assign {
        lhs: Spanned<Expr>,
        rhs: Spanned<Expr>,
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
            Self::UnOp { .. } => "Unary Operation",
            Self::BinOp { .. } => "Binary Operation",
            Self::Member { .. } => "MemberAccess",
            Self::Call { .. } => "Call",
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
            Self::Assign { .. } | Self::Assigns { .. } => "Assign",
            Self::If { .. } => "If/Else",
            Self::ForNum { .. } => "NumericFor",
            Self::FuncDef { .. } => "Function",
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
            Self::Member { val, member } => f
                .debug_struct("MemberAccess")
                .field("val", val)
                .field("member", member)
                .finish(),
            Self::Call { name, args } => f
                .debug_struct("Constructor")
                .field("name", name)
                .field("args", args)
                .finish(),
            Self::UnOp { val, op } => f
                .debug_struct("UnOp")
                .field("op", op)
                .field("val", val)
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
