use std::fmt::{self, Debug};

use crate::ast::{ArgDef, BinOp, Element, Expr, FieldConstructor, FuncBody, Stmt, Type, UnOp};
use crate::lexer::Token;
use crate::source::{Source, Span, Spanned};

#[derive(Debug)]
pub enum Error {
    Todo {
        message: &'static str,
        span: Span,
    },
    Unexpected {
        message: Option<&'static str>,
        expected: Vec<String>,
        found: Spanned<Token>,
        started: Option<Span>,
    },
}

impl Error {
    pub fn pretty_print<W: fmt::Write>(&self, source: &Source, out: &mut W) -> fmt::Result {
        match self {
            Self::Todo { message, span } => {
                out.write_fmt(format_args!("todo: {message}\n"))?;
                source.print_span(*span, out)
            }
            Self::Unexpected {
                message,
                expected,
                found,
                started,
            } => {
                out.write_str("error: ")?;
                if let Some(message) = message {
                    out.write_fmt(format_args!("Expected {message}"))?;
                    if expected.len() == 1 {
                        out.write_fmt(format_args!(" (token `{}`)", expected[0]))?;
                    } else if expected.len() > 1 {
                        out.write_fmt(format_args!(" (tokens {:?})", expected))?;
                    }
                    out.write_str(", but found")?;
                } else if expected.len() == 1 {
                    out.write_fmt(format_args!("Expected token `{}`, but found", expected[0]))?;
                } else {
                    out.write_str("Unexpected")?;
                }
                out.write_fmt(format_args!(" token `{:?}`\n", found.data))?;
                source.print_span(found.span, out)?;
                if let Some(started) = started {
                    out.write_str("note: Started by\n")?;
                    source.print_span(*started, out)?;
                }
                Ok(())
            }
        }
    }
}

impl Expr {
    fn is_lvalue(&self) -> bool {
        match self {
            Expr::Identifier(_) | Expr::Member { .. } | Expr::Index { .. } => true,
            _ => false,
        }
    }
}

#[repr(u8)]
#[derive(PartialOrd, Ord, PartialEq, Eq)]
enum Precedence {
    Zero = 0,
    Assign = 10,
    LOr = 20,            // ||
    LAnd = 30,           // &&
    Comparison = 40,     // == != < > <= >=
    Additive = 50,       // + -
    Bitwise = 60,        // & |
    BitShift = 70,       // << >>
    Modular = 80,        // %
    Multiplicative = 90, // * /
    Unary = 100,         // prefix operators
    Suffix = 110,        // ++ --
    Member = 120,        // . (field access)
    Call = 130,          // () []
}

pub struct Parser<'a> {
    pub source: &'a Source,
    pub tokens: Vec<Spanned<Token>>,
    pub pos: usize,
    eof: Spanned<Token>,
}

impl<'a> Parser<'a> {
    pub fn new(source: &'a Source, tokens: Vec<Spanned<Token>>) -> Parser<'a> {
        let eof = tokens.last().unwrap().clone();
        let mut pos = 0;
        while pos < tokens.len() && tokens[pos].same_kind_as(&Token::Comment) {
            pos += 1;
        }
        Parser {
            source,
            tokens,
            pos,
            eof,
        }
    }

    fn current(&self) -> &Spanned<Token> {
        self.tokens.get(self.pos).unwrap_or(&self.eof)
    }

    fn peek(&self, mut n: usize) -> &Spanned<Token> {
        let mut i = 0;
        loop {
            let Some(token) = self.tokens.get(self.pos + i) else {
                return &self.eof;
            };
            if n == 0 {
                return token;
            }
            if !token.same_kind_as(&Token::Comment) {
                n -= 1;
            }
            i += 1;
        }
    }

    fn advance(&mut self) {
        loop {
            self.pos += 1;
            let Some(token) = self.tokens.get(self.pos) else {
                return;
            };
            if !token.same_kind_as(&Token::Comment) {
                return;
            }
        }
    }

    fn expect_type(&mut self, message: Option<&'static str>) -> Result<Spanned<Type>, Error> {
        let mut nesting = 0;
        let mut starts = Vec::new();
        let span = self.current().span;
        while self.current().same_kind_as(&Token::LBracket) {
            starts.push(self.current().span);
            self.advance();
            nesting += 1;
        }
        let mut last_span = span;
        let typ = if self.current().same_kind_as(&Token::Function) {
            self.advance();
            self.expect(Token::LParen, None, None)?;
            let mut args = Vec::new();
            while !self.current().same_kind_as(&Token::RParen) {
                let arg = self.expect_type(Some("parameter type"))?;
                args.push(arg);
                if self.current().same_kind_as(&Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            last_span = self.expect(Token::RParen, None, None)?;
            let ret = if self.current().same_kind_as(&Token::Colon) {
                self.advance();
                let ret = self.expect_type(Some("return type"))?;
                last_span = ret.span;
                Some(Box::new(ret))
            } else {
                None
            };
            Type::Function { nesting, args, ret }
        } else {
            let name = self.expect_identifier(message)?;
            Type::Named { nesting, name }
        };
        for i in 0..nesting {
            last_span = self.expect(Token::RBracket, None, Some(starts[starts.len() - i - 1]))?;
        }
        let typ = span.merge(last_span).attach(typ);
        Ok(typ)
    }

    fn expect_identifier(
        &mut self,
        message: Option<&'static str>,
    ) -> Result<Spanned<String>, Error> {
        let token = self.current();
        let Token::Identifier(ident) = &token.data else {
            return Err(Error::Unexpected {
                message,
                expected: vec!["Identifier".to_string()],
                found: token.to_owned(),
                started: None,
            });
        };
        let ident = token.span.attach(ident.to_owned());
        self.advance();
        Ok(ident)
    }

    fn expect(
        &mut self,
        expected: Token,
        message: Option<&'static str>,
        started: Option<Span>,
    ) -> Result<Span, Error> {
        if !self.current().same_kind_as(&expected) {
            return Err(Error::Unexpected {
                message,
                expected: vec![format!("{expected:?}")],
                found: self.current().clone(),
                started,
            });
        }
        let span = self.current().span;
        self.advance();
        Ok(span)
    }

    pub fn parse(&mut self) -> Result<Vec<Spanned<Stmt>>, Error> {
        let mut stmts = Vec::new();
        while !self.current().same_kind_as(&Token::Eof) {
            let stmt = self.parse_stmt()?;
            stmts.push(stmt);
        }
        self.expect(Token::Eof, None, None)?;
        Ok(stmts)
    }

    pub fn parse_block(&mut self) -> Result<Spanned<Vec<Spanned<Stmt>>>, Error> {
        let mut stmts = Vec::new();
        let span = self.current().span;
        let mut last_span = span;
        loop {
            match self.current().data {
                Token::Eof | Token::End | Token::ElseIf | Token::Else => break,
                _ => {}
            }
            let stmt = self.parse_stmt()?;
            last_span = stmt.span;
            stmts.push(stmt);
        }
        let stmts = span.merge(last_span).attach(stmts);
        Ok(stmts)
    }

    pub fn parse_stmt(&mut self) -> Result<Spanned<Stmt>, Error> {
        let token = self.current();
        let span = token.span;
        let (is_local, token) = if let Token::Local = token.data {
            self.advance();
            (true, self.current())
        } else {
            (false, token)
        };
        let expected_local = || Error::Unexpected {
            message: None,
            expected: vec![],
            found: token.to_owned(),
            started: None,
        };
        let expr = match &token.data {
            Token::Break => {
                if is_local {
                    return Err(expected_local());
                }
                self.advance();
                span.attach(Stmt::Break)
            }
            Token::Return => {
                if is_local {
                    return Err(expected_local());
                }
                self.advance();
                match &self.current().data {
                    Token::End | Token::ElseIf | Token::Else => {
                        span.attach(Stmt::Return { expr: None })
                    }
                    _ => {
                        let expr = self.parse_expr()?;
                        span.merge(expr.span)
                            .attach(Stmt::Return { expr: Some(expr) })
                    }
                }
            }
            Token::Struct => {
                if is_local {
                    return Err(expected_local());
                }
                self.advance();
                let name = self.expect_identifier(Some("type name"))?;
                let mut fields = Vec::new();
                let field_start = self.current().span;
                let mut field_end = field_start;
                while !self.current().same_kind_as(&Token::End) {
                    let name = self.expect_identifier(Some("field name"))?;
                    self.expect(Token::Colon, None, None)?;
                    let typ = self.expect_type(Some("field type"))?;
                    field_end = typ.span;
                    fields.push(name.span.merge(typ.span).attach(ArgDef { name, typ }));
                }
                let fields = field_start.merge(field_end).attach(fields);
                let last_span = self.current().span;
                self.advance();
                span.merge(last_span).attach(Stmt::TypeDef { name, fields })
            }
            Token::Function => {
                self.advance();
                let name = self.expect_identifier(Some("function name"))?;
                self.expect(Token::LParen, None, None)?;
                let mut args = Vec::new();
                let args_start = self.current().span;
                let mut args_end = args_start;
                while !self.current().same_kind_as(&Token::RParen) {
                    let name = self.expect_identifier(Some("arg name"))?;
                    self.expect(Token::Colon, None, None)?;
                    let typ = self.expect_type(Some("arg type"))?;
                    args_end = typ.span;
                    args.push(name.span.merge(typ.span).attach(ArgDef { name, typ }));
                }
                let args = args_start.merge(args_end).attach(args);
                let last_span = self.current().span;
                self.expect(Token::RParen, None, None)?;
                let ret = if self.current().same_kind_as(&Token::Colon) {
                    self.advance();
                    let typ = self.expect_type(Some("return type"))?;
                    Some(typ)
                } else {
                    None
                };

                let body = self.parse_block()?;
                let end = self.expect(Token::End, None, None)?;
                span.merge(end).attach(Stmt::FuncDef {
                    is_local,
                    name,
                    body: FuncBody { args, ret, body },
                })
            }
            Token::If => {
                if is_local {
                    return Err(expected_local());
                }
                self.advance();
                let condition = self.parse_expr()?;
                self.expect(Token::Then, None, None)?;
                let then_block = self.parse_block()?;
                let mut else_if_blocks = Vec::new();
                while self.current().same_kind_as(&Token::ElseIf) {
                    self.advance();
                    let condition = self.parse_expr()?;
                    self.expect(Token::Then, None, None)?;
                    let then_block = self.parse_block()?;
                    else_if_blocks.push((condition, then_block));
                }
                let else_block = if self.current().same_kind_as(&Token::Else) {
                    self.advance();
                    let else_block = self.parse_block()?;
                    Some(else_block)
                } else {
                    None
                };
                let end = self.expect(Token::End, None, None)?;
                span.merge(end).attach(Stmt::If {
                    condition: Box::new(condition),
                    then_block,
                    else_if_blocks,
                    else_block,
                })
            }
            // Token::Identifier(ident) => {
            //     let ident = span.attach(ident.to_owned());
            //     self.advance();
            //     let token = self.current();
            //     let lhs_span = span;
            //     if token.same_kind_as(&Token::LParen) {
            //         if is_local {
            //             return Err(Error::Unexpected {
            //                 message: None,
            //                 expected: vec![],
            //                 found: token.to_owned(),
            //                 started: None,
            //             });
            //         }
            //         self.advance();
            //         let args = self.parse_expr_list(Token::RParen)?;
            //         let last_span =
            //             self.expect(Token::RParen, Some("end of arg list"), Some(span))?;
            //         return Ok(span
            //             .merge(last_span)
            //             .attach(Stmt::Call { name: ident, args }));
            //     }
            //     let mut lhs = vec![ident];
            //     let mut last_span = lhs_span;
            //     while self.current().same_kind_as(&Token::Comma) {
            //         self.advance();
            //         let ident = self.expect_identifier(None)?;
            //         last_span = ident.span;
            //         lhs.push(ident);
            //     }
            //     self.expect(Token::Assign, None, None)?;
            //     let lhs = lhs_span.merge(last_span).attach(lhs);

            //     let expr = self.parse_expr()?;
            //     let rhs_span = expr.span;
            //     let mut last_span = rhs_span;
            //     let mut rhs = vec![expr];
            //     while self.current().same_kind_as(&Token::Comma) {
            //         self.advance();
            //         let expr = self.parse_expr()?;
            //         last_span = expr.span;
            //         rhs.push(expr);
            //     }
            //     let rhs = rhs_span.merge(last_span).attach(rhs);

            //     span.merge(last_span)
            //         .attach(Stmt::Assigns { is_local, lhs, rhs })
            // }
            _ => {
                if is_local {
                    let first = self.expect_identifier(None)?;
                    let start = first.span;
                    let mut names = vec![first];
                    let mut end = start;
                    while self.current().same_kind_as(&Token::Comma) {
                        self.advance();
                        let ident = self.expect_identifier(Some("binding name"))?;
                        end = ident.span;
                        names.push(ident);
                    }
                    self.expect(Token::Assign, None, None)?;
                    let lhs = start.merge(end).attach(names);

                    let expr = self.parse_expr()?;
                    let rhs_span = expr.span;
                    let mut last_span = rhs_span;
                    let mut rhs = vec![expr];
                    while self.current().same_kind_as(&Token::Comma) {
                        self.advance();
                        let expr = self.parse_expr()?;
                        last_span = expr.span;
                        rhs.push(expr);
                    }
                    let rhs = rhs_span.merge(last_span).attach(rhs);
                    return Ok(span.merge(rhs.span).attach(Stmt::Binding { lhs, rhs }));
                }

                let expr = self.parse_expr()?;
                let lhs_span = expr.span;
                if let Expr::Call { expr, args } = expr.data {
                    return Ok(lhs_span.attach(Stmt::Call { expr, args }));
                }

                if !expr.is_lvalue() {
                    return Err(Error::Unexpected {
                        message: Some("lvalue or function call"),
                        expected: vec![],
                        found: self.current().to_owned(),
                        started: None,
                    });
                }

                let mut last_span = span;
                let mut lhs = vec![expr];
                while self.current().same_kind_as(&Token::Comma) {
                    self.advance();
                    let expr = self.parse_expr()?;
                    if !expr.is_lvalue() {
                        return Err(Error::Unexpected {
                            message: Some("lvalue"),
                            expected: vec![],
                            found: self.current().to_owned(),
                            started: None,
                        });
                    }
                    last_span = expr.span;
                    lhs.push(expr);
                }
                self.expect(Token::Assign, None, None)?;
                let lhs = lhs_span.merge(last_span).attach(lhs);

                let expr = self.parse_expr()?;
                let rhs_span = expr.span;
                let mut last_span = rhs_span;
                let mut rhs = vec![expr];
                while self.current().same_kind_as(&Token::Comma) {
                    self.advance();
                    let expr = self.parse_expr()?;
                    last_span = expr.span;
                    rhs.push(expr);
                }
                let rhs = rhs_span.merge(last_span).attach(rhs);

                span.merge(last_span).attach(Stmt::Assign { lhs, rhs })
            }
        };
        Ok(expr)
    }

    fn peek_infix_precedence(&self) -> Option<usize> {
        match self.current().data {
            Token::Add | Token::Minus | Token::Concat => Some(Precedence::Additive),
            Token::Modulo => Some(Precedence::Modular),
            Token::Times | Token::Divide => Some(Precedence::Multiplicative),
            Token::BitAnd | Token::BitOr | Token::BitXor => Some(Precedence::Bitwise),
            Token::LShift | Token::RShift => Some(Precedence::BitShift),
            Token::And => Some(Precedence::LAnd),
            Token::Or => Some(Precedence::LOr),
            Token::LParen | Token::LBracket | Token::LBrace => Some(Precedence::Call),
            Token::Dot | Token::Colon => Some(Precedence::Member),
            Token::EQ | Token::NE | Token::LT | Token::GT | Token::LE | Token::GE => {
                Some(Precedence::Comparison)
            }
            _ => None,
        }
        .map(|p| p as usize)
    }

    fn parse_expr(&mut self) -> Result<Spanned<Expr>, Error> {
        self.parse_expr_inner(0)
    }

    fn parse_expr_inner(&mut self, min_precedence: usize) -> Result<Spanned<Expr>, Error> {
        let mut expr = self.parse_primary()?;
        while let Some(op_precedence) = self.peek_infix_precedence() {
            if op_precedence < min_precedence {
                break;
            }
            expr = self.parse_infix(expr, op_precedence)?;
        }
        Ok(expr)
    }

    fn parse_infix(
        &mut self,
        lhs: Spanned<Expr>,
        precedence: usize,
    ) -> Result<Spanned<Expr>, Error> {
        let token = self.current();
        let op = match token.data {
            Token::Add => BinOp::Add,
            Token::Minus => BinOp::Sub,
            Token::Concat => BinOp::Concat,
            Token::Times => BinOp::Mul,
            Token::Divide => BinOp::Div,
            Token::Modulo => BinOp::Mod,
            Token::BitAnd => BinOp::BitAnd,
            Token::BitOr => BinOp::BitOr,
            Token::BitXor => BinOp::BitXor,
            Token::LShift => BinOp::Shl,
            Token::RShift => BinOp::Shr,
            Token::And => BinOp::And,
            Token::Or => BinOp::Or,
            Token::EQ => BinOp::EQ,
            Token::NE => BinOp::NE,
            Token::LT => BinOp::LT,
            Token::GT => BinOp::GT,
            Token::LE => BinOp::LE,
            Token::GE => BinOp::GE,
            Token::Dot | Token::Colon => {
                self.advance();
                let member = self.expect_identifier(Some("member"))?;
                let expr = lhs.span.merge(member.span).attach(Expr::Member {
                    expr: Box::new(lhs),
                    member,
                });
                return Ok(expr);
            }
            Token::LParen => {
                let span = token.span;
                self.advance();
                let args = self.parse_expr_list(Token::RParen)?;
                let last_span =
                    self.expect(Token::RParen, Some("end of function call"), Some(span))?;
                let expr = lhs.span.merge(last_span).attach(Expr::Call {
                    expr: Box::new(lhs),
                    args,
                });
                return Ok(expr);
            }
            Token::LBracket => {
                let span = token.span;
                self.advance();
                let index = self.parse_expr()?;
                let last_span =
                    self.expect(Token::RBracket, Some("end of indexing"), Some(span))?;
                let expr = lhs.span.merge(last_span).attach(Expr::Index {
                    expr: Box::new(lhs),
                    index: Box::new(index),
                });
                return Ok(expr);
            }
            _ => unreachable!(),
        };
        let op = token.span.attach(op);
        self.advance();
        let rhs = self.parse_expr_inner(precedence + 1)?;
        let expr = lhs.span.merge(rhs.span).attach(Expr::BinOp {
            rhs: Box::new(rhs),
            lhs: Box::new(lhs),
            op,
        });
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Spanned<Expr>, Error> {
        let span = self.current().span;
        let expr = match &self.current().data {
            Token::Nil => {
                self.advance();
                span.attach(Expr::Nil)
            }
            Token::True => {
                self.advance();
                span.attach(Expr::True)
            }
            Token::False => {
                self.advance();
                span.attach(Expr::False)
            }
            Token::Integer(n) => {
                let n = *n;
                self.advance();
                span.attach(Expr::Integer(n as i64))
            }
            Token::String(s) => {
                let s = s.to_owned();
                self.advance();
                span.attach(Expr::String(s))
            }
            Token::Add => {
                self.advance();
                self.parse_expr_inner(Precedence::Unary as usize)?
            }
            Token::Minus => {
                self.advance();
                let expr = self.parse_expr_inner(Precedence::Unary as usize)?;
                span.merge(expr.span).attach(Expr::UnOp {
                    expr: Box::new(expr),
                    op: span.attach(UnOp::Neg),
                })
            }
            Token::Not => {
                self.advance();
                let expr = self.parse_expr_inner(Precedence::Unary as usize)?;
                span.merge(expr.span).attach(Expr::UnOp {
                    expr: Box::new(expr),
                    op: span.attach(UnOp::Not),
                })
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(Token::RParen, None, None)?;
                expr
            }
            Token::LBracket => {
                self.advance();
                let elements = self.parse_expr_list(Token::RBracket)?;
                let last_span = self.expect(Token::RBracket, Some("end of list"), Some(span))?;
                span.merge(last_span).attach(Expr::List { elements })
            }
            Token::LBrace => {
                self.advance();
                let mut elements = Vec::new();
                while !self.current().same_kind_as(&Token::RBrace) {
                    let token = self.current();
                    match &token.data {
                        Token::Identifier(name) if self.peek(1).same_kind_as(&Token::Assign) => {
                            let name = token.span.attach(name.to_owned());
                            self.advance();
                            self.advance();
                            let expr = self.parse_expr()?;
                            elements.push(
                                name.span
                                    .merge(expr.span)
                                    .attach(Element::Named { name, expr }),
                            );
                        }
                        _ => {
                            let expr = self.parse_expr()?;
                            elements.push(expr.span.attach(Element::Indexed(expr)));
                        }
                    }
                    if !self.current().same_kind_as(&Token::Comma) {
                        break;
                    }
                    self.advance();
                }
                let last_span = self.expect(Token::RBrace, None, None)?;
                span.merge(last_span).attach(Expr::Table { elements })
            }
            Token::Identifier(name) => {
                let name = name.to_owned();
                self.advance();
                let started = self.current().span;
                match self.current().data {
                    // Token::LParen => {
                    //     self.advance();
                    //     let args = self.parse_expr_list(Token::RParen)?;
                    //     let last_span =
                    //         self.expect(Token::RParen, Some("end of constructor"), Some(started))?;
                    //     let name = span.attach(name);
                    //     span.merge(last_span).attach(Expr::Call { name, args })
                    // }
                    Token::LBrace => {
                        self.advance();
                        let mut fields = Vec::new();
                        let field_start = self.current().span;
                        let mut field_end = field_start;
                        while !self.current().same_kind_as(&Token::RBrace) {
                            let name = self.expect_identifier(Some("field"))?;
                            if !self.current().same_kind_as(&Token::Colon) {
                                fields
                                    .push(name.span.attach(FieldConstructor::Implicit(name.data)));
                            } else {
                                self.advance();
                                let expr = self.parse_expr()?;
                                field_end = expr.span;
                                fields.push(
                                    name.span
                                        .merge(expr.span)
                                        .attach(FieldConstructor::Explicit { name, expr }),
                                );
                            }
                            if self.current().same_kind_as(&Token::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        let fields = field_start.merge(field_end).attach(fields);
                        let last_span = self.expect(Token::RBrace, None, None)?;
                        span.merge(last_span).attach(Expr::TypeConstructor {
                            name: span.attach(name),
                            fields,
                        })
                    }
                    _ => {
                        let ident = Expr::Identifier(name);
                        span.attach(ident)
                    }
                }
            }
            _ => {
                return Err(Error::Unexpected {
                    expected: Vec::new(),
                    message: Some("primary expression"),
                    found: self.current().clone(),
                    started: None,
                });
            }
        };
        Ok(expr)
    }

    fn parse_expr_list(&mut self, terminator: Token) -> Result<Spanned<Vec<Spanned<Expr>>>, Error> {
        let span = self.current().span;
        let mut last_span = span;
        let mut exprs = Vec::new();
        while !self.current().same_kind_as(&terminator) {
            let expr = self.parse_expr()?;
            last_span = expr.span;
            exprs.push(expr);
            if self.current().same_kind_as(&Token::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(span.merge(last_span).attach(exprs))
    }

    // fn parse_block(&mut self) -> Result<Spanned<Block>, Error> {
    //     let span = self.expect(Token::LBrace, Some("start of block"), None)?;
    //     let mut exprs = Vec::new();
    //     let mut value = None;
    //     while self.current() != Token::RBrace {
    //         let expr = self.parse_expr()?;
    //         if self.current() == Token::Semicolon {
    //             self.advance();
    //             exprs.push(expr);
    //         } else {
    //             value = Some(Box::new(expr));
    //             break;
    //         }
    //     }
    //     let last_span = self.expect(Token::RBrace, Some("end of block"), Some(span))?;
    //     Ok(span.merge(last_span).attach(Block { exprs, value }))
    // }
}
