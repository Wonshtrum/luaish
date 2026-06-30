use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fmt::{self, Write};
use std::rc::Rc;

use crate::log;

use crate::ast::{self, BinOp, Element, Expr, FieldConstructor, FuncBody, Stmt, UnOp};
use crate::resolution::Prototypes;
use crate::source::{Source, Span, Spanned};

#[derive(Debug)]
pub enum Error<'a> {
    Return {
        span: Span,
        value: Value<'a>,
    },
    Break {
        span: Span,
    },
    MalformedControlFlow {
        span: Span,
    },

    Custom {
        message: String,
        span: Span,
    },
    OutOfBound {
        len: usize,
        index: i64,
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
        span: Span,
        found: Value<'a>,
    },
    FFI(&'static str),
    TypeMismatch {
        expected: &'static str,
        found: Value<'a>,
        span: Span,
    },
}
impl Error<'_> {
    pub fn pretty_print<W: fmt::Write>(&self, source: &Source, out: &mut W) -> fmt::Result {
        match self {
            Self::Return { span, .. }
            | Self::Break { span }
            | Self::MalformedControlFlow { span } => {
                out.write_fmt(format_args!("error: malformed control flow\n"))?;
                source.print_span(*span, out)
            }

            Self::Exit { span } => {
                out.write_str("exit")?;
                out.write_char('\n')?;
                source.print_span(*span, out)
            }
            Self::Unbound { ident, span } => {
                out.write_fmt(format_args!("error: unbound ident `{ident}`\n"))?;
                source.print_span(*span, out)
            }
            Self::OutOfBound { len, index, span } => {
                out.write_fmt(format_args!("error: index {index} out of bounds 0..{len}"))?;
                source.print_span(*span, out)
            }
            Self::NotAFunction { found, span } => {
                out.write_fmt(format_args!(
                    "error: expected this expression to be a function\n"
                ))?;
                source.print_span(*span, out)?;
                out.write_fmt(format_args!("but found:\n{found:#?}"))
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
                out.write_fmt(format_args!("error: type mismatch, expected: {expected}\n"))?;
                source.print_span(*span, out)?;
                out.write_fmt(format_args!("but found:\n{found:#?}"))
            }
            Self::FFI(name) => out.write_fmt(format_args!(
                "error: extern function `{name}` internal error"
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Table<'a> {
    indexed: BTreeMap<i64, Value<'a>>,
    named: HashMap<Rc<str>, Value<'a>>,
}
impl Table<'_> {
    fn is_empty(&self) -> bool {
        self.indexed.is_empty() && self.named.is_empty()
    }
}

#[derive(Clone)]
pub enum Value<'a> {
    Nil,
    Boolean(bool),
    Float(f64),
    Integer(i64),
    String(Rc<String>),
    List(Rc<RefCell<Vec<Value<'a>>>>),
    Struct {
        typ: &'a str,
        fields: Rc<RefCell<Box<[Value<'a>]>>>,
    },
    Table(Rc<Table<'a>>),
    Func(&'a FuncBody),
    FFI(fn(&mut Context<'a>, span: Span, Vec<Value<'a>>) -> Result<Value<'a>, Error<'a>>),
}

impl fmt::Debug for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nil => f.write_str("nil"),
            Self::Boolean(true) => f.write_str("true"),
            Self::Boolean(false) => f.write_str("false"),
            Self::Float(x) => f.write_fmt(format_args!("{x}")),
            Self::Integer(x) => f.write_fmt(format_args!("{x}")),
            Self::String(x) => f.write_fmt(format_args!("{x:?}")),
            Self::List(x) => fmt::Debug::fmt(&x.borrow(), f),
            Self::Struct { typ, fields } => {
                let mut f = f.debug_struct(typ);
                for (i, field) in fields.borrow().iter().enumerate() {
                    f.field(&format!("{i}"), field);
                }
                f.finish()
            }
            Self::Table(_) => f.write_str("{…}"),
            Self::Func(_) => f.write_str("fn{…}"),
            Self::FFI(_) => f.write_str("ffi{…}"),
        }
    }
}

impl Value<'_> {
    fn truthy(&self) -> bool {
        match self {
            Value::Nil => false,
            Value::Boolean(v) => *v,
            Value::Float(v) => *v != 0.,
            Value::Integer(v) => *v != 0,
            Value::String(v) => !v.is_empty(),
            Value::List(v) => !v.borrow().is_empty(),
            Value::Table(v) => !v.is_empty(),
            Value::Struct { .. } => true,
            Value::Func(_) => true,
            Value::FFI(_) => true,
        }
    }

    fn coerce<'a>(self, typ: &Spanned<ast::Type>) -> Result<Self, Error<'a>> {
        Ok(self)
        // match (self, typ) {
        //     (_, ast::Type::Function { .. }) => unreachable!(),
        //     (Value::Nil, _) => todo!(),
        //     (Value::Float(v), ast::Type::Named { nesting, name }
        // }
        // todo!()
    }
}

type Scope<'a> = HashMap<&'a str, Value<'a>>;

#[derive(Debug)]
pub struct Context<'a> {
    protos: Prototypes<'a>,
    scopes: Vec<Scope<'a>>,
}

impl<'a> Context<'a> {
    fn push(&mut self) {
        self.scopes.push(Scope::default());
    }
    fn pop(&mut self) {
        self.scopes.pop();
    }

    pub fn get(&mut self, ident: &str, span: Span) -> Result<Value<'a>, Error<'a>> {
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
    pub fn set(&mut self, ident: &'a str, val: Value<'a>, span: Span) -> Result<(), Error<'a>> {
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
) -> Result<Value<'a>, Error<'a>> {
    log!("EXIT!");
    Err(Error::Exit { span })
}
fn extern_print<'a>(
    _ctx: &mut Context<'a>,
    _span: Span,
    args: Vec<Value<'a>>,
) -> Result<Value<'a>, Error<'a>> {
    log!("PRINT {args:?}");
    let res = Value::Integer(args.len() as i64);
    Ok(res)
}
fn extern_test<'a>(
    ctx: &mut Context<'a>,
    _span: Span,
    args: Vec<Value<'a>>,
) -> Result<Value<'a>, Error<'a>> {
    log!("TEST {args:?}");
    match args.as_slice() {
        [Value::Func(f), Value::Integer(n)] => {
            for _ in 0..*n {
                inner_func_call(ctx, f, Vec::new())?;
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
) -> Result<Value<'a>, Error<'a>> {
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
        .collect::<Result<Vec<_>, _>>()?;
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

pub fn run<'a>(protos: Prototypes<'a>, stmts: &'a [Spanned<Stmt>]) -> Result<String, Error<'a>> {
    let mut global = Scope::default();
    global.insert("print", Value::FFI(extern_print));
    global.insert("test", Value::FFI(extern_test));
    global.insert("exec", Value::FFI(extern_exec));
    global.insert("exit", Value::FFI(extern_exit));
    let mut ctx = Context {
        scopes: vec![global],
        protos,
    };
    eval_stmts(&mut ctx, stmts)?;
    Ok(format!("{ctx:#?}"))
}

fn eval_stmts<'a>(ctx: &mut Context<'a>, stmts: &'a [Spanned<Stmt>]) -> Result<(), Error<'a>> {
    for stmt in stmts {
        eval_stmt(ctx, stmt)?;
    }
    Ok(())
}

fn eval_stmt<'a>(ctx: &mut Context<'a>, stmt: &'a Spanned<Stmt>) -> Result<(), Error<'a>> {
    match &stmt.data {
        Stmt::Break => return Err(Error::Break { span: stmt.span }),
        Stmt::Return { expr } => {
            let value = if let Some(expr) = expr {
                eval_expr(ctx, expr)?
            } else {
                Value::Nil
            };
            return Err(Error::Return {
                span: stmt.span,
                value,
            });
        }
        Stmt::Call { expr, args } => {
            let _ = func_call(ctx, expr, args)?;
        }
        Stmt::Binding { lhs, rhs } => {
            for (lhs, rhs) in lhs.iter().zip(rhs.iter()) {
                let val = eval_expr(ctx, rhs)?;
                ctx.set(lhs, val, lhs.span)?;
            }
        }
        Stmt::Assign { lhs, rhs } => {
            for (lhs, rhs) in lhs.iter().zip(rhs.iter()) {
                let rhs = eval_expr(ctx, rhs)?;
                match &lhs.data {
                    Expr::Identifier(ident) => ctx.set(ident, rhs, lhs.span)?,
                    Expr::Member { expr, member } => {
                        let span = expr.span;
                        let val = eval_expr(ctx, expr)?;
                        match val {
                            Value::Struct { typ, fields } => {
                                let offset =
                                    ctx.protos.offsets.get(&(typ, member.as_str())).unwrap();
                                let mut fields = fields.borrow_mut();
                                let field = fields.get_mut(*offset).unwrap();
                                *field = rhs;
                            }
                            _ => {
                                return Err(Error::TypeMismatch {
                                    expected: "Type",
                                    found: val,
                                    span,
                                });
                            }
                        }
                    }
                    Expr::Index { expr, index } => {
                        let val = eval_expr(ctx, expr)?;
                        let idx = eval_expr(ctx, index)?;
                        let Value::List(l) = val else {
                            return Err(Error::TypeMismatch {
                                expected: "List",
                                found: val,
                                span: expr.span,
                            });
                        };
                        let Value::Integer(i) = idx else {
                            return Err(Error::TypeMismatch {
                                expected: "int",
                                found: idx,
                                span: index.span,
                            });
                        };
                        let mut l = l.borrow_mut();
                        if i < 0 || i as usize >= l.len() {
                            return Err(Error::OutOfBound {
                                index: i,
                                len: l.len(),
                                span: index.span,
                            });
                        }
                        let e = unsafe { l.get_unchecked_mut(i as usize) };
                        *e = rhs;
                    }
                    _ => unreachable!(),
                };
            }
        }
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
    func: &'a Spanned<Expr>,
    args: &'a Spanned<Vec<Spanned<Expr>>>,
) -> Result<Value<'a>, Error<'a>> {
    let span = func.span;
    let func = eval_expr(ctx, func)?;
    let args = args
        .iter()
        .map(|arg| eval_expr(ctx, arg))
        .collect::<Result<Vec<_>, _>>()?;
    match func {
        Value::Func(func_body) => inner_func_call(ctx, func_body, args),
        Value::FFI(f) => f(ctx, span, args),
        _ => Err(Error::NotAFunction { span, found: func }),
    }
}
pub fn inner_func_call<'a>(
    ctx: &mut Context<'a>,
    func_body: &'a FuncBody,
    args: Vec<Value<'a>>,
) -> Result<Value<'a>, Error<'a>> {
    let mut scope = Scope::default();
    for (arg, val) in func_body.args.iter().zip(args) {
        let val = val.coerce(&arg.typ)?;
        scope.insert(arg.name.as_str(), val);
    }
    ctx.scopes.push(scope);
    let res = match eval_stmts(ctx, &func_body.body) {
        Ok(()) => Value::Nil,
        Err(Error::Return { value, .. }) => value,
        Err(Error::Break { span }) => return Err(Error::MalformedControlFlow { span }),
        Err(error) => return Err(error),
    };
    ctx.pop();
    Ok(res)
}

fn eval_expr<'a>(ctx: &mut Context<'a>, expr: &'a Spanned<Expr>) -> Result<Value<'a>, Error<'a>> {
    let val = match &expr.data {
        Expr::Nil => Value::Nil,
        Expr::True => Value::Boolean(true),
        Expr::False => Value::Boolean(false),
        Expr::Float(val) => Value::Float(*val),
        Expr::Integer(val) => Value::Integer(*val),
        Expr::String(val) => Value::String(Rc::from(val.to_owned())),
        Expr::Identifier(ident) => ctx.get(ident, expr.span)?,
        Expr::List { elements } => {
            let mut res = Vec::new();
            for e in &elements.data {
                let e = eval_expr(ctx, e)?;
                res.push(e);
            }
            Value::List(Rc::new(RefCell::new(res)))
        }
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
            Value::Table(Rc::new(Table { named, indexed }))
        }
        Expr::UnOp { expr, op } => {
            let val = eval_expr(ctx, expr)?;
            match (op.data, val) {
                (UnOp::Neg, Value::Float(val)) => Value::Float(-val),
                (UnOp::Neg, Value::Integer(val)) => Value::Integer(-val),
                (UnOp::Not, Value::Boolean(val)) => Value::Boolean(!val),
                (op, val) => todo!("{op:?} {val:?}"),
            }
        }
        Expr::Member { expr, member } => {
            let span = expr.span;
            let val = eval_expr(ctx, expr)?;
            match val {
                Value::Table(t) => t.named.get(member.as_str()).cloned().unwrap_or(Value::Nil),
                Value::Struct { typ, fields } => {
                    let offset = ctx.protos.offsets.get(&(typ, member.as_str())).unwrap();
                    fields.borrow().get(*offset).unwrap().clone()
                }
                _ => {
                    return Err(Error::TypeMismatch {
                        expected: "Struct",
                        found: val,
                        span,
                    });
                }
            }
        }
        Expr::Index { expr, index } => {
            let span = expr.span;
            let val = eval_expr(ctx, expr)?;
            let idx = eval_expr(ctx, index)?;
            match (&val, idx) {
                (Value::Table(t), Value::Integer(i)) => {
                    t.indexed.get(&i).cloned().unwrap_or(Value::Nil)
                }
                (Value::List(l), Value::Integer(i)) => {
                    let l = l.borrow();
                    if i < 0 || i as usize >= l.len() {
                        return Err(Error::OutOfBound {
                            index: i,
                            len: l.len(),
                            span: index.span,
                        });
                    }
                    unsafe { l.get_unchecked(i as usize).clone() }
                }
                _ => {
                    return Err(Error::TypeMismatch {
                        expected: "List",
                        found: val,
                        span,
                    });
                }
            }
        }
        Expr::BinOp { rhs, lhs, op } => {
            let rhs = eval_expr(ctx, rhs)?;
            let lhs = eval_expr(ctx, lhs)?;
            match (op.data, lhs, rhs) {
                (BinOp::Add, Value::String(l), r) => {
                    let mut buffer = l.to_string();
                    match r {
                        Value::Nil => {
                            buffer.write_str("nil");
                        }
                        Value::Boolean(true) => {
                            buffer.write_str("true");
                        }
                        Value::Boolean(false) => {
                            buffer.write_str("false");
                        }
                        Value::Float(x) => {
                            buffer.write_fmt(format_args!("{x}"));
                        }
                        Value::Integer(x) => {
                            buffer.write_fmt(format_args!("{x}"));
                        }
                        Value::String(x) => {
                            buffer.write_str(x.as_str());
                        }
                        Value::Struct { typ, .. } => {
                            buffer.write_fmt(format_args!("{typ}{{…}}"));
                        }
                        Value::Table(_) => {
                            buffer.write_str("{…}");
                        }
                        Value::List(_) => {
                            buffer.write_str("[…]");
                        }
                        Value::Func(_) => {
                            buffer.write_str("fn{…}");
                        }
                        Value::FFI(_) => {
                            buffer.write_str("ffi{…}");
                        }
                    }
                    Value::String(Rc::from(buffer))
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

                (BinOp::Add, Value::List(l), Value::List(r)) => {
                    let l = l.borrow();
                    let r = r.borrow();
                    let mut res = Vec::with_capacity(l.len() + r.len());
                    res.extend_from_slice(&l);
                    res.extend_from_slice(&r);
                    Value::List(Rc::new(RefCell::new(res)))
                }

                (op, l, r) => todo!("{op:?} {l:?} {r:?}"),
            }
        }
        Expr::TypeConstructor { name, fields } => {
            let typ = name.as_str();
            let size = ctx.protos.sizes.get(typ).unwrap();
            let mut holder = vec![Value::Nil; *size].into_boxed_slice();
            for field in &fields.data {
                let (field_name, val) = match &field.data {
                    FieldConstructor::Implicit(ident) => {
                        let val = ctx.get(ident, field.span)?;
                        (ident.as_str(), val)
                    }
                    FieldConstructor::Explicit { name, expr } => {
                        let val = eval_expr(ctx, expr)?;
                        (name.as_str(), val)
                    }
                };
                let mut offset = ctx.protos.offsets.get(&(typ, field_name)).unwrap();
                holder[*offset] = val;
            }
            Value::Struct {
                typ,
                fields: Rc::new(RefCell::new(holder)),
            }
        }
        Expr::Call { expr, args } => return func_call(ctx, expr, args),
        Expr::Func { body } => todo!(),
    };
    Ok(val)
}
