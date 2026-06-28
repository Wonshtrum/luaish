use std::fmt;

use crate::source::{Source, Span, Spanned};

#[derive(Debug)]
pub enum Error {
    Unexpected(Span),
}

impl Error {
    pub fn pretty_print<W: fmt::Write>(&self, source: &Source, out: &mut W) -> fmt::Result {
        match self {
            Self::Unexpected(span) => {
                out.write_fmt(format_args!(
                    "error: Unexpected character `{}`\n",
                    source.substring(span.start, span.end)
                ))?;
                source.print_span(*span, out)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // only for highlighting
    Comment,

    // Keywords
    Break,
    For,
    Do,
    // While,
    End,

    If,
    Then,
    Else,
    ElseIf,

    Function,
    Local,
    Return,
    Struct,

    // Literals
    Nil,
    True,
    False,
    Float(f64),
    Integer(u64),
    String(String),
    Identifier(String),

    // Symbols
    Dot,
    Comma,
    Colon,
    DoubleColon,
    Semicolon,

    // Misc operators
    In,
    Assign,
    Concat,

    // Logic operators
    And,
    Or,
    Not,

    // Arithmetic operators
    Add,
    Minus,
    Times,
    Divide,
    Modulo,

    // Bitwise operators
    BitAnd,
    BitOr,
    BitXor,
    LShift,
    RShift,

    // Comparison operators
    EQ,
    NE,
    LT,
    GT,
    LE,
    GE,

    // Delimiters
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,

    // End of input
    Eof,
}
impl Token {
    pub fn same_kind_as(&self, other: &Token) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

pub struct Lexer<'a> {
    pub source: &'a Source,
    pub pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a Source) -> Lexer<'a> {
        Lexer { source, pos: 0 }
    }

    fn span(&self, start: usize) -> Span {
        Span {
            start,
            end: self.pos,
        }
    }

    fn current(&self) -> Option<char> {
        if self.pos < self.source.len() {
            Some(self.source[self.pos])
        } else {
            None
        }
    }

    fn peek(&self, offset: usize) -> Option<char> {
        let pos = self.pos + offset;
        if pos < self.source.len() {
            Some(self.source[pos])
        } else {
            None
        }
    }

    #[inline]
    fn advance(&mut self) {
        self.pos += 1;
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.current() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn read_identifier(&mut self) -> String {
        let mut result = String::new();
        while let Some(c) = self.current() {
            if c.is_alphanumeric() || c == '_' {
                result.push(c);
                self.advance();
            } else {
                break;
            }
        }
        result
    }

    fn read_integer(&mut self) -> u64 {
        let mut result = 0;
        let mut nb_digits = 0;
        while let Some(c) = self.current() {
            if c.is_ascii_digit() {
                nb_digits += 1;
                result = 10 * result + (c as u8 - b'0') as u64;
                self.advance();
            } else {
                break;
            }
        }
        result
    }

    pub fn next_token(&mut self) -> Result<Spanned<Token>, Error> {
        self.skip_whitespace();

        let start = self.pos;
        let data = match self.current() {
            None => Token::Eof,
            Some('"') => {
                let mut s = String::new();
                // let mut escaped = false;
                loop {
                    self.advance();
                    match self.current() {
                        Some('"') => break,
                        Some(c) => s.push(c),
                        None => {
                            return Err(Error::Unexpected(Span {
                                start,
                                end: self.pos,
                            }));
                        }
                    }
                }
                self.advance();
                Token::String(s)
            }
            Some('/') => {
                self.advance();
                if self.current() == Some('/') {
                    self.advance();
                    while let Some(c) = self.current() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                    Token::Comment
                } else {
                    Token::Divide
                }
            }
            Some('=') => {
                self.advance();
                if self.current() == Some('=') {
                    self.advance();
                    Token::EQ
                } else {
                    Token::Assign
                }
            }
            Some(':') => {
                self.advance();
                if self.current() == Some(':') {
                    self.advance();
                    Token::DoubleColon
                } else {
                    Token::Colon
                }
            }
            Some('.') => {
                self.advance();
                if self.current() == Some('.') {
                    self.advance();
                    Token::Concat
                } else {
                    Token::Dot
                }
            }
            Some('<') => {
                self.advance();
                if self.current() == Some('=') {
                    self.advance();
                    Token::LE
                } else {
                    Token::LT
                }
            }
            Some('>') => {
                self.advance();
                if self.current() == Some('=') {
                    self.advance();
                    Token::GE
                } else {
                    Token::GT
                }
            }
            // standard lua uses "~="
            Some('!') if self.peek(1) == Some('=') => {
                self.advance();
                Token::NE
            }
            Some('+') => {
                self.advance();
                Token::Add
            }
            Some('-') => {
                self.advance();
                Token::Minus
            }
            Some('%') => {
                self.advance();
                Token::Modulo
            }
            Some('*') => {
                self.advance();
                Token::Times
            }
            Some(',') => {
                self.advance();
                Token::Comma
            }
            Some(';') => {
                self.advance();
                Token::Semicolon
            }
            Some('(') => {
                self.advance();
                Token::LParen
            }
            Some(')') => {
                self.advance();
                Token::RParen
            }
            Some('[') => {
                self.advance();
                Token::LBracket
            }
            Some(']') => {
                self.advance();
                Token::RBracket
            }
            Some('{') => {
                self.advance();
                Token::LBrace
            }
            Some('}') => {
                self.advance();
                Token::RBrace
            }
            Some(c) if c.is_alphabetic() || c == '_' => {
                let ident = self.read_identifier();
                match ident.as_str() {
                    "break" => Token::Break,
                    "for" => Token::For,
                    "do" => Token::For,
                    "end" => Token::End,
                    "if" => Token::If,
                    "then" => Token::Then,
                    "else" => Token::Else,
                    "elseif" => Token::ElseIf,
                    "function" => Token::Function,
                    "local" => Token::Local,
                    "return" => Token::Return,
                    "struct" => Token::Struct,
                    "in" => Token::In,

                    "nil" => Token::Nil,
                    "true" => Token::True,
                    "false" => Token::False,

                    "not" => Token::Not,
                    "and" => Token::And,
                    "or" => Token::Or,
                    _ => Token::Identifier(ident),
                }
            }
            Some(c) if c.is_numeric() => {
                let num = self.read_integer();
                Token::Integer(num)
            }
            Some(_) => {
                return Err(Error::Unexpected(Span {
                    start,
                    end: start + 1,
                }));
            }
        };
        Ok(Spanned {
            data,
            span: self.span(start),
        })
    }

    pub fn tokenize(&mut self) -> Result<Vec<Spanned<Token>>, Error> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token()?;
            if let Token::Eof = token.data {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }
        Ok(tokens)
    }
}
