use std::{fmt, ops::Range};

use nu_protocol::Span;

/// An error found while reading PCF text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    message: String,
    span: Range<usize>,
}

impl ParseError {
    pub(crate) fn new(message: impl Into<String>, span: Range<usize>) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// Byte range of the offending text in the parsed source.
    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    /// One-based line and column (in characters) where the error starts.
    pub fn position(&self, source: &str) -> (usize, usize) {
        let offset = floor_char_boundary(source, self.span.start);
        let before = &source[..offset];
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = before[line_start..].chars().count() + 1;
        (line, column)
    }

    /// Renders the offending source line with a caret marker underneath, for use in help text.
    pub fn snippet(&self, source: &str) -> String {
        let (line_number, column) = self.position(source);
        let line = source.lines().nth(line_number - 1).unwrap_or_default();
        let start = floor_char_boundary(source, self.span.start);
        let end = floor_char_boundary(source, self.span.end).max(start);
        let width = source[start..end]
            .lines()
            .next()
            .map_or(0, |text| text.chars().count())
            .max(1);
        let gutter = line_number.to_string();
        let pad = " ".repeat(gutter.len());
        format!(
            "{gutter} | {line}\n{pad} | {}{}",
            " ".repeat(column - 1),
            "^".repeat(width)
        )
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ParseError {}

/// An error found while converting a Nushell value to PCF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteError {
    message: String,
    help: Option<String>,
    span: Span,
}

impl WriteError {
    pub(crate) fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            help: None,
            span,
        }
    }

    pub(crate) fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn help(&self) -> Option<&str> {
        self.help.as_deref()
    }

    /// Span of the value that could not be converted.
    pub fn span(&self) -> Span {
        self.span
    }
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WriteError {}

fn floor_char_boundary(source: &str, offset: usize) -> usize {
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}
