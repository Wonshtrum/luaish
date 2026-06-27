use std::collections::HashMap;
use std::fmt::{self, Debug, Write};

use crate::ast::{self, BinOp, Expr, FieldConstructor, FuncBody, Stmt, UnOp};
use crate::log;
use crate::source::{Source, Span, Spanned};

#[derive(Debug)]
pub enum Error {
    Unbound {
        ident: String,
        span: Span,
    },
    Mismatch {
        expected: NType,
        found: NType,
        span: Span,
    },
    UnexpectedField {
        ident: String,
        span: Span,
    },
}
impl Error {
    pub fn pretty_print<W: fmt::Write>(&self, source: &Source, out: &mut W) -> fmt::Result {
        match self {
            Self::Unbound { ident, span } => {
                out.write_fmt(format_args!("error: unbound ident `{ident}`\n"))?;
                source.print_span(*span, out)
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
            Self::UnexpectedField { ident, span } => {
                out.write_fmt(format_args!("error: unexpected field `{ident}`\n"))?;
                source.print_span(*span, out)
            }
        }
    }
}

type TypeId = usize;
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
            (Type::Integer, Type::Float) => true,
            (Type::Float, Type::Integer) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Boolean,
    Float,
    Integer,
    String,
    Struct(TypeId),
    Func {
        args: Vec<NType>,
        ret: Option<Box<NType>>,
    },
}
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
                if let Some(ret) = ret {
                    f.write_fmt(format_args!(":{ret}"))?;
                }
                return Ok(());
            }
        };
        f.write_str(name)
    }
}

#[derive(Clone)]
struct StructProto<'a> {
    fields: Box<[(&'a str, NType)]>,
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
            let ret = ret.as_ref().map(|typ| Box::new(process_type(names, typ)));
            (*nesting, Type::Func { args, ret })
        }
    };
    NType { nesting, inner }
}

pub fn run<'a>(stmts: &'a [Spanned<Stmt>]) -> Result<Context<'a>, Error> {
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
                args: vec![NType {
                    nesting: 0,
                    inner: Type::String,
                }],
                ret: Some(Box::new(NType {
                    nesting: 0,
                    inner: Type::Integer,
                })),
            },
        },
    );
    global.insert(
        "test",
        NType {
            nesting: 0,
            inner: Type::Func {
                args: vec![NType {
                    nesting: 0,
                    inner: Type::Integer,
                }],
                ret: None,
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
                ret: Some(Box::new(NType {
                    nesting: 0,
                    inner: Type::String,
                })),
            },
        },
    );
    global.insert(
        "exit",
        NType {
            nesting: 0,
            inner: Type::Func {
                args: vec![],
                ret: None,
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
        eval_stmt(&mut ctx, stmt)?;
    }

    Ok(ctx)
}

fn eval_call<'a>(
    ctx: &mut Context<'a>,
    name: &Spanned<String>,
    exprs: &'a [Spanned<Expr>],
) -> Result<Option<NType>, Error> {
    let val = ctx.get(name, name.span)?.clone();
    if val.nesting == 0 {
        if let Type::Func { args, ret } = val.inner {
            for (arg_typ, expr) in args.into_iter().zip(exprs.iter()) {
                let typ = eval_expr(ctx, expr)?.unwrap();
                if !arg_typ.can_coerce(&typ) {
                    return Err(Error::Mismatch {
                        expected: arg_typ,
                        found: typ,
                        span: expr.span,
                    });
                }
            }
            let ret = ret.as_ref().map(|typ| *typ.to_owned());
            return Ok(ret);
        }
    };
    panic!("expected function")
}

fn eval_func<'a>(ctx: &mut Context<'a>, body: &'a FuncBody) -> Result<NType, Error> {
    let mut scope = Scope::default();
    let mut args = Vec::with_capacity(body.args.len());
    for arg in &body.args {
        let typ = NType {
            nesting: 0,
            inner: Type::String,
        };
        args.push(typ.clone());
        scope.insert(arg.as_str(), typ);
    }
    ctx.scopes.push(scope);
    eval_stmts(ctx, &body.body)?;
    ctx.pop();
    Ok(NType {
        nesting: 0,
        inner: Type::Func { args, ret: None },
    })
}

pub fn eval_stmts<'a>(ctx: &mut Context<'a>, stmts: &'a [Spanned<Stmt>]) -> Result<(), Error> {
    for stmt in stmts {
        eval_stmt(ctx, stmt)?;
    }
    Ok(())
}

pub fn eval_stmt<'a>(ctx: &mut Context<'a>, stmt: &'a Spanned<Stmt>) -> Result<(), Error> {
    match &stmt.data {
        Stmt::TypeDef { .. } => {}
        Stmt::Break => {}
        Stmt::Return { values } => todo!(),
        Stmt::Call { name, args } => {
            eval_call(ctx, name, args)?;
        }
        Stmt::Assigns { is_local, lhs, rhs } => {
            for (l, r) in lhs.iter().zip(rhs.iter()) {
                let typ = eval_expr(ctx, r)?.unwrap();
                ctx.set(*is_local, l, typ, l.span)?;
            }
        }
        Stmt::Assign { lhs, rhs } => todo!(),
        Stmt::If {
            condition,
            then_block,
            else_if_blocks,
            else_block,
        } => {
            {
                let typ = eval_expr(ctx, condition)?.unwrap();
                if typ != BOOLEAN {
                    panic!("expected boolean");
                }
                ctx.push();
                eval_stmts(ctx, then_block)?;
                ctx.pop();
            }
            for (condition, then_block) in else_if_blocks {
                let typ = eval_expr(ctx, condition)?.unwrap();
                if typ != BOOLEAN {
                    panic!("expected boolean");
                }
                ctx.push();
                eval_stmts(ctx, then_block)?;
                ctx.pop();
            }
            if let Some(else_block) = else_block {
                ctx.push();
                eval_stmts(ctx, else_block)?;
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

pub fn eval_expr<'a>(
    ctx: &mut Context<'a>,
    expr: &'a Spanned<Expr>,
) -> Result<Option<NType>, Error> {
    let inner = match &expr.data {
        Expr::Nil => todo!(),
        Expr::True => Type::Boolean,
        Expr::False => Type::Boolean,
        Expr::Float(_) => Type::Float,
        Expr::Integer(_) => Type::Integer,
        Expr::String(_) => Type::String,
        Expr::Identifier(ident) => return ctx.get(ident, expr.span).map(|t| Some(t.to_owned())),
        Expr::UnOp { val, op } => {
            let typ = eval_expr(ctx, val)?.unwrap();
            match (op.data, typ) {
                (UnOp::Neg, FLOAT) => Type::Float,
                (UnOp::Neg, INTEGER) => Type::Integer,
                (UnOp::Not, BOOLEAN) => Type::Boolean,
                (op, val) => todo!("{op:?} {val:?}"),
            }
        }
        Expr::BinOp { rhs, lhs, op } => {
            let rhs = eval_expr(ctx, rhs)?.unwrap();
            let lhs = eval_expr(ctx, lhs)?.unwrap();
            match (op.data, lhs, rhs) {
                (BinOp::Add, STRING, STRING) => Type::String,

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
                (op, l, r) => todo!("{op:?} {l:?} {r:?}"),
            }
        }

        Expr::Table { elements } => todo!(),
        Expr::Call { name, args } => return eval_call(ctx, name, args),
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
                        let typ = eval_expr(ctx, expr)?.unwrap();
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
        Expr::Member { val, member } => {
            let typ = eval_expr(ctx, val)?.unwrap();
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
                    return Ok(Some(typ.clone()));
                }
            }
            return Err(Error::UnexpectedField {
                ident: member.to_string(),
                span: member.span,
            });
        }
    };
    Ok(Some(NType { nesting: 0, inner }))
}
