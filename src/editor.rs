use std::io::{self, Stdout, Write};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    terminal,
};

use crate::log;
use crate::logger::LOGGER;

use crate::ast::{Element, Expr, FieldConstructor, Stmt};
use crate::interpreter;
use crate::lexer::{self, Lexer, Token};
use crate::parser::{self, Parser};
use crate::resolution;
use crate::source::{Source, Span, Spanned};

// ---------------------------------------------------------------------------
// Token foreground color
// ---------------------------------------------------------------------------

fn token_color(kind: &Token) -> Color {
    match kind {
        Token::Comment => Color::Cyan,

        // Literals
        Token::Nil
        | Token::True
        | Token::False
        | Token::Integer(_)
        | Token::Float(_)
        | Token::String(_) => Color::DarkRed,

        // Identifiers and paths
        Token::Identifier(_) | Token::DoubleColon => Color::DarkMagenta,

        Token::Break
        | Token::For
        | Token::In
        | Token::End
        | Token::If
        | Token::Then
        | Token::Else
        | Token::ElseIf
        | Token::Function
        | Token::Local
        | Token::Return
        | Token::Struct => Color::AnsiValue(130),

        // Keywords and operators
        Token::Not
        | Token::And
        | Token::Or
        | Token::Add
        | Token::Minus
        | Token::Times
        | Token::Divide
        | Token::Modulo
        | Token::BitAnd
        | Token::BitOr
        | Token::BitXor
        | Token::LShift
        | Token::RShift
        | Token::EQ
        | Token::NE
        | Token::GT
        | Token::LT
        | Token::GE
        | Token::LE => Color::DarkGreen,

        _ => Color::AnsiValue(236),
    }
}

// ---------------------------------------------------------------------------
// Lightweight AST cursor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct AstNode {
    label: &'static str,
    span: Span,
    weak: bool,
}

// ---------------------------------------------------------------------------
// Compiler output
// ---------------------------------------------------------------------------

struct CompilerOutput {
    errors: Vec<Diagnostic>,
    result: Result<
        (
            Vec<Spanned<Token>>,
            Result<(Vec<Spanned<Stmt>>, ()), String>,
        ),
        String,
    >,
}
impl CompilerOutput {
    fn tokens(&self) -> &[Spanned<Token>] {
        match &self.result {
            Ok((tokens, _)) => tokens,
            _ => &[],
        }
    }
}

#[derive(Debug, Clone)]
struct Diagnostic {
    span: Span,
    message: String,
}

// ---------------------------------------------------------------------------
// Panes
// ---------------------------------------------------------------------------

trait Pane {
    fn label(&self) -> &str;
    fn status(&self, ctx: &EditorContext) -> String;

    fn on_focus(&mut self, _ctx: &mut EditorContext) {}
    fn on_key(&mut self, _ctx: &mut EditorContext, _key: KeyEvent) {}

    fn render(
        &self,
        ctx: &EditorContext,
        stdout: &mut Stdout,
        top_y: u16,
        rows: usize,
        cols: usize,
    ) -> io::Result<()>;
}

struct BasePane {
    cursor: usize,
    scroll_row: usize,
}
impl BasePane {
    fn new() -> Self {
        Self {
            cursor: 0,
            scroll_row: 0,
        }
    }
    fn status(&self, source: &Source) -> String {
        let pos = source.index_to_position(self.cursor);
        format!(" {}:{} │ {}", pos.row + 1, pos.col + 1, source.len())
    }
    fn on_key(&mut self, source: &Source, key: KeyEvent, rows: usize) {
        match key.code {
            KeyCode::Left => self.move_cursor_by(source, -1),
            KeyCode::Right => self.move_cursor_by(source, 1),
            KeyCode::Up => self.move_cursor_line(source, -1),
            KeyCode::Down => self.move_cursor_line(source, 1),
            KeyCode::Home => self.move_cursor_home(source),
            KeyCode::End => self.move_cursor_end(source),
            KeyCode::Enter => self.move_cursor_line(source, 1),
            KeyCode::Backspace => self.move_cursor_line(source, -1),
            KeyCode::PageUp => self.move_cursor_line(source, -20),
            KeyCode::PageDown => self.move_cursor_line(source, 20),
            _ => {}
        }
        self.update_scroll(source, rows);
    }

    fn update_scroll(&mut self, source: &Source, rows: usize) {
        let row = source.index_to_position(self.cursor).row;
        if row < self.scroll_row {
            self.scroll_row = row;
        } else if row >= self.scroll_row + rows {
            self.scroll_row = row + 1 - rows;
        }
    }

    fn move_cursor_by(&mut self, source: &Source, delta: isize) {
        self.cursor = (self.cursor as isize + delta).clamp(0, source.len() as isize) as usize;
    }
    fn move_cursor_line(&mut self, source: &Source, delta: isize) {
        let pos = source.index_to_position(self.cursor);
        let new_row =
            (pos.row as isize + delta).clamp(0, source.line_count() as isize - 1) as usize;
        self.cursor = source.position_to_index(new_row, pos.col);
    }
    fn move_cursor_home(&mut self, source: &Source) {
        self.cursor = source.index_to_position(self.cursor).line_start;
    }
    fn move_cursor_end(&mut self, source: &Source) {
        self.cursor = source.index_to_position(self.cursor).line_end;
    }

    fn render(
        &self,
        source: &Source,
        stdout: &mut Stdout,
        top_y: u16,
        rows: usize,
        cols: usize,
    ) -> io::Result<()> {
        const GUTTER: usize = 4;
        let text_cols = cols.saturating_sub(GUTTER);
        let cursor_row = source.index_to_position(self.cursor).row;

        let visible: Vec<(usize, usize)> = source
            .lines()
            .enumerate()
            .filter(|(i, _)| *i >= self.scroll_row && *i < self.scroll_row + rows)
            .map(|(_, b)| b)
            .collect();

        for (screen_row, (line_start, line_end)) in visible.iter().enumerate() {
            let abs_row = self.scroll_row + screen_row;
            let y = top_y + screen_row as u16;
            let text: String = source.buffer[*line_start..*line_end]
                .iter()
                .take(text_cols)
                .collect();
            let padded = format!("{text:<text_cols$}");

            queue!(
                stdout,
                cursor::MoveTo(0, y),
                SetForegroundColor(Color::AnsiValue(130)),
                Print(format!("{:3} ", abs_row + 1)),
                ResetColor,
            )?;

            if abs_row == cursor_row {
                queue!(
                    stdout,
                    SetBackgroundColor(Color::AnsiValue(236)),
                    SetForegroundColor(Color::White),
                    Print(&padded),
                    ResetColor,
                )?;
            } else {
                queue!(
                    stdout,
                    SetForegroundColor(Color::AnsiValue(236)),
                    Print(&padded),
                    ResetColor
                )?;
            }
        }

        for blank in visible.len()..rows {
            queue!(
                stdout,
                cursor::MoveTo(0, top_y + blank as u16),
                Print(format!("{:width$}", "", width = cols)),
            )?;
        }

        Ok(())
    }
}

struct LogsPane(BasePane);
impl LogsPane {
    fn new() -> Self {
        Self(BasePane::new())
    }
}
impl Pane for LogsPane {
    fn label(&self) -> &str {
        "Logs"
    }
    fn status(&self, _ctx: &EditorContext) -> String {
        self.0.status(&LOGGER.lock().unwrap())
    }
    fn on_key(&mut self, ctx: &mut EditorContext, key: KeyEvent) {
        let rows = ctx.content_rows() as usize;
        if key.code == KeyCode::Enter {
            LOGGER.lock().unwrap().append("\n");
        }
        self.0.on_key(&LOGGER.lock().unwrap(), key, rows);
    }
    fn on_focus(&mut self, ctx: &mut EditorContext) {
        let source = LOGGER.lock().unwrap();
        self.0.cursor = source.len();
        self.0.update_scroll(&source, ctx.content_rows() as usize);
    }
    fn render(
        &self,
        _ctx: &EditorContext,
        stdout: &mut Stdout,
        top_y: u16,
        rows: usize,
        cols: usize,
    ) -> io::Result<()> {
        self.0
            .render(&LOGGER.lock().unwrap(), stdout, top_y, rows, cols)
    }
}

struct TextPane {
    label: &'static str,
    base: BasePane,
    version: u64,
    source: Source,
    generate: fn(&EditorContext) -> String,
}
impl TextPane {
    fn new(label: &'static str, generate: fn(&EditorContext) -> String) -> Self {
        Self {
            label,
            base: BasePane::new(),
            source: Source::empty(String::new()),
            version: 0,
            generate,
        }
    }
}
impl Pane for TextPane {
    fn label(&self) -> &str {
        self.label
    }
    fn status(&self, _ctx: &EditorContext) -> String {
        self.base.status(&self.source)
    }
    fn on_focus(&mut self, ctx: &mut EditorContext) {
        if self.version < ctx.version {
            let s = (self.generate)(ctx);
            self.source = Source::new(String::new(), &s);
            self.version = ctx.version;
        }
    }
    fn on_key(&mut self, ctx: &mut EditorContext, key: KeyEvent) {
        let rows = ctx.content_rows() as usize;
        self.base.on_key(&self.source, key, rows);
    }
    fn render(
        &self,
        _ctx: &EditorContext,
        stdout: &mut Stdout,
        top_y: u16,
        rows: usize,
        cols: usize,
    ) -> io::Result<()> {
        self.base.render(&self.source, stdout, top_y, rows, cols)
    }
}

// ---------------------------------------------------------------------------
// Editor state
// ---------------------------------------------------------------------------

pub struct EditorContext {
    pub version: u64,
    pub source: Source,
    is_focus: bool,
    output: CompilerOutput,
    ast_path: Vec<AstNode>,
    ast_cursor: Option<usize>,
    term_size: (u16, u16),
}

pub struct Editor {
    pub ctx: EditorContext,
    panes: Vec<Box<dyn Pane>>,
    active: usize,
}

impl Editor {
    pub fn new(text: &str) -> Self {
        Self::from_source(Source::new("<editor>".into(), text))
    }

    pub fn from_source(source: Source) -> Self {
        let term_size = terminal::size().unwrap_or((80, 24));
        let mut ctx = EditorContext {
            version: 1,
            source,
            is_focus: true,
            output: CompilerOutput {
                errors: Vec::new(),
                result: Err("".into()),
            },
            ast_cursor: None,
            ast_path: Vec::new(),
            term_size,
        };
        log!("started");
        ctx.recompile();
        if let Ok((_token, Ok((ast, _)))) = &ctx.output.result {
            ctx.ast_path = ast_path_at(ast, 0);
        }
        Self {
            ctx,
            panes: vec![
                Box::new(EditorPane::new()),
                Box::new(LogsPane::new()),
                Box::new(TextPane::new("Lexer", |ctx| match &ctx.output.result {
                    Err(error) => error.clone(),
                    Ok((tokens, _)) => format!("{tokens:#?}"),
                })),
                Box::new(TextPane::new("Parser", |ctx| match &ctx.output.result {
                    Err(error) | Ok((_, Err(error))) => error.clone(),
                    Ok((_tokens, Ok((ast, _)))) => format!("{ast:#?}"),
                })),
                Box::new(TextPane::new("Resolution", |ctx| {
                    match &ctx.output.result {
                        Err(error) | Ok((_, Err(error))) => error.clone(),
                        Ok((_tokens, Ok((ast, _)))) => match resolution::run(ast) {
                            Ok(res) => format!("{res:#?}"),
                            Err(error) => {
                                let mut out = String::new();
                                let _ = error.pretty_print(&ctx.source, &mut out);
                                out
                            }
                        },
                    }
                })),
                Box::new(TextPane::new("Exec", |ctx| match &ctx.output.result {
                    Err(error) | Ok((_, Err(error))) => error.clone(),
                    Ok((_tokens, Ok((ast, _)))) => match interpreter::run(ast) {
                        Ok(res) => res,
                        Err(error) => {
                            let mut out = String::new();
                            let _ = error.pretty_print(&ctx.source, &mut out);
                            out
                        }
                    },
                })),
            ],
            active: 0,
        }
    }

    // -----------------------------------------------------------------------
    // Main loop
    // -----------------------------------------------------------------------

    pub fn run(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        terminal::enable_raw_mode()?;
        install_panic_hook();
        execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide,)?;

        self.render(&mut stdout)?;

        loop {
            execute!(stdout, event::EnableFocusChange, event::EnableMouseCapture)?;
            let e = event::read()?;
            // log!("{e:?}");
            match e {
                Event::Key(key) => {
                    if self.handle_key(key) {
                        break;
                    }
                }
                Event::Resize(w, h) => {
                    self.ctx.term_size = (w, h);
                }
                Event::FocusGained => self.ctx.is_focus = true,
                Event::FocusLost => self.ctx.is_focus = false,
                _ => {}
            }
            self.render(&mut stdout)?;
        }

        Self::cleanup(&mut stdout)
    }

    fn cleanup(stdout: &mut Stdout) -> io::Result<()> {
        execute!(stdout, terminal::LeaveAlternateScreen, cursor::Show)?;
        terminal::disable_raw_mode()?;
        stdout.flush()?;
        Ok(())
    }

    fn render(&self, stdout: &mut Stdout) -> io::Result<()> {
        let cols = self.ctx.term_size.0 as usize;
        let content_rows = self.ctx.content_rows() as usize;
        let diag_rows = self.ctx.diagnostic_line_count();
        let status_y = self.ctx.term_size.1.saturating_sub(1);

        self.render_tabs(stdout, cols)?;
        self.panes[self.active].render(&self.ctx, stdout, 1, content_rows, cols)?;

        let diag_top = 1u16 + content_rows as u16;
        self.ctx
            .render_diagnostics(stdout, diag_top, cols, diag_rows)?;

        let status = pad(&self.panes[self.active].status(&self.ctx), cols);
        queue!(
            stdout,
            cursor::MoveTo(0, status_y),
            SetBackgroundColor(Color::DarkBlue),
            SetForegroundColor(Color::White),
            Print(status),
            ResetColor,
        )?;

        stdout.flush()
    }

    fn render_tabs(&self, stdout: &mut Stdout, cols: usize) -> io::Result<()> {
        queue!(stdout, cursor::MoveTo(0, 0))?;

        queue!(
            stdout,
            SetBackgroundColor(Color::DarkBlue),
            SetForegroundColor(Color::White),
            Print("  │"),
        )?;
        let mut cursor = 3;
        for (i, pane) in self.panes.iter().enumerate() {
            let mut label = format!(" {}-{} ", i + 1, pane.label());
            if cursor + label.chars().count() > cols {
                label = pad(&label, cols - cursor);
            }
            if i == self.active {
                queue!(
                    stdout,
                    SetBackgroundColor(Color::White),
                    SetForegroundColor(Color::DarkBlue),
                    Print(&label),
                    ResetColor,
                )?;
            } else {
                queue!(
                    stdout,
                    SetBackgroundColor(Color::DarkBlue),
                    SetForegroundColor(Color::White),
                    Print(&label),
                    ResetColor,
                )?;
            }
            cursor += label.chars().count() + 1;
            if cursor >= cols {
                return Ok(());
            }
            queue!(
                stdout,
                SetBackgroundColor(Color::DarkBlue),
                SetForegroundColor(Color::White),
                Print("│"),
            )?;
        }
        queue!(
            stdout,
            SetBackgroundColor(Color::DarkBlue),
            Print(pad("", cols - cursor)),
            ResetColor,
        )?;
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        if ctrl && key.code == KeyCode::Char('c') {
            return true;
        }
        if alt {
            if let KeyCode::Char(c) = key.code {
                if let Some(digit) = c.to_digit(10) {
                    let idx = (digit as usize).saturating_sub(1);
                    if idx != self.active && idx < self.panes.len() {
                        self.active = idx;
                        self.panes[self.active].on_focus(&mut self.ctx);
                    }
                    return false;
                }
            }
        }

        self.panes[self.active].on_key(&mut self.ctx, key);
        false
    }
}

impl EditorContext {
    // -----------------------------------------------------------------------
    // AST navigation
    // -----------------------------------------------------------------------

    fn last_strong(&self) -> Option<&AstNode> {
        self.ast_path.iter().rev().find(|node| !node.weak)
    }

    fn ast_navigate_up(&mut self) {
        let len = self.ast_path.len();
        self.ast_cursor = match (self.ast_cursor, len) {
            (_, 0) => None,
            (Some(i), _) => Some(i.saturating_sub(1)),
            (None, _) => Some(len - 1),
        };
    }

    fn ast_navigate_down(&mut self) {
        let len = self.ast_path.len();
        self.ast_cursor = match (self.ast_cursor, len) {
            (_, 0) => None,
            (Some(i), _) => Some((i + 1).min(len - 1)),
            (None, _) => Some(len - 1),
        };
    }

    // -----------------------------------------------------------------------
    // Recompile
    // -----------------------------------------------------------------------

    fn recompile(&mut self) {
        let mut errors: Vec<Diagnostic> = Vec::new();

        let mut lexer = Lexer::new(&self.source);
        let tokens = match lexer.tokenize() {
            Ok(tokens) => tokens,
            Err(error) => {
                let (span, message) = lex_error_info(&error);
                let mut out = String::new();
                let _ = error.pretty_print(&self.source, &mut out);
                errors.push(Diagnostic { span, message });
                self.output = CompilerOutput {
                    errors,
                    result: Err(out),
                };
                return;
            }
        };

        let mut parser = Parser::new(&self.source, tokens.clone());
        let ast = match parser.parse() {
            Ok(stmts) => stmts,
            Err(error) => {
                let (span, message) = parser_error_info(&error);
                let mut out = String::new();
                let _ = error.pretty_print(&self.source, &mut out);
                errors.push(Diagnostic { span, message });
                self.output = CompilerOutput {
                    errors,
                    result: Ok((tokens, Err(out))),
                };
                return;
            }
        };

        // let mut env = Env::with_symbols(&interpreter::default_symbols());
        // match resolution::resolve_exprs(&mut env, &ast, None) {
        //     Ok(()) => {}
        //     Err(error) => {
        //         let (span, message) = resolution_error_info(&error);
        //         let mut out = String::new();
        //         let _ = error.pretty_print(&self.source, &mut out);
        //         errors.push(Diagnostic { span, message });
        //         self.output = CompilerOutput {
        //             errors,
        //             result: Ok((tokens, Ok((ast, Err(out))))),
        //         };
        //         return;
        //     }
        // };

        self.output = CompilerOutput {
            errors,
            result: Ok((tokens, Ok((ast, ())))),
            // result: Ok((tokens, Ok((ast, Ok(env))))),
        };
    }

    // -----------------------------------------------------------------------
    // Diagnostics
    // -----------------------------------------------------------------------

    fn content_rows(&self) -> u16 {
        let reserved = 2u16 + self.diagnostic_line_count() as u16;
        self.term_size.1.saturating_sub(reserved)
    }

    fn diagnostic_line_count(&self) -> usize {
        if self.output.errors.is_empty() {
            0
        } else {
            (1 + self.output.errors.len()).min(6)
        }
    }

    fn render_diagnostics(
        &self,
        stdout: &mut Stdout,
        top_y: u16,
        cols: usize,
        rows: usize,
    ) -> io::Result<()> {
        if rows == 0 {
            return Ok(());
        }

        let n = self.output.errors.len();
        let header = format!(" {} error{}", n, if n == 1 { "" } else { "s" });
        queue!(
            stdout,
            cursor::MoveTo(0, top_y),
            SetBackgroundColor(Color::DarkRed),
            SetForegroundColor(Color::White),
            SetAttribute(Attribute::Bold),
            Print(pad(&header, cols)),
            SetAttribute(Attribute::Reset),
            ResetColor,
        )?;

        for (i, diag) in self.output.errors.iter().enumerate().take(rows - 1) {
            let pos = self.source.index_to_position(diag.span.start);
            let line = format!("  {}:{} — {}", pos.row + 1, pos.col + 1, diag.message);
            queue!(
                stdout,
                cursor::MoveTo(0, top_y + 1 + i as u16),
                SetForegroundColor(Color::Red),
                Print(pad(&line, cols)),
                ResetColor,
            )?;
        }

        Ok(())
    }
}

struct EditorPane(BasePane);
impl std::ops::Deref for EditorPane {
    type Target = BasePane;
    fn deref(&self) -> &BasePane {
        &self.0
    }
}
impl std::ops::DerefMut for EditorPane {
    fn deref_mut(&mut self) -> &mut BasePane {
        &mut self.0
    }
}

impl Pane for EditorPane {
    fn label(&self) -> &str {
        "Editor"
    }

    fn status(&self, ctx: &EditorContext) -> String {
        let pos = ctx.source.index_to_position(self.cursor);
        let loc = format!("{}:{}", pos.row + 1, pos.col + 1);

        let token_info = {
            let tokens = ctx.output.tokens();
            let idx = tokens.partition_point(|tok| tok.span.end <= self.cursor);
            tokens
                .get(idx)
                .filter(|tok| tok.contains(self.cursor))
                .map(|tok| format!("{tok:?}"))
                .unwrap_or_else(|| "—".into())
        };

        let ast_info = if let Some(idx) = ctx.ast_cursor {
            let node = &ctx.ast_path[idx];
            format!(
                "AST [{}/{}] {}  {}..{}",
                idx + 1,
                ctx.ast_path.len(),
                node.label,
                node.span.start,
                node.span.end,
            )
        } else if let Some(deepest) = ctx.ast_path.last() {
            format!("AST: {}", deepest.label)
        } else {
            "AST: —".into()
        };

        let ok = if ctx.output.errors.is_empty() {
            "✓"
        } else {
            ""
        };
        format!(" {loc} │ {token_info} │ {ast_info} │ {ok} ")
    }

    fn on_key(&mut self, ctx: &mut EditorContext, key: KeyEvent) {
        let rows = ctx.content_rows() as usize;

        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        if alt {
            match key.code {
                KeyCode::Up => {
                    ctx.ast_navigate_up();
                    return;
                }
                KeyCode::Down => {
                    ctx.ast_navigate_down();
                    return;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Left => {
                if alt {
                    if let Some(idx) = ctx.ast_cursor {
                        self.cursor = ctx.ast_path[idx].span.start;
                    } else {
                        self.move_node_left(ctx);
                    }
                } else if ctrl || shift {
                    self.move_token_left(ctx);
                } else {
                    self.move_cursor_by(&ctx.source, -1);
                }
            }
            KeyCode::Right => {
                if alt {
                    if let Some(idx) = ctx.ast_cursor {
                        self.cursor = ctx.ast_path[idx].span.end;
                    } else {
                        self.move_node_right(ctx);
                    }
                } else if ctrl || shift {
                    self.move_token_right(ctx);
                } else {
                    self.move_cursor_by(&ctx.source, 1);
                }
            }

            KeyCode::Tab => {
                for _ in 0..4 {
                    ctx.source.insert(self.cursor, ' ');
                    self.cursor += 1;
                }
                ctx.recompile();
            }
            KeyCode::Char(c) => {
                ctx.source.insert(self.cursor, c);
                self.cursor += 1;
                ctx.recompile();
            }
            KeyCode::Enter => {
                ctx.source.insert(self.cursor, '\n');
                self.cursor += 1;
                ctx.recompile();
            }
            KeyCode::Backspace => {
                if let Some(idx) = ctx.ast_cursor {
                    let span = ctx.ast_path[idx].span;
                    ctx.source.remove_span(span);
                    self.cursor = span.start;
                } else if self.cursor > 0 {
                    if ctrl || shift {
                        let end = self.cursor;
                        self.move_token_left(ctx);
                        ctx.source.remove_span(Span {
                            start: self.cursor,
                            end,
                        })
                    } else {
                        self.cursor -= 1;
                        ctx.source.remove(self.cursor);
                    }
                }
                ctx.recompile();
            }
            KeyCode::Delete => {
                if let Some(idx) = ctx.ast_cursor {
                    let span = ctx.ast_path[idx].span;
                    ctx.source.remove_span(span);
                    self.cursor = span.start;
                } else if self.cursor < ctx.source.len() {
                    ctx.source.remove(self.cursor);
                }
                ctx.recompile();
            }
            _ => self.0.on_key(&ctx.source, key, rows),
        }

        ctx.version += 1;
        ctx.ast_cursor = None;
        match &ctx.output.result {
            Ok((_, Ok((ast, _)))) => ctx.ast_path = ast_path_at(ast, self.cursor),
            _ => {}
        }
        self.update_scroll(&ctx.source, rows);
    }

    fn render(
        &self,
        ctx: &EditorContext,
        stdout: &mut Stdout,
        top_y: u16,
        rows: usize,
        cols: usize,
    ) -> io::Result<()> {
        let highlight_span: Option<Span> = ctx.ast_cursor.map(|idx| ctx.ast_path[idx].span);

        const GUTTER: usize = 4;
        let text_cols = cols.saturating_sub(GUTTER);

        let source_lines: Vec<(usize, usize)> = ctx
            .source
            .lines()
            .enumerate()
            .filter(|(i, _)| *i >= self.scroll_row && *i < self.scroll_row + rows)
            .map(|(_, b)| b)
            .collect();

        for (screen_row, (line_start, line_end)) in source_lines.iter().enumerate() {
            let y = top_y + screen_row as u16;
            queue!(
                stdout,
                cursor::MoveTo(0, y),
                SetForegroundColor(Color::AnsiValue(130)),
                Print(format!("{:3} ", self.scroll_row + screen_row + 1)),
                ResetColor,
            )?;

            self.render_line(
                ctx,
                stdout,
                *line_start,
                *line_end,
                text_cols,
                highlight_span,
            )?;
        }

        for blank in source_lines.len()..rows {
            queue!(
                stdout,
                cursor::MoveTo(0, top_y + blank as u16),
                Print(format!("{:width$}", "", width = cols)),
            )?;
        }
        Ok(())
    }
}

impl EditorPane {
    fn new() -> Self {
        Self(BasePane::new())
    }

    // -----------------------------------------------------------------------
    // Cursor movement
    // -----------------------------------------------------------------------

    fn move_node_left(&mut self, ctx: &EditorContext) {
        if let Some(node) = ctx.ast_path.last() {
            self.cursor = node.span.start;
        } else {
            self.move_token_left(ctx);
        }
    }

    fn move_node_right(&mut self, ctx: &EditorContext) {
        if let Some(node) = ctx.ast_path.last() {
            self.cursor = node.span.end.saturating_sub(1).max(node.span.start);
        } else {
            self.move_token_right(ctx);
        }
    }

    fn move_token_left(&mut self, ctx: &EditorContext) {
        let pos = self.cursor;
        let tokens = ctx.output.tokens();
        let idx = tokens.partition_point(|tok| tok.span.start < pos);
        if let Some(tok) = tokens.get(idx.saturating_sub(1)) {
            self.cursor = if pos <= tok.span.start {
                0
            } else {
                tok.span.start
            };
        }
    }

    fn move_token_right(&mut self, ctx: &EditorContext) {
        let pos = self.cursor;
        let tokens = ctx.output.tokens();
        let idx = tokens.partition_point(|tok| tok.span.start <= pos);
        if let Some(tok) = tokens.get(idx) {
            self.cursor = tok.span.start;
        }
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    fn render_line(
        &self,
        ctx: &EditorContext,
        stdout: &mut Stdout,
        line_start: usize,
        line_end: usize,
        max_cols: usize,
        highlight_span: Option<Span>,
    ) -> io::Result<()> {
        let tokens = &ctx.output.tokens();
        let mut col = 0usize;
        let mut offset = line_start;
        let mut tok_idx = tokens.partition_point(|tok| tok.span.end <= line_start);

        while offset < line_end && col < max_cols {
            let next_tok_start = tokens
                .get(tok_idx)
                .map(|tok| tok.span.start)
                .unwrap_or(usize::MAX);

            if offset < next_tok_start {
                let gap_end = next_tok_start.min(line_end);
                col += self.render_segment(
                    ctx,
                    stdout,
                    offset,
                    gap_end,
                    Color::DarkGrey,
                    highlight_span,
                    col,
                    max_cols,
                )?;
                offset = gap_end;
                continue;
            }

            let Some(tok) = tokens.get(tok_idx) else {
                break;
            };
            if tok.span.start >= line_end {
                break;
            }

            let tok_end = tok.span.end.min(line_end);
            col += self.render_segment(
                ctx,
                stdout,
                offset,
                tok_end,
                token_color(&tok.data),
                highlight_span,
                col,
                max_cols,
            )?;
            offset = tok_end;
            tok_idx += 1;
        }

        if self.cursor == line_end && col < max_cols && ctx.is_focus {
            queue!(
                stdout,
                SetBackgroundColor(Color::Black),
                Print(' '),
                ResetColor
            )?;
            col += 1;
        }

        if col < max_cols {
            queue!(
                stdout,
                Print(format!("{:width$}", "", width = max_cols - col))
            )?;
        }

        Ok(())
    }

    fn render_segment(
        &self,
        ctx: &EditorContext,
        stdout: &mut Stdout,
        start: usize,
        end: usize,
        base_color: Color,
        highlight_span: Option<Span>,
        col_offset: usize,
        max_cols: usize,
    ) -> io::Result<usize> {
        let cursor_inside = self.cursor >= start && self.cursor < end && ctx.is_focus;
        let hl_inside = highlight_span
            .map(|s| s.start < end && s.end > start)
            .unwrap_or(false);
        let err_inside = ctx
            .output
            .errors
            .iter()
            .any(|e| e.span.start < end && e.span.end > start);

        if !cursor_inside && !hl_inside && !err_inside {
            let len = (max_cols - col_offset).min(end - start);
            if len == 0 {
                return Ok(0);
            }
            let text = ctx.source.substring(start, start + len);
            queue!(
                stdout,
                SetForegroundColor(base_color),
                Print(&text),
                ResetColor
            )?;
            return Ok(len);
        }

        let mut count = 0;
        for offset in start..end {
            if col_offset + count >= max_cols {
                break;
            }
            let c = ctx.source.buffer[offset];
            let is_cursor = offset == self.cursor;
            let is_hl = highlight_span.map(|s| s.contains(offset)).unwrap_or(false);
            let is_err = ctx.output.errors.iter().any(|e| e.span.contains(offset));

            if is_cursor {
                queue!(
                    stdout,
                    SetBackgroundColor(base_color),
                    SetForegroundColor(Color::White),
                    Print(c),
                    ResetColor,
                )?;
            } else if is_hl {
                queue!(
                    stdout,
                    SetBackgroundColor(Color::Grey),
                    SetForegroundColor(base_color),
                    Print(c),
                    ResetColor,
                )?;
            } else if is_err {
                queue!(
                    stdout,
                    SetBackgroundColor(Color::Red),
                    SetForegroundColor(Color::White),
                    Print(c),
                    ResetColor,
                )?;
            } else {
                queue!(stdout, SetForegroundColor(base_color), Print(c), ResetColor)?;
            }
            count += 1;
        }
        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// AST traversal
// ---------------------------------------------------------------------------

fn ast_path_at(stmts: &[Spanned<Stmt>], cursor: usize) -> Vec<AstNode> {
    let mut path = Vec::new();
    path.push(AstNode {
        label: "Program",
        span: Span {
            start: stmts.first().map_or(0, |s| s.span.start),
            end: stmts.last().map_or(0, |s| s.span.end),
        },
        weak: true,
    });
    for stmt in stmts {
        if stmt.contains(cursor) {
            visit_stmt(stmt, cursor, &mut path);
            break;
        }
    }
    path
}

fn visit_stmt(stmt: &Spanned<Stmt>, cursor: usize, path: &mut Vec<AstNode>) {
    path.push(AstNode {
        label: stmt.label(),
        span: stmt.span,
        weak: false,
    });
    match &stmt.data {
        Stmt::Break => {}
        Stmt::Return { expr } => {
            if let Some(expr) = expr {
                if expr.contains(cursor) {
                    visit_expr(expr, cursor, path)
                }
            }
        }
        Stmt::Call { name, args } => {
            if name.contains(cursor) {
                path.push(AstNode {
                    label: "FunctionName",
                    span: name.span,
                    weak: true,
                });
                return;
            }
            if args.contains(cursor) {
                visit_exprs(args, cursor, path);
            }
        }
        Stmt::Binding { lhs, rhs } => {
            if lhs.contains(cursor) {
                visit_idents(lhs, cursor, path);
                return;
            }
            if rhs.contains(cursor) {
                visit_exprs(rhs, cursor, path);
            }
        }
        Stmt::Assign { lhs, rhs } => {
            if lhs.contains(cursor) {
                visit_exprs(lhs, cursor, path);
                return;
            }
            if rhs.contains(cursor) {
                visit_exprs(rhs, cursor, path);
            }
        }
        Stmt::ForNum {
            var,
            start,
            limit,
            step,
            body,
        } => todo!(),
        Stmt::If {
            condition,
            then_block,
            else_if_blocks,
            else_block,
        } => {
            if condition.contains(cursor) {
                visit_expr(condition, cursor, path);
                return;
            }
            if then_block.contains(cursor) {
                visit_stmts(then_block, cursor, path);
                return;
            }
            for (condition, then_block) in else_if_blocks {
                if condition.contains(cursor) {
                    visit_expr(condition, cursor, path);
                    return;
                }
                if then_block.contains(cursor) {
                    visit_stmts(then_block, cursor, path);
                    return;
                }
            }
            if let Some(else_block) = else_block {
                if else_block.contains(cursor) {
                    visit_stmts(else_block, cursor, path);
                }
            }
        }
        Stmt::TypeDef { name, fields } => {
            if name.contains(cursor) {
                path.push(AstNode {
                    label: "TypeName",
                    span: name.span,
                    weak: true,
                });
                return;
            }
            if fields.contains(cursor) {
                path.push(AstNode {
                    label: "TypeFields",
                    span: fields.span,
                    weak: true,
                });
                for field in &fields.data {
                    if field.contains(cursor) {
                        path.push(AstNode {
                            label: "TypeField",
                            span: field.span,
                            weak: true,
                        });
                        if field.name.contains(cursor) {
                            path.push(AstNode {
                                label: "FieldName",
                                span: field.name.span,
                                weak: true,
                            });
                        }
                        if field.typ.contains(cursor) {
                            path.push(AstNode {
                                label: "FieldType",
                                span: field.typ.span,
                                weak: true,
                            });
                            // match &field.typ.data {
                            //     Type::Named { nesting, name } => {
                            //         if *nesting > 0 && name.contains(cursor) {
                            //             path.push(AstNode {
                            //                 label: "NestedFieldTypeName",
                            //                 span: name.span,
                            //                 weak: true,
                            //             });
                            //         }
                            //     }
                            //     Type::Function { nesting, args, ret } => {
                            //     },
                            // }
                        }
                        return;
                    }
                }
            }
        }
        Stmt::FuncDef {
            is_local,
            name,
            body,
        } => {
            if name.contains(cursor) {
                path.push(AstNode {
                    label: "FunctionName",
                    span: name.span,
                    weak: true,
                });
                return;
            }
            if body.body.contains(cursor) {
                visit_stmts(&body.body, cursor, path);
            }
        }
    }
}

fn visit_expr(expr: &Spanned<Expr>, cursor: usize, path: &mut Vec<AstNode>) {
    path.push(AstNode {
        label: expr.label(),
        span: expr.span,
        weak: false,
    });
    match &expr.data {
        Expr::Nil
        | Expr::True
        | Expr::False
        | Expr::Float(_)
        | Expr::Integer(_)
        | Expr::String(_)
        | Expr::Identifier(_) => {}
        Expr::Table { elements } => {
            for e in elements {
                if e.contains(cursor) {
                    match &e.data {
                        Element::Indexed(expr) => visit_expr(expr, cursor, path),
                        Element::Named { name, expr } => {
                            path.push(AstNode {
                                label: "TableEntry",
                                span: e.span,
                                weak: true,
                            });
                            if name.contains(cursor) {
                                path.push(AstNode {
                                    label: "EntryName",
                                    span: name.span,
                                    weak: true,
                                });
                                continue;
                            }
                            if expr.contains(cursor) {
                                visit_expr(expr, cursor, path);
                            }
                        }
                    }
                    break;
                }
            }
        }
        Expr::TypeConstructor { name, fields } => {
            if name.contains(cursor) {
                path.push(AstNode {
                    label: "TypeName",
                    span: name.span,
                    weak: true,
                });
                return;
            }
            if fields.contains(cursor) {
                path.push(AstNode {
                    label: "FieldConstructorList",
                    span: fields.span,
                    weak: true,
                });
                for field in &fields.data {
                    if field.contains(cursor) {
                        match &field.data {
                            FieldConstructor::Implicit(_) => {
                                path.push(AstNode {
                                    label: "ImplicitFieldConstructor",
                                    span: field.span,
                                    weak: true,
                                });
                            }
                            FieldConstructor::Explicit { name, expr } => {
                                path.push(AstNode {
                                    label: "ExplicitFieldConstructor",
                                    span: field.span,
                                    weak: true,
                                });
                                if name.contains(cursor) {
                                    path.push(AstNode {
                                        label: "FieldConstructorName",
                                        span: name.span,
                                        weak: true,
                                    });
                                    return;
                                }
                                if expr.contains(cursor) {
                                    path.push(AstNode {
                                        label: "FieldConstructorExpression",
                                        span: expr.span,
                                        weak: true,
                                    });
                                    visit_expr(expr, cursor, path);
                                    return;
                                }
                            }
                        }
                        return;
                    }
                }
            }
        }
        Expr::Call { name, args } => {
            if name.contains(cursor) {
                path.push(AstNode {
                    label: "FunctionName",
                    span: name.span,
                    weak: true,
                });
                return;
            }
            if args.contains(cursor) {
                visit_exprs(args, cursor, path);
            }
        }
        Expr::Member { val, member } => {
            if val.contains(cursor) {
                visit_expr(val, cursor, path);
                return;
            }
            if member.contains(cursor) {
                path.push(AstNode {
                    label: "Member",
                    span: member.span,
                    weak: true,
                });
            }
        }
        Expr::UnOp { val, .. } => {
            if val.contains(cursor) {
                visit_expr(val, cursor, path)
            }
        }
        Expr::BinOp { rhs, lhs, .. } => {
            if lhs.contains(cursor) {
                visit_expr(lhs, cursor, path);
                return;
            }
            if rhs.contains(cursor) {
                visit_expr(rhs, cursor, path);
            }
        }
        Expr::Func { body } => todo!(),
    }
}

fn visit_idents(idents: &Spanned<Vec<Spanned<String>>>, cursor: usize, path: &mut Vec<AstNode>) {
    path.push(AstNode {
        label: "IdentifierList",
        span: idents.span,
        weak: true,
    });
    for ident in &idents.data {
        if ident.contains(cursor) {
            path.push(AstNode {
                label: "Identifier",
                span: ident.span,
                weak: true,
            });
            return;
        }
    }
}
fn visit_exprs(exprs: &Spanned<Vec<Spanned<Expr>>>, cursor: usize, path: &mut Vec<AstNode>) {
    path.push(AstNode {
        label: "ExpressionList",
        span: exprs.span,
        weak: true,
    });
    for expr in &exprs.data {
        if expr.contains(cursor) {
            visit_expr(expr, cursor, path);
            return;
        }
    }
}
fn visit_stmts(stmts: &Spanned<Vec<Spanned<Stmt>>>, cursor: usize, path: &mut Vec<AstNode>) {
    path.push(AstNode {
        label: "StatementList",
        span: stmts.span,
        weak: true,
    });
    for stmt in &stmts.data {
        if stmt.contains(cursor) {
            visit_stmt(stmt, cursor, path);
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Error info
// ---------------------------------------------------------------------------

fn lex_error_info(error: &lexer::Error) -> (Span, String) {
    match error {
        lexer::Error::Unexpected(span) => (*span, "Unexpected character".into()),
    }
}

fn parser_error_info(error: &parser::Error) -> (Span, String) {
    match error {
        parser::Error::Todo { message, span } => (*span, message.to_string()),
        parser::Error::Unexpected {
            message,
            expected,
            found,
            ..
        } => {
            let msg = if let Some(m) = message {
                if !expected.is_empty() {
                    format!("Expected {} ({:?}), found {:?}", m, expected[0], found.data)
                } else {
                    format!("Expected {}, found {:?}", m, found.data)
                }
            } else if !expected.is_empty() {
                format!("Expected {:?}, found {:?}", expected[0], found.data)
            } else {
                format!("Unexpected {:?}", found.data)
            };
            (found.span, msg)
        }
    }
}

// fn resolution_error_info(error: &resolution::Error) -> (Span, String) {
//     match error {
//         resolution::Error::Reached => unreachable!(),
//         resolution::Error::NotInScope { ident, span } => {
//             (*span, format!("Identifier `{ident}` not found"))
//         }
//         resolution::Error::ShadowInPattern { ident, new, .. } => (
//             *new,
//             format!("Identifier `{ident}` bound more than once in same pattern"),
//         ),
//         resolution::Error::UnexpectedInAlternative { ident, span, .. } => (
//             *span,
//             format!("Identifier `{ident}` not bound in all alternatives"),
//         ),
//     }
// }

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn pad(s: &str, width: usize) -> String {
    let count = s.chars().count();
    if count >= width {
        s.chars().take(width.saturating_sub(1)).collect::<String>() + "…"
    } else {
        format!("{s:<width$}")
    }
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let mut stdout = io::stdout();
        let _ = Editor::cleanup(&mut stdout);

        eprintln!("\n========== PANIC ==========");
        if let Some(location) = panic_info.location() {
            eprintln!(
                "Panic occurred in file '{}' at line {}",
                location.file(),
                location.line()
            );
        }
        if let Some(message) = panic_info.payload().downcast_ref::<&str>() {
            eprintln!("Message: {}", message);
        } else if let Some(message) = panic_info.payload().downcast_ref::<String>() {
            eprintln!("Message: {}", message);
        }

        eprintln!("\n========== LOG DUMP ==========");
        match LOGGER.lock() {
            Ok(logger) => {
                eprintln!("{}", logger.buffer.iter().collect::<String>());
            }
            Err(_) => {
                eprintln!("Logger mutex was poisoned!");
            }
        }

        eprintln!("=============================\n");
    }));
}
