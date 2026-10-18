//! Turns PCF source text into tokens.

use std::ops::Range;

use super::{
    ParseError,
    syntax::{is_identifier_part, is_identifier_start},
};

/// A byte order mark, which editors on Windows like to put at the start of UTF-8 files.
const BOM: char = '\u{FEFF}';

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Token {
    Identifier {
        name: String,
        quoted: bool,
    },
    /// The magnitude of an integer literal; the sign is applied by the parser so that
    /// `-9223372036854775808` can be represented.
    Int(u64),
    Float(f64),
    String(String),
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    LeftParen,
    RightParen,
    LeftAngle,
    RightAngle,
    Equals,
    Comma,
    Semicolon,
    Dot,
    Minus,
    Eof,
}

impl Token {
    /// How the token is referred to in error messages.
    pub(crate) fn describe(&self) -> String {
        match self {
            Token::Identifier { name, .. } => format!("identifier `{name}`"),
            Token::Int(_) | Token::Float(_) => "a number".into(),
            Token::String(_) => "a string".into(),
            Token::LeftBrace => "`{`".into(),
            Token::RightBrace => "`}`".into(),
            Token::LeftBracket => "`[`".into(),
            Token::RightBracket => "`]`".into(),
            Token::LeftParen => "`(`".into(),
            Token::RightParen => "`)`".into(),
            Token::LeftAngle => "`<`".into(),
            Token::RightAngle => "`>`".into(),
            Token::Equals => "`=`".into(),
            Token::Comma => "`,`".into(),
            Token::Semicolon => "`;`".into(),
            Token::Dot => "`.`".into(),
            Token::Minus => "`-`".into(),
            Token::Eof => "end of input".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Spanned {
    pub(crate) token: Token,
    pub(crate) span: Range<usize>,
}

/// Splits `source` into tokens. The result always ends with [`Token::Eof`].
pub(crate) fn tokenize(source: &str) -> Result<Vec<Spanned>, ParseError> {
    let mut lexer = Lexer {
        src: source,
        pos: if source.starts_with(BOM) {
            BOM.len_utf8()
        } else {
            0
        },
    };
    let mut tokens = Vec::new();
    loop {
        let spanned = lexer.next_token()?;
        let done = spanned.token == Token::Eof;
        tokens.push(spanned);
        if done {
            return Ok(tokens);
        }
    }
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
}

impl Lexer<'_> {
    fn rest(&self) -> &str {
        &self.src[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_nth(&self, n: usize) -> Option<char> {
        self.rest().chars().nth(n)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn next_token(&mut self) -> Result<Spanned, ParseError> {
        self.skip_trivia()?;
        let start = self.pos;
        let Some(c) = self.peek() else {
            return Ok(Spanned {
                token: Token::Eof,
                span: start..start,
            });
        };
        let token = match c {
            '0'..='9' => self.number()?,
            '.' if self.peek_nth(1).is_some_and(|c| c.is_ascii_digit()) => self.number()?,
            '"' => self.string(start, 0)?,
            '#' => {
                let pounds = self.rest().bytes().take_while(|&b| b == b'#').count();
                if self.rest()[pounds..].starts_with('"') {
                    self.pos += pounds;
                    self.string(start, pounds)?
                } else {
                    return Err(ParseError::new(
                        "unexpected character `#`",
                        start..start + 1,
                    ));
                }
            }
            '`' => self.quoted_identifier(start)?,
            c if is_identifier_start(c) => {
                let len = self
                    .rest()
                    .find(|c| !is_identifier_part(c))
                    .unwrap_or(self.rest().len());
                let name = self.rest()[..len].to_owned();
                self.pos += len;
                Token::Identifier {
                    name,
                    quoted: false,
                }
            }
            _ => {
                self.bump();
                match c {
                    '{' => Token::LeftBrace,
                    '}' => Token::RightBrace,
                    '[' => Token::LeftBracket,
                    ']' => Token::RightBracket,
                    '(' => Token::LeftParen,
                    ')' => Token::RightParen,
                    '<' => Token::LeftAngle,
                    '>' => Token::RightAngle,
                    '=' => Token::Equals,
                    ',' => Token::Comma,
                    ';' => Token::Semicolon,
                    '.' => Token::Dot,
                    '-' => Token::Minus,
                    ':' => {
                        return Err(ParseError::new(
                            "unexpected character `:`, type annotations are not supported in PCF",
                            start..self.pos,
                        ));
                    }
                    _ => {
                        return Err(ParseError::new(
                            format!("unexpected character `{c}`"),
                            start..self.pos,
                        ));
                    }
                }
            }
        };
        Ok(Spanned {
            token,
            span: start..self.pos,
        })
    }

    /// Skips whitespace, comments and a leading shebang line.
    fn skip_trivia(&mut self) -> Result<(), ParseError> {
        if self.src[..self.pos].trim_start_matches(BOM).is_empty() && self.rest().starts_with("#!")
        {
            self.skip_line();
        }
        loop {
            let rest = self.rest();
            if rest.starts_with(char::is_whitespace) {
                self.bump();
            } else if rest.starts_with("//") {
                self.skip_line();
            } else if let Some(comment) = rest.strip_prefix("/*") {
                let Some(end) = comment.find("*/") else {
                    return Err(ParseError::new(
                        "unterminated block comment",
                        self.pos..self.pos + 2,
                    ));
                };
                self.pos += end + 4;
            } else {
                return Ok(());
            }
        }
    }

    fn skip_line(&mut self) {
        self.pos += self.rest().find('\n').unwrap_or(self.rest().len());
    }

    fn quoted_identifier(&mut self, start: usize) -> Result<Token, ParseError> {
        self.bump();
        let len = self
            .rest()
            .find(['`', '\n', '\r'])
            .filter(|&i| self.rest()[i..].starts_with('`'))
            .ok_or_else(|| ParseError::new("unterminated quoted identifier", start..start + 1))?;
        let name = self.rest()[..len].to_owned();
        self.pos += len + 1;
        Ok(Token::Identifier { name, quoted: true })
    }

    fn number(&mut self) -> Result<Token, ParseError> {
        let start = self.pos;
        let radix = match self.rest().as_bytes() {
            [b'0', b'x', ..] => Some(16),
            [b'0', b'b', ..] => Some(2),
            [b'0', b'o', ..] => Some(8),
            _ => None,
        };
        if let Some(radix) = radix {
            self.pos += 2;
            let digits = self.digits(|c| c.is_digit(radix));
            if digits.is_empty() {
                return Err(ParseError::new(
                    "expected digits after the number prefix",
                    start..self.pos,
                ));
            }
            return u64::from_str_radix(&digits, radix)
                .map(Token::Int)
                .map_err(|_| ParseError::new("integer literal is too large", start..self.pos));
        }

        let mut text = self.digits(|c| c.is_ascii_digit());
        let mut is_float = false;
        if self.peek() == Some('.') && self.peek_nth(1).is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
            text.push('.');
            text.push_str(&self.digits(|c| c.is_ascii_digit()));
            is_float = true;
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            let sign = matches!(self.peek_nth(1), Some('+' | '-'));
            if self
                .peek_nth(1 + usize::from(sign))
                .is_some_and(|c| c.is_ascii_digit())
            {
                self.bump();
                text.push('e');
                if sign {
                    text.extend(self.bump());
                }
                text.push_str(&self.digits(|c| c.is_ascii_digit()));
                is_float = true;
            }
        }

        if is_float {
            text.parse()
                .map(Token::Float)
                .map_err(|_| ParseError::new("invalid float literal", start..self.pos))
        } else {
            text.parse()
                .map(Token::Int)
                .map_err(|_| ParseError::new("integer literal is too large", start..self.pos))
        }
    }

    /// Consumes digits accepted by `is_digit`, allowing `_` separators after the first digit.
    fn digits(&mut self, is_digit: impl Fn(char) -> bool) -> String {
        let mut digits = String::new();
        while let Some(c) = self.peek() {
            if is_digit(c) {
                digits.push(c);
            } else if c != '_' || digits.is_empty() {
                break;
            }
            self.bump();
        }
        digits
    }

    /// Lexes a string literal whose opening quote is at the current position, `pounds` being the
    /// number of `#` characters (already consumed) of a custom delimiter such as `#"..."#`.
    fn string(&mut self, start: usize, pounds: usize) -> Result<Token, ParseError> {
        if self.rest().starts_with(r#"""""#) {
            self.pos += 3;
            self.multi_line_string(start, pounds)
        } else {
            self.pos += 1;
            let body = self.string_body(start, 1, pounds)?;
            let mut value = String::new();
            unescape(&self.src[body.clone()], body.start, pounds, &mut value)?;
            Ok(Token::String(value))
        }
    }

    fn multi_line_string(&mut self, start: usize, pounds: usize) -> Result<Token, ParseError> {
        let line_end = self.rest().find('\n').unwrap_or(self.rest().len());
        if !self.rest()[..line_end].trim().is_empty() {
            return Err(ParseError::new(
                "the content of a multi-line string must begin on a new line",
                start..self.pos,
            ));
        }
        self.pos = (self.pos + line_end + 1).min(self.src.len());
        let body = self.string_body(start, 3, pounds)?;
        let raw = &self.src[body.clone()];

        // The whitespace in front of the closing delimiter is the indentation to strip.
        let indent_start = raw.rfind('\n').map_or(0, |i| i + 1);
        let indent = &raw[indent_start..];
        if !indent.chars().all(|c| c == ' ' || c == '\t') {
            return Err(ParseError::new(
                "the closing delimiter of a multi-line string must begin on a new line",
                body.end..self.pos,
            ));
        }

        let mut value = String::new();
        let mut line_start = body.start;
        let content = if indent_start == 0 {
            ""
        } else {
            &raw[..indent_start - 1]
        };
        let mut lines = content.split('\n').peekable();
        while let Some(line) = lines.next() {
            let line_offset = line_start;
            line_start += line.len() + 1;
            let line = line.strip_suffix('\r').unwrap_or(line);
            let continues = if let Some(stripped) = line.strip_prefix(indent) {
                unescape(stripped, line_offset + indent.len(), pounds, &mut value)?
            } else if line.chars().all(|c| c == ' ' || c == '\t') && indent.starts_with(line) {
                false
            } else {
                return Err(ParseError::new(
                    "line must match or exceed the indentation of the closing delimiter",
                    line_offset..line_offset + line.len(),
                ));
            };
            if !continues && lines.peek().is_some() {
                value.push('\n');
            }
        }
        Ok(Token::String(value))
    }

    /// Advances past the closing delimiter (`quotes` double quotes followed by `pounds` `#`
    /// characters) and returns the byte range of the raw content before it.
    fn string_body(
        &mut self,
        start: usize,
        quotes: usize,
        pounds: usize,
    ) -> Result<Range<usize>, ParseError> {
        let unterminated = |end: usize| ParseError::new("unterminated string literal", start..end);
        let content_start = self.pos;
        loop {
            let rest = self.rest();
            match rest.chars().next() {
                None => return Err(unterminated(self.pos)),
                Some('\n' | '\r') if quotes == 1 => return Err(unterminated(self.pos)),
                Some('\\') if has_pounds(&rest[1..], pounds) => {
                    self.pos += 1 + pounds;
                    match self.bump() {
                        None => return Err(unterminated(self.pos)),
                        Some('\n' | '\r') if quotes == 1 => return Err(unterminated(self.pos)),
                        Some(_) => {}
                    }
                }
                Some('"')
                    if rest.len() >= quotes + pounds
                        && rest.as_bytes()[..quotes].iter().all(|&b| b == b'"')
                        && has_pounds(&rest[quotes..], pounds) =>
                {
                    let content = content_start..self.pos;
                    self.pos += quotes + pounds;
                    return Ok(content);
                }
                Some(_) => {
                    self.bump();
                }
            }
        }
    }
}

fn has_pounds(text: &str, pounds: usize) -> bool {
    text.len() >= pounds && text.as_bytes()[..pounds].iter().all(|&b| b == b'#')
}

/// Appends `raw` to `out`, resolving escape sequences. `offset` is the position of `raw` in the
/// source, used for error spans. Returns `true` if `raw` ends with a line continuation (`\`).
fn unescape(raw: &str, offset: usize, pounds: usize, out: &mut String) -> Result<bool, ParseError> {
    let mut chars = raw.char_indices();
    while let Some((i, c)) = chars.next() {
        if c != '\\' || !has_pounds(&raw[i + 1..], pounds) {
            out.push(c);
            continue;
        }
        for _ in 0..pounds {
            chars.next();
        }
        let start = offset + i;
        let Some((j, escaped)) = chars.next() else {
            return Ok(true);
        };
        let end = offset + j + escaped.len_utf8();
        match escaped {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            'u' => {
                let rest = &raw[j + 1..];
                let invalid = || ParseError::new("invalid unicode escape sequence", start..end);
                let hex = rest
                    .strip_prefix('{')
                    .and_then(|rest| rest.split_once('}'))
                    .map(|(hex, _)| hex)
                    .ok_or_else(invalid)?;
                let code = u32::from_str_radix(hex, 16).map_err(|_| invalid())?;
                let decoded = char::from_u32(code).ok_or_else(|| {
                    ParseError::new(
                        "unicode escape sequence is not a valid code point",
                        start..end + hex.len() + 2,
                    )
                })?;
                out.push(decoded);
                for _ in 0..hex.chars().count() + 2 {
                    chars.next();
                }
            }
            '(' => {
                return Err(ParseError::new(
                    "string interpolation is not supported in PCF",
                    start..end,
                ));
            }
            other => {
                return Err(ParseError::new(
                    format!("invalid escape sequence `\\{}{other}`", "#".repeat(pounds)),
                    start..end,
                ));
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(source: &str) -> Vec<Token> {
        tokenize(source)
            .unwrap()
            .into_iter()
            .map(|spanned| spanned.token)
            .collect()
    }

    fn single(source: &str) -> Token {
        match tokens(source).as_slice() {
            [token, Token::Eof] => token.clone(),
            other => panic!("expected a single token, got {other:?}"),
        }
    }

    fn string(source: &str) -> String {
        match single(source) {
            Token::String(value) => value,
            other => panic!("expected a string, got {other:?}"),
        }
    }

    fn error(source: &str) -> String {
        tokenize(source).unwrap_err().message().to_owned()
    }

    #[test]
    fn punctuation_and_identifiers() {
        assert_eq!(
            tokens("foo { [`my key`] = bar; } (,) <.>-"),
            vec![
                Token::Identifier {
                    name: "foo".into(),
                    quoted: false
                },
                Token::LeftBrace,
                Token::LeftBracket,
                Token::Identifier {
                    name: "my key".into(),
                    quoted: true
                },
                Token::RightBracket,
                Token::Equals,
                Token::Identifier {
                    name: "bar".into(),
                    quoted: false
                },
                Token::Semicolon,
                Token::RightBrace,
                Token::LeftParen,
                Token::Comma,
                Token::RightParen,
                Token::LeftAngle,
                Token::Dot,
                Token::RightAngle,
                Token::Minus,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn unicode_and_dollar_identifiers() {
        assert_eq!(
            single("$π_1"),
            Token::Identifier {
                name: "$π_1".into(),
                quoted: false
            }
        );
        assert_eq!(
            single("``"),
            Token::Identifier {
                name: String::new(),
                quoted: true
            }
        );
    }

    #[test]
    fn byte_order_mark() {
        assert_eq!(
            tokens("\u{FEFF}#!/usr/bin/env pkl\na"),
            vec![
                Token::Identifier {
                    name: "a".into(),
                    quoted: false
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn comments_and_shebang() {
        assert_eq!(
            tokens("#!/usr/bin/env pkl\n// line\n/// doc\n/* block\n */ a"),
            vec![
                Token::Identifier {
                    name: "a".into(),
                    quoted: false
                },
                Token::Eof
            ]
        );
        assert_eq!(error("/* never closed"), "unterminated block comment");
    }

    #[test]
    fn integers() {
        assert_eq!(single("42"), Token::Int(42));
        assert_eq!(single("1_000_000"), Token::Int(1_000_000));
        assert_eq!(single("0xFF"), Token::Int(255));
        assert_eq!(single("0b1010"), Token::Int(10));
        assert_eq!(single("0o17"), Token::Int(15));
        assert_eq!(single("9223372036854775808"), Token::Int(1 << 63));
        assert_eq!(
            error("99999999999999999999"),
            "integer literal is too large"
        );
        assert_eq!(error("0x"), "expected digits after the number prefix");
    }

    #[test]
    fn floats() {
        assert_eq!(single("13.37"), Token::Float(13.37));
        assert_eq!(single(".5"), Token::Float(0.5));
        assert_eq!(single("1e3"), Token::Float(1000.0));
        assert_eq!(single("1.0E20"), Token::Float(1.0e20));
        assert_eq!(single("1.5E-7"), Token::Float(1.5e-7));
        assert_eq!(single("1_0.5e1_0"), Token::Float(1.05e11));
    }

    #[test]
    fn number_followed_by_unit() {
        assert_eq!(
            tokens("5.min 1.5.h"),
            vec![
                Token::Int(5),
                Token::Dot,
                Token::Identifier {
                    name: "min".into(),
                    quoted: false
                },
                Token::Float(1.5),
                Token::Dot,
                Token::Identifier {
                    name: "h".into(),
                    quoted: false
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn single_line_strings() {
        assert_eq!(string(r#""""#), "");
        assert_eq!(
            string(r#""tab\t \"q\" back\\slash cr\r nl\n""#),
            "tab\t \"q\" back\\slash cr\r nl\n"
        );
        assert_eq!(string(r#""\u{1F600}\u{41}""#), "😀A");
        assert_eq!(string(r#""raw unicode: π""#), "raw unicode: π");
    }

    #[test]
    fn custom_delimiter_strings() {
        assert_eq!(string(r##"#"a\b "q" \#n"#"##), "a\\b \"q\" \n");
        assert_eq!(string(r###"##"a"#b"##"###), "a\"#b");
        assert_eq!(string(r##"#"a""#"##), "a\"");
    }

    #[test]
    fn string_errors() {
        assert_eq!(error(r#""abc"#), "unterminated string literal");
        assert_eq!(error("\"abc\ndef\""), "unterminated string literal");
        assert_eq!(error(r#""\q""#), "invalid escape sequence `\\q`");
        assert_eq!(
            error(r#""\(name)""#),
            "string interpolation is not supported in PCF"
        );
        assert_eq!(
            error(r#""\u{110000}""#),
            "unicode escape sequence is not a valid code point"
        );
        assert_eq!(error(r#""\u{zz}""#), "invalid unicode escape sequence");
    }

    #[test]
    fn multi_line_strings() {
        assert_eq!(
            string("\"\"\"\n  line one\n  line two\n  \"\"\""),
            "line one\nline two"
        );
        assert_eq!(string("\"\"\"\n  a\n\n  b\n  \"\"\""), "a\n\nb");
        assert_eq!(string("\"\"\"\n  a\n  \n  b\n  \n  \"\"\""), "a\n\nb\n");
        assert_eq!(string("\"\"\"\n  \n  \n  \"\"\""), "\n");
        assert_eq!(string("\"\"\"\n  \"\"\""), "");
        assert_eq!(string("\"\"\"\n    nested\n  \"\"\""), "  nested");
        assert_eq!(string("\"\"\"\r\n  crlf\r\n  \"\"\""), "crlf");
    }

    #[test]
    fn multi_line_string_escapes() {
        assert_eq!(
            string("\"\"\"\n  quotes \"\" and \"\"\\\" end\\t\n  \"\"\""),
            "quotes \"\" and \"\"\" end\t"
        );
        assert_eq!(
            string("\"\"\"\n  joined \\\n  line\n  \"\"\""),
            "joined line"
        );
        assert_eq!(
            string("#\"\"\"\n  raw \\n \"\"\" \\#t\n  \"\"\"#"),
            "raw \\n \"\"\" \t"
        );
    }

    #[test]
    fn multi_line_string_errors() {
        assert_eq!(
            error("\"\"\"text\n\"\"\""),
            "the content of a multi-line string must begin on a new line"
        );
        assert_eq!(
            error("\"\"\"\n  a \"\"\""),
            "the closing delimiter of a multi-line string must begin on a new line"
        );
        assert_eq!(
            error("\"\"\"\n  a\n b\n  \"\"\""),
            "line must match or exceed the indentation of the closing delimiter"
        );
    }

    #[test]
    fn error_spans() {
        let err = tokenize("a = @").unwrap_err();
        assert_eq!(err.message(), "unexpected character `@`");
        assert_eq!(err.span(), 4..5);
        assert_eq!(
            error("port: Int = 1"),
            "unexpected character `:`, type annotations are not supported in PCF"
        );
    }
}
