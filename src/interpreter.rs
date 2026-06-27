use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::rc::Rc;

use crate::log;

use crate::ast::{BinOp, Element, Expr, FuncBody, Stmt, UnOp};
use crate::source::{Source, Span, Spanned};

#[derive(Debug)]
pub enum Error {
    Custom {
        message: String,
        span: Span,
    },
    Unbound {
        ident: String,
        span: Span,
    },
    Exit {
        span: Span,
    },
    NotAFunction {
        ident: String,
        span: Span,
    },
    FFI(&'static str),
    TypeMismatch {
        expected: &'static str,
        found: String,
        span: Span,
    },
}
impl Error {
    pub fn pretty_print<W: fmt::Write>(&self, source: &Source, out: &mut W) -> fmt::Result {
        match self {
            Self::Exit { span } => {
                out.write_str("exit")?;
                out.write_char('\n')?;
                source.print_span(*span, out)
            }
            Self::Unbound { ident, span } => {
                out.write_fmt(format_args!("error: unbound ident `{ident}`"))?;
                source.print_span(*span, out)
            }
            Self::NotAFunction { ident, span } => {
                out.write_fmt(format_args!("error: ident `{ident}` is not a function"))?;
                source.print_span(*span, out)
            }
            Self::Custom { message, span } => {
                out.write_str(message)?;
                out.write_char('\n')?;
                source.print_span(*span, out)
            }
            Self::TypeMismatch {
                expected,
                found,
                span,
            } => {
                out.write_fmt(format_args!(
                    "error: type mismatch, expected: {expected}, found:\n{found}\n"
                ))?;
                source.print_span(*span, out)
            }
            Self::FFI(name) => out.write_fmt(format_args!(
                "error: extern function `{name}` internal error"
            )),
        }
    }
}

#[derive(Debug, Clone)]
enum Field {
    Index(u64),
    Name(Rc<str>),
}
#[derive(Debug, Clone)]
pub struct Table<'a> {
    indexed: BTreeMap<u64, Value<'a>>,
    named: HashMap<Rc<str>, Value<'a>>,
}
impl Table<'_> {
    fn is_empty(&self) -> bool {
        self.indexed.is_empty() && self.named.is_empty()
    }
}

#[derive(Debug, Clone)]
pub enum Type {
    Boolean,
    Float,
    Integer,
    String,
    Struct(Proto),
}

#[derive(Debug, Clone)]
pub struct Proto {
    fields: Vec<(String, Type)>,
}

#[derive(Debug, Clone)]
pub enum Value<'a> {
    Nil,
    Boolean(bool),
    Float(f64),
    Integer(i64),
    String(Rc<str>),
    Struct {
        typ: &'a Proto,
        fields: Box<[Value<'a>]>,
    },
    Table(Rc<Table<'a>>),
    Func(&'a FuncBody),
    FFI(fn(&mut Context<'a>, span: Span, Vec<Value<'a>>) -> Result<Value<'a>, Error>),
}
impl Value<'_> {
    fn truthy(&self) -> bool {
        match self {
            Value::Nil => false,
            Value::Boolean(v) => *v,
            Value::Float(v) => *v != 0.,
            Value::Integer(v) => *v != 0,
            Value::String(v) => !v.is_empty(),
            Value::Struct { .. } => true,
            Value::Table(v) => !v.is_empty(),
            Value::Func(_) => true,
            Value::FFI(_) => true,
        }
    }
}

type Scope<'a> = HashMap<&'a str, Value<'a>>;

#[derive(Debug)]
pub struct Context<'a> {
    scopes: Vec<Scope<'a>>,
}

impl<'a> Context<'a> {
    fn push(&mut self) {
        self.scopes.push(Scope::default());
    }
    fn pop(&mut self) {
        self.scopes.pop();
    }

    pub fn get(&mut self, ident: &str, span: Span) -> Result<Value<'a>, Error> {
        for scope in self.scopes.iter().rev() {
            if let Some(val) = scope.get(ident) {
                return Ok(val.clone());
            }
        }
        Err(Error::Unbound {
            ident: ident.to_owned(),
            span,
        })
    }
    pub fn set(&mut self, ident: &'a str, val: Value<'a>, span: Span) -> Result<(), Error> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(binding) = scope.get_mut(ident) {
                *binding = val;
                return Ok(());
            }
        }
        self.scopes.last_mut().unwrap().insert(ident, val);
        Ok(())
    }
}

fn extern_exit<'a>(
    _ctx: &mut Context<'a>,
    span: Span,
    _args: Vec<Value<'a>>,
) -> Result<Value<'a>, Error> {
    log!("EXIT!");
    Err(Error::Exit { span })
}
fn extern_print<'a>(
    _ctx: &mut Context<'a>,
    _span: Span,
    args: Vec<Value<'a>>,
) -> Result<Value<'a>, Error> {
    log!("PRINT {args:?}");
    let res = Value::Integer(args.len() as i64);
    Ok(res)
}
fn extern_test<'a>(
    ctx: &mut Context<'a>,
    _span: Span,
    args: Vec<Value<'a>>,
) -> Result<Value<'a>, Error> {
    log!("TEST");
    match args.as_slice() {
        [Value::Func(f), Value::Integer(n)] => {
            for _ in 0..*n {
                inner_func_call(ctx, f, args.clone())?;
            }
            Ok(Value::Nil)
        }
        _ => Err(Error::FFI("ffi::test")),
    }
}
fn extern_exec<'a>(
    ctx: &mut Context<'a>,
    _span: Span,
    args: Vec<Value<'a>>,
) -> Result<Value<'a>, Error> {
    let args = args
        .into_iter()
        .filter_map(|arg| {
            let arg = match arg {
                Value::Nil => return None,
                Value::Boolean(true) => "true".into(),
                Value::Boolean(false) => "false".into(),
                Value::Float(x) => format!("{x}"),
                Value::Integer(x) => format!("{x}"),
                Value::String(x) => x.to_string(),
                _ => return Some(Err(Error::FFI("ff::extern_test::invalid_arg"))),
            };
            Some(Ok(arg))
        })
        .collect::<Result<Vec<String>, Error>>()?;
    log!("EXEC {args:?}");
    let Some(cmd) = args.first() else {
        return Err(Error::FFI("ffi::extern_test::no_args"));
    };
    let res = std::process::Command::new(cmd)
        .args(args.into_iter().skip(1))
        .output();
    let res = match res {
        Ok(res) => String::from_utf8_lossy(&res.stdout).to_string(),
        Err(error) => error.to_string(),
    };
    Ok(Value::String(Rc::from(res)))
}

pub fn run(stmts: &[Spanned<Stmt>]) -> Result<String, Error> {
    let mut global = Scope::default();
    global.insert("print", Value::FFI(extern_print));
    global.insert("test", Value::FFI(extern_test));
    global.insert("exec", Value::FFI(extern_exec));
    global.insert("exit", Value::FFI(extern_exit));
    let mut ctx = Context {
        scopes: vec![global],
    };
    eval_stmts(&mut ctx, stmts)?;
    Ok(format!("{ctx:#?}"))
}

fn eval_stmts<'a>(ctx: &mut Context<'a>, stmts: &'a [Spanned<Stmt>]) -> Result<(), Error> {
    for stmt in stmts {
        eval_stmt(ctx, stmt)?;
    }
    Ok(())
}

fn eval_stmt<'a>(ctx: &mut Context<'a>, stmt: &'a Spanned<Stmt>) -> Result<(), Error> {
    match &stmt.data {
        Stmt::Break => todo!(),
        Stmt::Return { values } => todo!(),
        Stmt::Call { name, args } => {
            let _ = func_call(ctx, name, args)?;
        }
        Stmt::Assigns { is_local, lhs, rhs } => {
            for (lhs, rhs) in lhs.iter().zip(rhs.iter()) {
                let val: Value<'a> = eval_expr(ctx, rhs)?;
                ctx.set(lhs, val, lhs.span)?;
            }
        }
        Stmt::Assign { lhs, rhs } => todo!(),
        Stmt::If {
            condition,
            then_block,
            else_if_blocks,
            else_block,
        } => {
            if eval_expr(ctx, condition)?.truthy() {
                ctx.push();
                eval_stmts(ctx, then_block)?;
                ctx.pop();
                return Ok(());
            }
            for (condition, then_block) in else_if_blocks {
                if eval_expr(ctx, condition)?.truthy() {
                    ctx.push();
                    eval_stmts(ctx, then_block)?;
                    ctx.pop();
                    return Ok(());
                }
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
        Stmt::TypeDef { .. } => {}
        Stmt::FuncDef {
            is_local,
            name,
            body,
        } => {
            ctx.set(name, Value::Func(body), name.span)?;
        }
    }
    Ok(())
}

fn func_call<'a>(
    ctx: &mut Context<'a>,
    name: &Spanned<String>,
    args: &Spanned<Vec<Spanned<Expr>>>,
) -> Result<Value<'a>, Error> {
    let args = args
        .iter()
        .map(|arg| eval_expr(ctx, arg))
        .collect::<Result<Vec<_>, Error>>()?;
    match ctx.get(name, name.span)? {
        Value::Func(func_body) => inner_func_call(ctx, func_body, args),
        Value::FFI(f) => f(ctx, name.span, args),
        _ => Err(Error::NotAFunction {
            ident: name.data.to_owned(),
            span: name.span,
        }),
    }
}
pub fn inner_func_call<'a>(
    ctx: &mut Context<'a>,
    func_body: &'a FuncBody,
    args: Vec<Value<'a>>,
) -> Result<Value<'a>, Error> {
    let mut scope = Scope::default();
    for (binding, val) in func_body.args.iter().zip(args) {
        scope.insert(binding, val);
    }
    ctx.scopes.push(scope);
    eval_stmts(ctx, &func_body.body)?;
    Ok(Value::Nil)
}

fn eval_expr<'a>(ctx: &mut Context<'a>, expr: &Spanned<Expr>) -> Result<Value<'a>, Error> {
    match &expr.data {
        Expr::Nil => Ok(Value::Nil),
        Expr::True => Ok(Value::Boolean(true)),
        Expr::False => Ok(Value::Boolean(false)),
        Expr::Float(val) => Ok(Value::Float(*val)),
        Expr::Integer(val) => Ok(Value::Integer(*val)),
        Expr::String(val) => Ok(Value::String(Rc::from(val.to_owned()))),
        Expr::Identifier(ident) => ctx.get(ident, expr.span),
        Expr::Table { elements } => {
            let mut named = HashMap::new();
            let mut indexed = BTreeMap::new();
            let mut index = 0;
            for e in elements {
                match &e.data {
                    Element::Indexed(expr) => {
                        indexed.insert(index, eval_expr(ctx, expr)?);
                        index += 1;
                    }
                    Element::Named { name, expr } => {
                        named.insert(Rc::from(name.to_string()), eval_expr(ctx, expr)?);
                    }
                }
            }
            Ok(Value::Table(Rc::new(Table { named, indexed })))
        }
        Expr::UnOp { val, op } => {
            let val = eval_expr(ctx, val)?;
            let res = match (op.data, val) {
                (UnOp::Neg, Value::Float(val)) => Value::Float(-val),
                (UnOp::Neg, Value::Integer(val)) => Value::Integer(-val),
                (UnOp::Not, Value::Boolean(val)) => Value::Boolean(!val),
                (op, val) => todo!("{op:?} {val:?}"),
            };
            Ok(res)
        }
        Expr::Member { val, member } => {
            let span = val.span;
            let val = eval_expr(ctx, val)?;
            let res = match val {
                Value::Table(t) => t.named.get(member.as_str()).cloned().unwrap_or(Value::Nil),
                _ => {
                    return Err(Error::TypeMismatch {
                        expected: "Table",
                        found: format!("{val:#?}"),
                        span,
                    });
                }
            };
            Ok(res)
        }
        Expr::BinOp { rhs, lhs, op } => {
            let rhs = eval_expr(ctx, rhs)?;
            let lhs = eval_expr(ctx, lhs)?;
            let res = match (op.data, lhs, rhs) {
                (BinOp::Add, Value::String(l), Value::String(r)) => {
                    Value::String(Rc::from(format!("{l}{r}")))
                }

                (BinOp::Add, Value::Integer(l), Value::Integer(r)) => Value::Integer(l + r),
                (BinOp::Add, Value::Float(l), Value::Integer(r)) => Value::Float(l + r as f64),
                (BinOp::Add, Value::Integer(l), Value::Float(r)) => Value::Float(l as f64 + r),
                (BinOp::Add, Value::Float(l), Value::Float(r)) => Value::Float(l + r),
                (BinOp::Sub, Value::Integer(l), Value::Integer(r)) => Value::Integer(l - r),
                (BinOp::Sub, Value::Float(l), Value::Integer(r)) => Value::Float(l - r as f64),
                (BinOp::Sub, Value::Integer(l), Value::Float(r)) => Value::Float(l as f64 - r),
                (BinOp::Sub, Value::Float(l), Value::Float(r)) => Value::Float(l - r),
                (BinOp::Mul, Value::Integer(l), Value::Integer(r)) => Value::Integer(l * r),
                (BinOp::Mul, Value::Float(l), Value::Integer(r)) => Value::Float(l * r as f64),
                (BinOp::Mul, Value::Integer(l), Value::Float(r)) => Value::Float(l as f64 * r),
                (BinOp::Mul, Value::Float(l), Value::Float(r)) => Value::Float(l * r),
                (BinOp::Div, Value::Integer(l), Value::Integer(r)) => Value::Integer(l / r),
                (BinOp::Div, Value::Float(l), Value::Integer(r)) => Value::Float(l / r as f64),
                (BinOp::Div, Value::Integer(l), Value::Float(r)) => Value::Float(l as f64 / r),
                (BinOp::Div, Value::Float(l), Value::Float(r)) => Value::Float(l / r),

                (BinOp::EQ, Value::Integer(l), Value::Integer(r)) => Value::Boolean(l == r),
                (BinOp::EQ, Value::Float(l), Value::Integer(r)) => Value::Boolean(l == r as f64),
                (BinOp::EQ, Value::Integer(l), Value::Float(r)) => Value::Boolean(l as f64 == r),
                (BinOp::EQ, Value::Float(l), Value::Float(r)) => Value::Boolean(l == r),
                (BinOp::NE, Value::Integer(l), Value::Integer(r)) => Value::Boolean(l != r),
                (BinOp::NE, Value::Float(l), Value::Integer(r)) => Value::Boolean(l != r as f64),
                (BinOp::NE, Value::Integer(l), Value::Float(r)) => Value::Boolean(l as f64 != r),
                (BinOp::NE, Value::Float(l), Value::Float(r)) => Value::Boolean(l != r),
                (BinOp::LT, Value::Integer(l), Value::Integer(r)) => Value::Boolean(l < r),
                (BinOp::LT, Value::Float(l), Value::Integer(r)) => Value::Boolean(l < r as f64),
                (BinOp::LT, Value::Integer(l), Value::Float(r)) => Value::Boolean((l as f64) < r),
                (BinOp::LT, Value::Float(l), Value::Float(r)) => Value::Boolean(l < r),
                (BinOp::GT, Value::Integer(l), Value::Integer(r)) => Value::Boolean(l > r),
                (BinOp::GT, Value::Float(l), Value::Integer(r)) => Value::Boolean(l > r as f64),
                (BinOp::GT, Value::Integer(l), Value::Float(r)) => Value::Boolean((l as f64) > r),
                (BinOp::GT, Value::Float(l), Value::Float(r)) => Value::Boolean(l > r),
                (BinOp::LE, Value::Integer(l), Value::Integer(r)) => Value::Boolean(l <= r),
                (BinOp::LE, Value::Float(l), Value::Integer(r)) => Value::Boolean(l <= r as f64),
                (BinOp::LE, Value::Integer(l), Value::Float(r)) => Value::Boolean((l as f64) <= r),
                (BinOp::LE, Value::Float(l), Value::Float(r)) => Value::Boolean(l <= r),
                (BinOp::GE, Value::Integer(l), Value::Integer(r)) => Value::Boolean(l >= r),
                (BinOp::GE, Value::Float(l), Value::Integer(r)) => Value::Boolean(l >= r as f64),
                (BinOp::GE, Value::Integer(l), Value::Float(r)) => Value::Boolean((l as f64) >= r),
                (BinOp::GE, Value::Float(l), Value::Float(r)) => Value::Boolean(l >= r),

                (BinOp::And, l, r) => {
                    if l.truthy() {
                        r
                    } else {
                        l
                    }
                }
                (BinOp::Or, l, r) => {
                    if l.truthy() {
                        r
                    } else {
                        l
                    }
                }
                (op, l, r) => todo!("{op:?} {l:?} {r:?}"),
            };
            Ok(res)
        }
        Expr::TypeConstructor { name, fields } => todo!(),
        Expr::Call { name, args } => func_call(ctx, name, args),
        Expr::Func { body } => todo!(),
    }
}
