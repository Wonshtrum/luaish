use std::collections::HashMap;
use std::fmt::{self, Debug, Write};

use crate::ast::{self, BinOp, Expr, FieldConstructor, FuncBody, Stmt, UnOp};
use crate::log;
use crate::source::{Source, Span, Spanned};

#[derive(Debug)]
pub enum Error {
    MissingTypeDefinition {
        name: String,
    },
    Unbound {
        ident: String,
        span: Span,
    },
    Mismatch {
        expected: NType,
        found: NType,
        span: Span,
    },
    ExpectedFunction {
        found: NType,
        span: Span,
    },
    UnexpectedField {
        ident: String,
        span: Span,
    },
    InvalidTypeForUnOp {
        op: UnOp,
        expr: Spanned<NType>,
    },
    InvalidTypeForBinOp {
        op: BinOp,
        lhs: Spanned<NType>,
        rhs: Spanned<NType>,
    },
    WrongNumberOfElements {
        expected: usize,
        found: usize,
        span: Span,
    },
    MalformedControlFow {
        span: Span,
    },
}
impl Error {
    pub fn pretty_print<W: fmt::Write>(&self, source: &Source, out: &mut W) -> fmt::Result {
        match self {
            Self::MalformedControlFow { span } => {
                out.write_fmt(format_args!("error: malformed control flow\n"))?;
                source.print_span(*span, out)
            }
            Self::WrongNumberOfElements {
                expected,
                found,
                span,
            } => {
                out.write_fmt(format_args!(
                    "error: expected {expected} elements, but found {found}\n"
                ))?;
                source.print_span(*span, out)
            }
            Self::MissingTypeDefinition { name } => out.write_fmt(format_args!(
                "error: missing type definition for `{name}`\n"
            )),
            Self::Unbound { ident, span } => {
                out.write_fmt(format_args!("error: unbound ident `{ident}`\n"))?;
                source.print_span(*span, out)
            }
            Self::InvalidTypeForUnOp { op, expr } => {
                out.write_fmt(format_args!(
                    "error: cannot apply `{op:?}` on type `{}`\n",
                    expr.data
                ))?;
                source.print_span(expr.span, out)
            }
            Self::InvalidTypeForBinOp { op, rhs, lhs } => {
                out.write_fmt(format_args!(
                    "error: cannot apply `{op:?}` between type `{}` and type `{}`\n",
                    lhs.data, rhs.data
                ))?;
                source.print_span(lhs.span, out)?;
                source.print_span(rhs.span, out)
            }
            Self::Mismatch {
                expected,
                found,
                span,
            } => {
                out.write_fmt(format_args!(
                    "error: expected `{expected}`, found `{found}`\n"
                ))?;
                source.print_span(*span, out)
            }
            Self::ExpectedFunction { found, span } => {
                out.write_fmt(format_args!("error: expected function, found `{found}`\n"))?;
                source.print_span(*span, out)
            }
            Self::UnexpectedField { ident, span } => {
                out.write_fmt(format_args!("error: unexpected field `{ident}`\n"))?;
                source.print_span(*span, out)
            }
        }
    }
}

pub type TypeId = usize;
#[derive(Clone, PartialEq, Eq)]
pub struct NType {
    nesting: usize,
    inner: Type,
}
impl NType {
    fn can_coerce(&self, other: &NType) -> bool {
        if self.nesting != other.nesting {
            return false;
        }
        if self.inner == other.inner {
            return true;
        }
        match (&self.inner, &other.inner) {
            // (Type::String, _) => true,
            (Type::Any, _) => true,
            (Type::Integer, Type::Float) => true,
            (Type::Float, Type::Integer) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Any,
    Nil,
    Boolean,
    Float,
    Integer,
    String,
    Struct(TypeId),
    Func { args: Vec<NType>, ret: Box<NType> },
}

const ANY: NType = NType {
    nesting: 0,
    inner: Type::Any,
};
const NIL: NType = NType {
    nesting: 0,
    inner: Type::Nil,
};
const BOOLEAN: NType = NType {
    nesting: 0,
    inner: Type::Boolean,
};
const FLOAT: NType = NType {
    nesting: 0,
    inner: Type::Float,
};
const INTEGER: NType = NType {
    nesting: 0,
    inner: Type::Integer,
};
const STRING: NType = NType {
    nesting: 0,
    inner: Type::String,
};

impl fmt::Debug for NType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("{self}"))
    }
}
impl fmt::Display for NType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for _ in 0..self.nesting {
            f.write_char('[')?;
        }
        f.write_fmt(format_args!("{}", self.inner))?;
        for _ in 0..self.nesting {
            f.write_char(']')?;
        }
        Ok(())
    }
}
impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Type::Any => "*",
            Type::Nil => "nil",
            Type::Boolean => "bool",
            Type::Float => "float",
            Type::Integer => "int",
            Type::String => "str",
            Type::Struct(id) => return f.write_fmt(format_args!("#{id}")),
            Type::Func { args, ret } => {
                f.write_str("fn(")?;
                for (i, arg) in args.iter().enumerate() {
                    if i != 0 {
                        f.write_char(',')?;
                    }
                    f.write_fmt(format_args!("{arg}"))?;
                }
                f.write_char(')')?;
                if **ret != NIL {
                    f.write_fmt(format_args!(":{ret}"))?;
                }
                return Ok(());
            }
        };
        f.write_str(name)
    }
}

#[derive(Clone)]
pub struct StructProto<'a> {
    pub fields: Box<[(&'a str, NType)]>,
}
impl fmt::Debug for StructProto<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut f = f.debug_struct("StructProto");
        for (name, typ) in &self.fields {
            f.field(name, typ);
        }
        f.finish()
    }
}

type Scope<'a> = HashMap<&'a str, NType>;
#[derive(Debug, Clone)]
pub struct Context<'a> {
    names: HashMap<&'a str, TypeId>,
    protos: HashMap<TypeId, StructProto<'a>>,
    scopes: Vec<Scope<'a>>,
}

#[derive(Debug, Clone)]
pub struct Prototypes<'a> {
    pub offsets: HashMap<(&'a str, &'a str), usize>,
    pub sizes: HashMap<&'a str, usize>,
}

impl<'a> Context<'a> {
    fn push(&mut self) {
        self.scopes.push(Scope::default());
    }
    fn pop(&mut self) {
        self.scopes.pop();
    }

    pub fn get(&self, ident: &str, span: Span) -> Result<&NType, Error> {
        for scope in self.scopes.iter().rev() {
            if let Some(tid) = scope.get(ident) {
                return Ok(tid);
            }
        }
        Err(Error::Unbound {
            ident: ident.to_owned(),
            span,
        })
    }
    pub fn set(
        &mut self,
        local: bool,
        ident: &'a str,
        typ: NType,
        span: Span,
    ) -> Result<(), Error> {
        if local {
            let scope = self.scopes.last_mut().unwrap();
            let shadow = scope.insert(ident, typ);
            if shadow.is_some() {
                panic!("can't shadow in same scope");
            }
            return Ok(());
        }
        for scope in self.scopes.iter_mut().rev() {
            if let Some(old) = scope.get_mut(ident) {
                if old == &typ {
                    return Ok(());
                }
                return Err(Error::Mismatch {
                    expected: old.to_owned(),
                    found: typ,
                    span,
                });
            }
        }
        Err(Error::Unbound {
            ident: ident.to_owned(),
            span,
        })
    }
}

fn process_type<'a>(names: &mut HashMap<&'a str, TypeId>, typ: &'a ast::Type) -> NType {
    let (nesting, inner) = match typ {
        ast::Type::Named { nesting, name } => (
            *nesting,
            match name.as_str() {
                "nil" => Type::Nil,
                "bool" => Type::Boolean,
                "float" => Type::Float,
                "int" => Type::Integer,
                "str" => Type::String,
                other => {
                    let id = names.len();
                    let id = names.entry(other).or_insert(id);
                    Type::Struct(*id)
                }
            },
        ),
        ast::Type::Function { nesting, args, ret } => {
            let args = args
                .iter()
                .map(|typ| process_type(names, typ))
                .collect::<Vec<_>>();
            let ret = ret
                .as_ref()
                .map(|typ| process_type(names, typ))
                .unwrap_or(NIL);
            (
                *nesting,
                Type::Func {
                    args,
                    ret: Box::new(ret),
                },
            )
        }
    };
    NType { nesting, inner }
}

pub fn run<'a>(stmts: &'a [Spanned<Stmt>]) -> Result<Prototypes<'a>, Error> {
    let mut protos = HashMap::new();
    let mut names = HashMap::new();
    for stmt in stmts {
        if let Stmt::TypeDef { name, fields } = &stmt.data {
            let id = if let Some(id) = names.get(name.as_str()) {
                if let Some(proto) = protos.get(id) {
                    panic!("redefinition of {name}");
                }
                *id
            } else {
                let id = names.len();
                names.insert(name.as_str(), id);
                id
            };
            let fields = fields
                .iter()
                .map(|f| (f.data.name.as_str(), process_type(&mut names, &f.data.typ)))
                .collect::<Vec<_>>()
                .into_boxed_slice();
            protos.insert(id, StructProto { fields });
        }
    }

    let mut global = HashMap::new();
    global.insert(
        "print",
        NType {
            nesting: 0,
            inner: Type::Func {
                args: vec![ANY],
                ret: Box::new(INTEGER),
            },
        },
    );
    global.insert(
        "test",
        NType {
            nesting: 0,
            inner: Type::Func {
                args: vec![
                    NType {
                        nesting: 0,
                        inner: Type::Func {
                            args: vec![],
                            ret: Box::new(NIL),
                        },
                    },
                    INTEGER,
                ],
                ret: Box::new(NIL),
            },
        },
    );
    global.insert(
        "exec",
        NType {
            nesting: 0,
            inner: Type::Func {
                args: vec![NType {
                    nesting: 1,
                    inner: Type::String,
                }],
                ret: Box::new(STRING),
            },
        },
    );
    global.insert(
        "exit",
        NType {
            nesting: 0,
            inner: Type::Func {
                args: vec![],
                ret: Box::new(NIL),
            },
        },
    );

    let mut ctx = Context {
        names,
        protos,
        scopes: vec![global],
    };
    log!("{ctx:#?}");

    for stmt in stmts {
        eval_stmt(&mut ctx, stmt, None)?;
    }

    let mut offsets = HashMap::new();
    let mut sizes = HashMap::new();
    for (name, tid) in ctx.names {
        let Some(proto) = ctx.protos.remove(&tid) else {
            return Err(Error::MissingTypeDefinition {
                name: name.to_owned(),
            });
        };
        sizes.insert(name, proto.fields.len());
        for (offset, (member, typ)) in proto.fields.into_iter().enumerate() {
            offsets.insert((name, member), offset);
        }
    }
    Ok(Prototypes { offsets, sizes })
}

fn eval_call<'a>(
    ctx: &mut Context<'a>,
    expr: &'a Spanned<Expr>,
    exprs: &'a [Spanned<Expr>],
) -> Result<NType, Error> {
    let typ = eval_expr(ctx, expr)?;
    if typ.nesting == 0 {
        if let Type::Func { args, ret } = typ.inner {
            if args.len() != exprs.len() {
                return Err(Error::WrongNumberOfElements {
                    expected: args.len(),
                    found: exprs.len(),
                    span: expr.span,
                });
            }
            for (arg_typ, expr) in args.into_iter().zip(exprs.iter()) {
                let typ = eval_expr(ctx, expr)?;
                if !arg_typ.can_coerce(&typ) {
                    return Err(Error::Mismatch {
                        expected: arg_typ,
                        found: typ,
                        span: expr.span,
                    });
                }
            }
            return Ok(*ret);
        }
    };
    Err(Error::ExpectedFunction {
        found: typ,
        span: expr.span,
    })
}

fn eval_func<'a>(ctx: &mut Context<'a>, body: &'a FuncBody) -> Result<NType, Error> {
    let mut scope = Scope::default();
    let mut args = Vec::with_capacity(body.args.len());
    for arg in &body.args.data {
        let typ = process_type(&mut ctx.names, &arg.typ);
        args.push(typ.clone());
        scope.insert(arg.name.as_str(), typ);
    }
    let ret = body
        .ret
        .as_ref()
        .map(|typ| process_type(&mut ctx.names, typ))
        .unwrap_or(NIL);
    ctx.scopes.push(scope);
    let ret = body.args.span.attach(ret);
    eval_stmts(ctx, &body.body, Some(&ret))?;
    ctx.pop();
    let ret = Box::new(ret.data);
    Ok(NType {
        nesting: 0,
        inner: Type::Func { args, ret },
    })
}

pub fn eval_stmts<'a>(
    ctx: &mut Context<'a>,
    stmts: &'a [Spanned<Stmt>],
    ret: Option<&Spanned<NType>>,
) -> Result<(), Error> {
    for stmt in stmts {
        eval_stmt(ctx, stmt, ret)?;
    }
    Ok(())
}

pub fn eval_stmt<'a>(
    ctx: &mut Context<'a>,
    stmt: &'a Spanned<Stmt>,
    ret: Option<&Spanned<NType>>,
) -> Result<(), Error> {
    match &stmt.data {
        Stmt::TypeDef { .. } => {}
        Stmt::Break => {}
        Stmt::Return { expr } => {
            let Some(ret) = ret else {
                return Err(Error::MalformedControlFow { span: stmt.span });
            };
            let (typ, span) = if let Some(expr) = expr {
                (eval_expr(ctx, expr)?, expr.span)
            } else {
                (NIL, stmt.span)
            };
            if !ret.can_coerce(&typ) {
                return Err(Error::Mismatch {
                    expected: ret.data.to_owned(),
                    found: typ,
                    span,
                });
            }
        }
        Stmt::Call { expr, args } => {
            eval_call(ctx, expr, args)?;
        }
        Stmt::Binding { lhs, rhs } => {
            if lhs.len() != rhs.len() {
                return Err(Error::WrongNumberOfElements {
                    expected: lhs.len(),
                    found: rhs.len(),
                    span: lhs.span,
                });
            }
            for (l, r) in lhs.iter().zip(rhs.iter()) {
                let typ = eval_expr(ctx, r)?;
                ctx.set(true, l, typ, l.span)?;
            }
        }
        Stmt::Assign { lhs, rhs } => {
            if lhs.len() != rhs.len() {
                return Err(Error::WrongNumberOfElements {
                    expected: lhs.len(),
                    found: rhs.len(),
                    span: lhs.span,
                });
            }
            for (l, r) in lhs.iter().zip(rhs.iter()) {
                let ltyp = eval_expr(ctx, l)?;
                let rtyp = eval_expr(ctx, r)?;
                if !ltyp.can_coerce(&rtyp) {
                    return Err(Error::Mismatch {
                        expected: ltyp,
                        found: rtyp,
                        span: r.span,
                    });
                }
            }
        }
        Stmt::If {
            condition,
            then_block,
            else_if_blocks,
            else_block,
        } => {
            {
                let typ = eval_expr(ctx, condition)?;
                if typ != BOOLEAN {
                    return Err(Error::Mismatch {
                        expected: BOOLEAN,
                        found: typ,
                        span: condition.span,
                    });
                }
                ctx.push();
                eval_stmts(ctx, then_block, ret)?;
                ctx.pop();
            }
            for (condition, then_block) in else_if_blocks {
                let typ = eval_expr(ctx, condition)?;
                if typ != BOOLEAN {
                    return Err(Error::Mismatch {
                        expected: BOOLEAN,
                        found: typ,
                        span: condition.span,
                    });
                }
                ctx.push();
                eval_stmts(ctx, then_block, ret)?;
                ctx.pop();
            }
            if let Some(else_block) = else_block {
                ctx.push();
                eval_stmts(ctx, else_block, ret)?;
                ctx.pop();
            }
        }
        Stmt::ForNum {
            var,
            start,
            limit,
            step,
            body,
        } => todo!(),
        Stmt::FuncDef {
            is_local,
            name,
            body,
        } => {
            let typ = eval_func(ctx, body)?;
            ctx.set(true, name, typ, name.span)?;
        }
    }
    Ok(())
}

pub fn eval_expr<'a>(ctx: &mut Context<'a>, expr: &'a Spanned<Expr>) -> Result<NType, Error> {
    let inner = match &expr.data {
        Expr::Nil => Type::Nil,
        Expr::True => Type::Boolean,
        Expr::False => Type::Boolean,
        Expr::Float(_) => Type::Float,
        Expr::Integer(_) => Type::Integer,
        Expr::String(_) => Type::String,
        Expr::List { elements } => {
            let Some(base) = elements.first() else {
                return todo!();
            };
            let mut base_typ = eval_expr(ctx, base)?;
            for e in &elements.data[1..] {
                let typ = eval_expr(ctx, e)?;
                if base_typ != typ {
                    return Err(Error::Mismatch {
                        expected: base_typ,
                        found: typ,
                        span: e.span,
                    });
                }
            }
            base_typ.nesting += 1;
            return Ok(base_typ);
        }
        Expr::Identifier(ident) => {
            return ctx.get(ident, expr.span).cloned();
        }
        Expr::UnOp { expr, op } => {
            let typ = eval_expr(ctx, expr)?;
            match (op.data, typ) {
                (UnOp::Neg, FLOAT) => Type::Float,
                (UnOp::Neg, INTEGER) => Type::Integer,
                (UnOp::Not, BOOLEAN) => Type::Boolean,
                (op, typ) => {
                    return Err(Error::InvalidTypeForUnOp {
                        op,
                        expr: expr.span.attach(typ),
                    });
                }
            }
        }
        Expr::BinOp { rhs, lhs, op } => {
            let r = eval_expr(ctx, rhs)?;
            let l = eval_expr(ctx, lhs)?;
            match (op.data, l, r) {
                (BinOp::Add, STRING, _) => Type::String,

                (
                    BinOp::Add,
                    NType {
                        nesting: ln,
                        inner: ltyp,
                    },
                    NType {
                        nesting: rn,
                        inner: rtyp,
                    },
                ) if ln == rn && ltyp == rtyp => {
                    return Ok(NType {
                        nesting: ln,
                        inner: ltyp,
                    });
                }

                (BinOp::Add, INTEGER, INTEGER) => Type::Integer,
                (BinOp::Add, FLOAT, INTEGER) => Type::Float,
                (BinOp::Add, INTEGER, FLOAT) => Type::Float,
                (BinOp::Add, FLOAT, FLOAT) => Type::Float,
                (BinOp::Sub, INTEGER, INTEGER) => Type::Integer,
                (BinOp::Sub, FLOAT, INTEGER) => Type::Float,
                (BinOp::Sub, INTEGER, FLOAT) => Type::Float,
                (BinOp::Sub, FLOAT, FLOAT) => Type::Float,
                (BinOp::Mul, INTEGER, INTEGER) => Type::Integer,
                (BinOp::Mul, FLOAT, INTEGER) => Type::Float,
                (BinOp::Mul, INTEGER, FLOAT) => Type::Float,
                (BinOp::Mul, FLOAT, FLOAT) => Type::Float,
                (BinOp::Div, INTEGER, INTEGER) => Type::Integer,
                (BinOp::Div, FLOAT, INTEGER) => Type::Float,
                (BinOp::Div, INTEGER, FLOAT) => Type::Float,
                (BinOp::Div, FLOAT, FLOAT) => Type::Float,

                (BinOp::EQ, INTEGER, INTEGER) => Type::Boolean,
                (BinOp::EQ, FLOAT, INTEGER) => Type::Boolean,
                (BinOp::EQ, INTEGER, FLOAT) => Type::Boolean,
                (BinOp::EQ, FLOAT, FLOAT) => Type::Boolean,
                (BinOp::NE, INTEGER, INTEGER) => Type::Boolean,
                (BinOp::NE, FLOAT, INTEGER) => Type::Boolean,
                (BinOp::NE, INTEGER, FLOAT) => Type::Boolean,
                (BinOp::NE, FLOAT, FLOAT) => Type::Boolean,
                (BinOp::LT, INTEGER, INTEGER) => Type::Boolean,
                (BinOp::LT, FLOAT, INTEGER) => Type::Boolean,
                (BinOp::LT, INTEGER, FLOAT) => Type::Boolean,
                (BinOp::LT, FLOAT, FLOAT) => Type::Boolean,
                (BinOp::GT, INTEGER, INTEGER) => Type::Boolean,
                (BinOp::GT, FLOAT, INTEGER) => Type::Boolean,
                (BinOp::GT, INTEGER, FLOAT) => Type::Boolean,
                (BinOp::GT, FLOAT, FLOAT) => Type::Boolean,
                (BinOp::LE, INTEGER, INTEGER) => Type::Boolean,
                (BinOp::LE, FLOAT, INTEGER) => Type::Boolean,
                (BinOp::LE, INTEGER, FLOAT) => Type::Boolean,
                (BinOp::LE, FLOAT, FLOAT) => Type::Boolean,
                (BinOp::GE, INTEGER, INTEGER) => Type::Boolean,
                (BinOp::GE, FLOAT, INTEGER) => Type::Boolean,
                (BinOp::GE, INTEGER, FLOAT) => Type::Boolean,
                (BinOp::GE, FLOAT, FLOAT) => Type::Boolean,

                (BinOp::And, BOOLEAN, BOOLEAN) => Type::Boolean,
                (BinOp::Or, BOOLEAN, BOOLEAN) => Type::Boolean,
                (op, l, r) => {
                    return Err(Error::InvalidTypeForBinOp {
                        op,
                        lhs: lhs.span.attach(l),
                        rhs: rhs.span.attach(r),
                    });
                }
            }
        }

        Expr::Table { elements } => todo!(),
        Expr::Call { expr, args } => {
            return eval_call(ctx, expr, args);
        }
        Expr::TypeConstructor { name, fields } => {
            let Some(proto_id) = ctx.names.get(name.as_str()).copied() else {
                panic!("expected type");
            };
            let Some(proto) = ctx.protos.get(&proto_id) else {
                panic!("expected defined type");
            };
            let proto_fields = proto.fields.clone();
            'next_field: for field in &fields.data {
                match &field.data {
                    FieldConstructor::Implicit(name) => {
                        let typ = ctx.get(name, field.span)?;
                        for proto_field in &proto_fields {
                            if proto_field.0 == name {
                                if !proto_field.1.can_coerce(typ) {
                                    return Err(Error::Mismatch {
                                        expected: proto_field.1.to_owned(),
                                        found: typ.to_owned(),
                                        span: field.span,
                                    });
                                }
                                continue 'next_field;
                            }
                        }
                        return Err(Error::UnexpectedField {
                            ident: name.to_owned(),
                            span: field.span,
                        });
                    }
                    FieldConstructor::Explicit { name, expr } => {
                        let typ = eval_expr(ctx, expr)?;
                        for proto_field in &proto_fields {
                            if proto_field.0 == name.as_str() {
                                if !proto_field.1.can_coerce(&typ) {
                                    return Err(Error::Mismatch {
                                        expected: proto_field.1.to_owned(),
                                        found: typ.to_owned(),
                                        span: name.span,
                                    });
                                }
                                continue 'next_field;
                            }
                        }
                        return Err(Error::UnexpectedField {
                            ident: name.to_string(),
                            span: name.span,
                        });
                    }
                }
            }
            Type::Struct(proto_id)
        }
        Expr::Func { body } => eval_func(ctx, body)?.inner,
        Expr::Member { expr, member } => {
            let typ = eval_expr(ctx, expr)?;
            if typ.nesting != 0 {
                panic!("expected struct");
            }
            let Type::Struct(proto_id) = typ.inner else {
                panic!("expected struct")
            };
            let Some(proto) = ctx.protos.get(&proto_id) else {
                unreachable!();
            };
            for (name, typ) in &proto.fields {
                if member.as_str() == *name {
                    return Ok(typ.clone());
                }
            }
            return Err(Error::UnexpectedField {
                ident: member.to_string(),
                span: member.span,
            });
        }
        Expr::Index { expr, index } => {
            let mut val_typ = eval_expr(ctx, expr)?;
            if val_typ.nesting == 0 {
                panic!("expected list");
            }
            let idx_typ = eval_expr(ctx, index)?;
            if idx_typ != INTEGER {
                return Err(Error::Mismatch {
                    expected: INTEGER,
                    found: idx_typ,
                    span: index.span,
                });
            }
            val_typ.nesting -= 1;
            return Ok(val_typ);
        }
    };
    Ok(NType { nesting: 0, inner })
}
