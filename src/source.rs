use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Span[{}, {}]", self.start, self.end)
    }
}

impl Span {
    pub fn merge(self, other: Self) -> Self {
        Self {
            start: self.start,
            end: other.end,
        }
    }

    pub fn attach<T>(self, data: T) -> Spanned<T> {
        Spanned { data, span: self }
    }

    pub fn contains(self, pos: usize) -> bool {
        self.start <= pos && pos < self.end
    }

    pub fn is_empty(self) -> bool {
        self.start >= self.end
    }
}

pub struct Spanned<T> {
    pub data: T,
    pub span: Span,
}

impl<T: Clone> Clone for Spanned<T> {
    fn clone(&self) -> Spanned<T> {
        Self {
            data: self.data.clone(),
            span: self.span,
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Spanned<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.data.fmt(f)
    }
}
impl<T: fmt::Display> fmt::Display for Spanned<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.data.fmt(f)
    }
}

impl<T> Spanned<T> {
    pub fn contains(&self, pos: usize) -> bool {
        self.span.contains(pos)
    }
}

impl<T> std::ops::Deref for Spanned<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.data
    }
}

#[derive(Debug, Clone)]
pub struct Position {
    pub row: usize,
    pub col: usize,
    pub line_start: usize,
    pub line_end: usize,
}

pub struct Source {
    pub name: String,
    pub buffer: Vec<char>,
    pub nl_map: Vec<usize>,
}

impl std::ops::Deref for Source {
    type Target = [char];
    fn deref(&self) -> &[char] {
        &self.buffer
    }
}

impl Source {
    pub fn new(name: String, text: &str) -> Self {
        let buffer = text.chars().collect::<Vec<_>>();
        let mut source = Self {
            name,
            buffer,
            nl_map: Vec::new(),
        };
        source.rebuild_nl_map();
        source
    }

    pub const fn empty(name: String) -> Self {
        Self {
            name,
            buffer: Vec::new(),
            nl_map: Vec::new(),
        }
    }

    fn rebuild_nl_map(&mut self) {
        self.nl_map = self
            .buffer
            .iter()
            .enumerate()
            .filter_map(|(i, &c)| (c == '\n').then_some(i))
            .collect()
    }

    // -----------------------------------------------------------------------
    // Mutation
    // -----------------------------------------------------------------------

    pub fn insert(&mut self, pos: usize, c: char) {
        self.buffer.insert(pos, c);
        self.rebuild_nl_map();
    }

    pub fn append(&mut self, chunk: &str) {
        let nl_map = chunk
            .chars()
            .enumerate()
            .filter_map(|(i, c)| (c == '\n').then_some(i + self.buffer.len()))
            .collect::<Vec<_>>();
        self.nl_map.extend(nl_map);
        self.buffer.extend(chunk.chars().collect::<Vec<_>>());
    }

    pub fn remove(&mut self, pos: usize) {
        self.buffer.remove(pos);
        self.rebuild_nl_map();
    }

    pub fn remove_span(&mut self, span: Span) {
        self.buffer.drain(span.start..span.end);
        self.rebuild_nl_map();
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    pub fn index_to_position(&self, index: usize) -> Position {
        let row = self.nl_map.partition_point(|&nl| nl < index);
        let line_start = if row == 0 {
            0
        } else {
            self.nl_map[row - 1] + 1
        };
        let line_end = self.nl_map.get(row).copied().unwrap_or(self.buffer.len());
        Position {
            row,
            col: index - line_start,
            line_start,
            line_end,
        }
    }

    pub fn position_to_index(&self, row: usize, col: usize) -> usize {
        let line_start = if row == 0 {
            0
        } else {
            self.nl_map
                .get(row - 1)
                .map(|&nl| nl + 1)
                .unwrap_or(self.buffer.len())
        };
        let line_end = self.nl_map.get(row).copied().unwrap_or(self.buffer.len());
        (line_start + col).min(line_end)
    }

    pub fn line_count(&self) -> usize {
        self.nl_map.len() + 1
    }

    pub fn lines(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        let total = self.buffer.len();
        let newlines = self.nl_map.len();
        (0..=newlines).map(move |i| {
            let start = if i == 0 { 0 } else { self.nl_map[i - 1] + 1 };
            let end = if i < newlines { self.nl_map[i] } else { total };
            (start, end)
        })
    }

    pub fn substring(&self, start: usize, end: usize) -> String {
        self.buffer[start..end.min(self.buffer.len())]
            .iter()
            .copied()
            .collect()
    }

    pub fn content(&self) -> String {
        self.buffer.iter().copied().collect()
    }

    // -----------------------------------------------------------------------
    // Diagnostic
    // -----------------------------------------------------------------------

    pub fn print_span<W: fmt::Write>(&self, span: Span, out: &mut W) -> fmt::Result {
        let pos = self.index_to_position(span.start);
        out.write_fmt(format_args!(
            "   --> {}:{}:{}\n",
            self.name,
            pos.row + 1,
            pos.col + 1
        ))?;
        out.write_str("    |\n")?;
        out.write_fmt(format_args!(
            "{: <3} | {}\n",
            pos.row + 1,
            self.substring(pos.line_start, pos.line_end)
        ))?;
        let len = (span.end - span.start).max(1);
        out.write_fmt(format_args!(
            "    | {: >col$}{:^>len$}\n",
            "",
            "",
            col = pos.col,
            len = len
        ))
    }
}
