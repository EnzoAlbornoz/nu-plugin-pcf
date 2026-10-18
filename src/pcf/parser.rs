//! Turns PCF tokens into Nushell values.
//!
//! PCF is the static subset of Pkl: a module made of properties whose values are literals,
//! object bodies and a handful of constructors such as `List(...)`. The value mapping is:
//!
//! | PCF                                    | Nushell                       |
//! |----------------------------------------|-------------------------------|
//! | object with properties and/or entries  | record                        |
//! | object with elements, `new Listing {}` | list                          |
//! | `List(...)`, `Set(...)`                | list                          |
//! | `Map(...)`                             | record                        |
//! | `Pair(a, b)`                           | record `{first: a, second: b}` |
//! | `IntSeq(a, b)` / `.step(n)`            | range                         |
//! | `Regex("...")`                         | string                        |
//! | `Bytes(...)`                           | binary                        |
//! | duration (`5.min`)                     | duration                      |
//! | data size (`5.mb`)                     | filesize                      |
//!
//! Entry keys that are not strings (`[1] = ...`) are converted to their PCF representation.

use std::ops::{Bound, Range};

use nu_protocol::{Record, Span, Value};

use super::{
    ParseError,
    lexer::{Spanned, Token, tokenize},
    syntax::{data_size_unit, duration_unit, is_keyword},
    writer,
};

/// Parses a PCF document into a record. Every produced value carries `span`.
pub fn parse(source: &str, span: Span) -> Result<Value, ParseError> {
    let mut parser = Parser {
        tokens: tokenize(source)?,
        pos: 0,
        depth: 0,
        span,
    };
    parser.document()
}

/// What kind of object a body belongs to, which restricts the members it may contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObjectKind {
    Module,
    Dynamic,
    Listing,
    Mapping,
}

enum MemberKind {
    Property(String),
    Entry(Value),
    Element,
}

struct Member {
    kind: MemberKind,
    value: Value,
    span: Range<usize>,
}

/// How deeply objects and constructors may be nested, which keeps the recursive descent from
/// overflowing the stack on hostile input.
const MAX_DEPTH: usize = 128;

struct Parser {
    tokens: Vec<Spanned>,
    pos: usize,
    depth: usize,
    span: Span,
}

impl Parser {
    fn peek(&self) -> &Token {
        self.peek_nth(0)
    }

    fn peek_nth(&self, n: usize) -> &Token {
        let index = (self.pos + n).min(self.tokens.len() - 1);
        &self.tokens[index].token
    }

    fn current_span(&self) -> Range<usize> {
        self.tokens[self.pos].span.clone()
    }

    fn previous_end(&self) -> usize {
        self.pos
            .checked_sub(1)
            .map_or(0, |index| self.tokens[index].span.end)
    }

    fn advance(&mut self) -> Spanned {
        let spanned = self.tokens[self.pos].clone();
        if spanned.token != Token::Eof {
            self.pos += 1;
        }
        spanned
    }

    fn expect(&mut self, expected: &Token, what: &str) -> Result<Spanned, ParseError> {
        if self.peek() == expected {
            Ok(self.advance())
        } else {
            Err(self.unexpected(what))
        }
    }

    fn unexpected(&self, expected: &str) -> ParseError {
        ParseError::new(
            format!("expected {expected}, found {}", self.peek().describe()),
            self.current_span(),
        )
    }

    fn skip_semicolons(&mut self) {
        while self.peek() == &Token::Semicolon {
            self.advance();
        }
    }

    fn document(&mut self) -> Result<Value, ParseError> {
        let mut object = ObjectBuilder::new(ObjectKind::Module);
        loop {
            self.skip_semicolons();
            if self.peek() == &Token::Eof {
                return object.finish(self.span);
            }
            let member = self.member()?;
            object.push(member)?;
        }
    }

    /// Runs `parse` one nesting level deeper, failing once [`MAX_DEPTH`] is reached.
    fn nested<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        if self.depth == MAX_DEPTH {
            return Err(ParseError::new(
                format!("values are nested more than {MAX_DEPTH} levels deep"),
                self.current_span(),
            ));
        }
        self.depth += 1;
        let result = parse(self);
        self.depth -= 1;
        result
    }

    fn object_body(&mut self, kind: ObjectKind) -> Result<Value, ParseError> {
        self.nested(|parser| parser.object_members(kind))
    }

    fn object_members(&mut self, kind: ObjectKind) -> Result<Value, ParseError> {
        let open = self.expect(&Token::LeftBrace, "`{`")?;
        let mut object = ObjectBuilder::new(kind);
        loop {
            self.skip_semicolons();
            match self.peek() {
                Token::RightBrace => {
                    self.advance();
                    return object.finish(self.span);
                }
                Token::Eof => {
                    return Err(ParseError::new(
                        "unclosed object body, expected `}`",
                        open.span,
                    ));
                }
                _ => {
                    let member = self.member()?;
                    object.push(member)?;
                }
            }
        }
    }

    fn member(&mut self) -> Result<Member, ParseError> {
        let start = self.current_span().start;
        let kind = match self.peek().clone() {
            Token::Identifier { name, quoted }
                if quoted
                    || (!is_keyword(&name)
                        && (matches!(self.peek_nth(1), Token::Equals | Token::LeftBrace)
                            || !self.starts_value(&name))) =>
            {
                self.advance();
                MemberKind::Property(name)
            }
            Token::Identifier {
                name,
                quoted: false,
            } if name != "new" && matches!(self.peek_nth(1), Token::Equals | Token::LeftBrace) => {
                return Err(ParseError::new(
                    format!(
                        "keyword `{name}` cannot be a property name unless it is quoted with \
                         backticks"
                    ),
                    self.current_span(),
                ));
            }
            Token::Identifier {
                name,
                quoted: false,
            } if is_declaration_keyword(&name) => {
                return Err(ParseError::new(
                    format!(
                        "`{name}` is not supported in PCF, which only allows static values; \
                         evaluate the module with `pkl eval -f pcf` first"
                    ),
                    self.current_span(),
                ));
            }
            Token::LeftBracket => {
                self.advance();
                let key = self.expression()?;
                self.expect(&Token::RightBracket, "`]`")?;
                MemberKind::Entry(key)
            }
            _ => {
                let value = self.expression()?;
                return Ok(Member {
                    kind: MemberKind::Element,
                    value,
                    span: start..self.previous_end(),
                });
            }
        };
        let value = match self.peek() {
            Token::Equals => {
                self.advance();
                self.expression()?
            }
            Token::LeftBrace => self.object_body(ObjectKind::Dynamic)?,
            _ => return Err(self.unexpected("`=` or `{`")),
        };
        Ok(Member {
            kind,
            value,
            span: start..self.previous_end(),
        })
    }

    /// Whether the identifier `name` at the current position begins a value rather than a
    /// property name.
    fn starts_value(&self, name: &str) -> bool {
        matches!(name, "NaN" | "Infinity")
            || (is_constructor(name) && self.peek_nth(1) == &Token::LeftParen)
    }

    fn expression(&mut self) -> Result<Value, ParseError> {
        let start = self.current_span().start;
        match self.peek().clone() {
            Token::Minus => {
                self.advance();
                if matches!(self.peek(), Token::Int(_) | Token::Float(_)) {
                    return self.number(true);
                }
                let value = self.nested(Self::expression)?;
                negate(value).ok_or_else(|| {
                    ParseError::new(
                        "`-` can only be applied to numbers, durations and data sizes",
                        start..self.previous_end(),
                    )
                })
            }
            Token::Int(_) | Token::Float(_) => self.number(false),
            Token::String(value) => {
                self.advance();
                Ok(Value::string(value, self.span))
            }
            Token::Identifier {
                name,
                quoted: false,
            } => match name.as_str() {
                "true" | "false" => {
                    self.advance();
                    Ok(Value::bool(name == "true", self.span))
                }
                "null" => {
                    self.advance();
                    Ok(Value::nothing(self.span))
                }
                "NaN" => {
                    self.advance();
                    Ok(Value::float(f64::NAN, self.span))
                }
                "Infinity" => {
                    self.advance();
                    Ok(Value::float(f64::INFINITY, self.span))
                }
                "new" => self.new_object(),
                _ if is_constructor(&name) && self.peek_nth(1) == &Token::LeftParen => {
                    self.constructor(&name)
                }
                _ => Err(ParseError::new(
                    format!(
                        "unexpected identifier `{name}`, PCF only supports literal values \
                         (references and expressions need to be evaluated with `pkl` first)"
                    ),
                    self.current_span(),
                )),
            },
            _ => Err(self.unexpected("a value")),
        }
    }

    fn number(&mut self, negative: bool) -> Result<Value, ParseError> {
        let literal = self.advance();
        let start = literal.span.start;
        if self.peek() == &Token::Dot
            && let Token::Identifier {
                name: unit,
                quoted: false,
            } = self.peek_nth(1).clone()
        {
            self.advance();
            let unit_span = self.advance().span;
            let span = start..unit_span.end;
            if let Some(scale) = duration_unit(&unit) {
                let nanos = scale_number(&literal.token, negative, scale)
                    .ok_or_else(|| ParseError::new("duration is out of range", span))?;
                return Ok(Value::duration(nanos, self.span));
            }
            if let Some(scale) = data_size_unit(&unit) {
                let bytes = scale_number(&literal.token, negative, scale)
                    .ok_or_else(|| ParseError::new("data size is out of range", span))?;
                return Ok(Value::filesize(bytes, self.span));
            }
            return Err(ParseError::new(
                format!(
                    "unknown unit `{unit}`, expected a duration unit (ns, us, ms, s, min, h, d) \
                     or a data size unit (b, kb, kib, mb, mib, gb, gib, tb, tib, pb, pib)"
                ),
                unit_span,
            ));
        }
        match literal.token {
            Token::Int(magnitude) => signed(magnitude, negative)
                .map(|value| Value::int(value, self.span))
                .ok_or_else(|| ParseError::new("integer is out of range", literal.span)),
            Token::Float(value) => Ok(Value::float(
                if negative { -value } else { value },
                self.span,
            )),
            _ => unreachable!("number() is only called on numeric tokens"),
        }
    }

    fn new_object(&mut self) -> Result<Value, ParseError> {
        self.advance();
        let mut kind = ObjectKind::Dynamic;
        if let Token::Identifier {
            name,
            quoted: false,
        } = self.peek().clone()
        {
            self.advance();
            let mut type_name = name;
            while self.peek() == &Token::Dot {
                self.advance();
                match self.advance().token {
                    Token::Identifier { name, .. } => type_name = name,
                    _ => return Err(ParseError::new("expected a type name", self.current_span())),
                }
            }
            if self.peek() == &Token::LeftAngle {
                self.skip_type_arguments()?;
            }
            kind = match type_name.as_str() {
                "Listing" => ObjectKind::Listing,
                "Mapping" => ObjectKind::Mapping,
                _ => ObjectKind::Dynamic,
            };
        }
        self.object_body(kind)
    }

    fn skip_type_arguments(&mut self) -> Result<(), ParseError> {
        let open = self.advance();
        let mut depth = 1;
        while depth > 0 {
            match self.advance().token {
                Token::LeftAngle => depth += 1,
                Token::RightAngle => depth -= 1,
                Token::Eof | Token::LeftBrace => {
                    return Err(ParseError::new(
                        "unclosed type arguments, expected `>`",
                        open.span,
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Parses `( expression, ... )`, returning each argument with its source span.
    fn arguments(&mut self) -> Result<Vec<(Value, Range<usize>)>, ParseError> {
        self.expect(&Token::LeftParen, "`(`")?;
        let mut arguments = Vec::new();
        while self.peek() != &Token::RightParen {
            let start = self.current_span().start;
            let value = self.expression()?;
            arguments.push((value, start..self.previous_end()));
            if self.peek() == &Token::Comma {
                self.advance();
            } else if self.peek() != &Token::RightParen {
                return Err(self.unexpected("`,` or `)`"));
            }
        }
        self.advance();
        Ok(arguments)
    }

    fn constructor(&mut self, name: &str) -> Result<Value, ParseError> {
        let start = self.advance().span.start;
        let arguments = self.nested(Self::arguments)?;
        let span = start..self.previous_end();
        let count = arguments.len();
        let arity_error = |expected: &str| {
            ParseError::new(
                format!("`{name}` expects {expected}, got {count}"),
                span.clone(),
            )
        };
        match name {
            "List" => Ok(Value::list(
                arguments.into_iter().map(|(value, _)| value).collect(),
                self.span,
            )),
            "Set" => {
                let mut values: Vec<Value> = Vec::with_capacity(arguments.len());
                for (value, _) in arguments {
                    if !values.contains(&value) {
                        values.push(value);
                    }
                }
                Ok(Value::list(values, self.span))
            }
            "Map" => {
                if arguments.len() % 2 != 0 {
                    return Err(arity_error("alternating keys and values"));
                }
                let mut record = Record::new();
                let mut arguments = arguments.into_iter();
                while let (Some((key, key_span)), Some((value, _))) =
                    (arguments.next(), arguments.next())
                {
                    record.insert(key_to_string(&key, key_span)?, value);
                }
                Ok(Value::record(record, self.span))
            }
            "Pair" => {
                let [(first, _), (second, _)] =
                    <[_; 2]>::try_from(arguments).map_err(|_| arity_error("2 arguments"))?;
                let mut record = Record::new();
                record.push("first", first);
                record.push("second", second);
                Ok(Value::record(record, self.span))
            }
            "Regex" => match arguments.as_slice() {
                [(Value::String { val, .. }, _)] => Ok(Value::string(val.clone(), self.span)),
                [(_, argument_span)] => Err(ParseError::new(
                    "`Regex` expects a string pattern",
                    argument_span.clone(),
                )),
                _ => Err(arity_error("1 argument")),
            },
            "Bytes" => {
                let bytes = arguments
                    .into_iter()
                    .map(|(value, span)| {
                        match value {
                            Value::Int { val, .. } => u8::try_from(val).ok(),
                            _ => None,
                        }
                        .ok_or_else(|| {
                            ParseError::new("`Bytes` expects integers between 0 and 255", span)
                        })
                    })
                    .collect::<Result<Vec<u8>, _>>()?;
                Ok(Value::binary(bytes, self.span))
            }
            "IntSeq" => {
                let [(start_value, start_span), (end_value, end_span)] =
                    <[_; 2]>::try_from(arguments).map_err(|_| arity_error("2 arguments"))?;
                let seq_start = expect_int(&start_value, start_span)?;
                let seq_end = expect_int(&end_value, end_span)?;
                let step = self.int_seq_step()?;
                let next = seq_start.checked_add(step).ok_or_else(|| {
                    ParseError::new("`IntSeq` step is out of range", span.clone())
                })?;
                Ok(Value::range(
                    nu_protocol::Range::new_int(seq_start, next, Bound::Included(seq_end)),
                    self.span,
                ))
            }
            _ => unreachable!("constructor() is only called for known constructors"),
        }
    }

    /// Parses an optional `.step(n)` suffix after `IntSeq(...)`.
    fn int_seq_step(&mut self) -> Result<i64, ParseError> {
        let is_step = self.peek() == &Token::Dot
            && matches!(self.peek_nth(1), Token::Identifier { name, quoted: false } if name == "step");
        if !is_step {
            return Ok(1);
        }
        let start = self.advance().span.start;
        self.advance();
        let arguments = self.arguments()?;
        let span = start..self.previous_end();
        match arguments.as_slice() {
            [(value, argument_span)] => match expect_int(value, argument_span.clone())? {
                0 => Err(ParseError::new("`IntSeq` step must not be zero", span)),
                step => Ok(step),
            },
            _ => Err(ParseError::new("`step` expects 1 argument", span)),
        }
    }
}

/// Accumulates the members of an object body and turns them into a record or a list.
struct ObjectBuilder {
    kind: ObjectKind,
    record: Record,
    elements: Vec<Value>,
}

impl ObjectBuilder {
    fn new(kind: ObjectKind) -> Self {
        Self {
            kind,
            record: Record::new(),
            elements: Vec::new(),
        }
    }

    fn push(&mut self, member: Member) -> Result<(), ParseError> {
        let Member { kind, value, span } = member;
        let key = match kind {
            MemberKind::Element => {
                return match self.kind {
                    ObjectKind::Module => Err(ParseError::new(
                        "only properties are allowed at the top level of a PCF document",
                        span,
                    )),
                    ObjectKind::Mapping => {
                        Err(ParseError::new("a `Mapping` cannot contain elements", span))
                    }
                    _ if !self.record.is_empty() => Err(mixed_members(span)),
                    _ => {
                        self.elements.push(value);
                        Ok(())
                    }
                };
            }
            MemberKind::Property(name) => match self.kind {
                ObjectKind::Listing | ObjectKind::Mapping => {
                    return Err(ParseError::new(
                        format!("a `{}` cannot contain properties", self.kind_name()),
                        span,
                    ));
                }
                _ => name,
            },
            MemberKind::Entry(key) => match self.kind {
                ObjectKind::Module => {
                    return Err(ParseError::new(
                        "only properties are allowed at the top level of a PCF document",
                        span,
                    ));
                }
                ObjectKind::Listing => {
                    return Err(ParseError::new("a `Listing` cannot contain entries", span));
                }
                _ => key_to_string(&key, span.clone())?,
            },
        };
        if !self.elements.is_empty() {
            return Err(mixed_members(span));
        }
        if self.record.contains(&key) {
            return Err(ParseError::new(
                format!("duplicate definition of `{key}`"),
                span,
            ));
        }
        self.record.push(key, value);
        Ok(())
    }

    fn kind_name(&self) -> &'static str {
        match self.kind {
            ObjectKind::Listing => "Listing",
            ObjectKind::Mapping => "Mapping",
            ObjectKind::Dynamic | ObjectKind::Module => "Dynamic",
        }
    }

    fn finish(self, span: Span) -> Result<Value, ParseError> {
        if !self.elements.is_empty() || self.kind == ObjectKind::Listing {
            Ok(Value::list(self.elements, span))
        } else {
            Ok(Value::record(self.record, span))
        }
    }
}

fn mixed_members(span: Range<usize>) -> ParseError {
    ParseError::new(
        "objects that mix elements with properties or entries cannot be converted to Nushell values",
        span,
    )
}

fn is_constructor(name: &str) -> bool {
    matches!(
        name,
        "List" | "Set" | "Map" | "Pair" | "IntSeq" | "Regex" | "Bytes"
    )
}

/// Pkl keywords that start declarations, which have no place in static PCF.
fn is_declaration_keyword(word: &str) -> bool {
    matches!(
        word,
        "abstract"
            | "amends"
            | "class"
            | "const"
            | "extends"
            | "external"
            | "fixed"
            | "function"
            | "hidden"
            | "import"
            | "local"
            | "module"
            | "open"
            | "typealias"
    )
}

/// Converts an entry or map key to a record column name.
fn key_to_string(key: &Value, span: Range<usize>) -> Result<String, ParseError> {
    match key {
        Value::String { val, .. } => Ok(val.clone()),
        Value::Int { .. }
        | Value::Float { .. }
        | Value::Bool { .. }
        | Value::Nothing { .. }
        | Value::Duration { .. }
        | Value::Filesize { .. } => {
            writer::scalar_to_string(key).ok_or_else(|| unsupported_key(key, span))
        }
        _ => Err(unsupported_key(key, span)),
    }
}

fn unsupported_key(key: &Value, span: Range<usize>) -> ParseError {
    ParseError::new(
        format!(
            "keys of type {} cannot be converted to record keys",
            key.get_type()
        ),
        span,
    )
}

fn expect_int(value: &Value, span: Range<usize>) -> Result<i64, ParseError> {
    match value {
        Value::Int { val, .. } => Ok(*val),
        _ => Err(ParseError::new("expected an integer", span)),
    }
}

fn signed(magnitude: u64, negative: bool) -> Option<i64> {
    let value = i128::from(magnitude);
    i64::try_from(if negative { -value } else { value }).ok()
}

/// Multiplies a numeric literal by a unit scale, rounding floats to the nearest whole unit.
fn scale_number(literal: &Token, negative: bool, scale: i64) -> Option<i64> {
    let sign = if negative { -1 } else { 1 };
    match *literal {
        Token::Int(magnitude) => {
            i64::try_from(i128::from(magnitude) * i128::from(scale) * sign).ok()
        }
        Token::Float(value) => {
            let scaled = (value * scale as f64).round() * sign as f64;
            // `i64::MAX as f64` rounds up to 2^63, so the upper bound is exclusive.
            (scaled.is_finite() && scaled >= i64::MIN as f64 && scaled < i64::MAX as f64)
                .then_some(scaled as i64)
        }
        _ => None,
    }
}

fn negate(value: Value) -> Option<Value> {
    let span = value.span();
    match value {
        Value::Int { val, .. } => val.checked_neg().map(|val| Value::int(val, span)),
        Value::Float { val, .. } => Some(Value::float(-val, span)),
        Value::Duration { val, .. } => val.checked_neg().map(|val| Value::duration(val, span)),
        Value::Filesize { val, .. } => val
            .get()
            .checked_neg()
            .map(|val| Value::filesize(val, span)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nu_protocol::record;

    fn parse_ok(source: &str) -> Value {
        parse(source, Span::test_data()).unwrap_or_else(|err| panic!("{err}"))
    }

    fn property(source: &str) -> Value {
        let value = parse_ok(&format!("x = {source}"));
        value.as_record().unwrap().get("x").unwrap().clone()
    }

    fn error(source: &str) -> String {
        parse(source, Span::test_data())
            .unwrap_err()
            .message()
            .to_owned()
    }

    fn int(value: i64) -> Value {
        Value::test_int(value)
    }

    fn string(value: &str) -> Value {
        Value::test_string(value)
    }

    #[test]
    fn empty_document() {
        assert_eq!(parse_ok(""), Value::test_record(Record::new()));
        assert_eq!(
            parse_ok("// just a comment\n"),
            Value::test_record(Record::new())
        );
    }

    #[test]
    fn scalar_properties() {
        assert_eq!(
            parse_ok("name = \"Pkl\"\nport = 8080\nratio = 13.37\nenabled = true\nnothing_ = null"),
            Value::test_record(record! {
                "name" => string("Pkl"),
                "port" => int(8080),
                "ratio" => Value::test_float(13.37),
                "enabled" => Value::test_bool(true),
                "nothing_" => Value::test_nothing(),
            })
        );
    }

    #[test]
    fn quoted_property_names() {
        assert_eq!(
            parse_ok("`my-key` = 1; `class` = 2; `` = 3"),
            Value::test_record(record! {
                "my-key" => int(1),
                "class" => int(2),
                "" => int(3),
            })
        );
    }

    #[test]
    fn numbers() {
        assert_eq!(property("-5"), int(-5));
        assert_eq!(property("- 5"), int(-5));
        assert_eq!(property("--5"), int(5));
        assert_eq!(property("-9223372036854775808"), int(i64::MIN));
        assert_eq!(property("-0.5"), Value::test_float(-0.5));
        assert_eq!(property("1.0E20"), Value::test_float(1.0e20));
        assert_eq!(property("0x10"), int(16));
        assert!(matches!(property("NaN"), Value::Float { val, .. } if val.is_nan()));
        assert_eq!(property("Infinity"), Value::test_float(f64::INFINITY));
        assert_eq!(property("-Infinity"), Value::test_float(f64::NEG_INFINITY));
        assert_eq!(error("x = 9223372036854775808"), "integer is out of range");
    }

    #[test]
    fn durations() {
        assert_eq!(property("5.min"), Value::test_duration(300_000_000_000));
        assert_eq!(property("1.5.h"), Value::test_duration(5_400_000_000_000));
        assert_eq!(property("-5.min"), Value::test_duration(-300_000_000_000));
        assert_eq!(property("3.ns"), Value::test_duration(3));
        assert_eq!(property("0.5.us"), Value::test_duration(500));
        assert_eq!(property("2.d"), Value::test_duration(172_800_000_000_000));
        assert_eq!(error("x = 1000000000.d"), "duration is out of range");
        assert!(error("x = 5.lightyears").starts_with("unknown unit `lightyears`"));
    }

    #[test]
    fn data_sizes() {
        assert_eq!(property("5.mb"), Value::test_filesize(5_000_000));
        assert_eq!(property("2.5.gib"), Value::test_filesize(2_684_354_560_i64));
        assert_eq!(property("1.kib"), Value::test_filesize(1024));
        assert_eq!(property("0.b"), Value::test_filesize(0));
        assert_eq!(property("-1.kb"), Value::test_filesize(-1000));
    }

    #[test]
    fn nested_objects() {
        assert_eq!(
            parse_ok("bird {\n  name = \"Pigeon\"\n  nested { deep = true }\n}\nempty {}"),
            Value::test_record(record! {
                "bird" => Value::test_record(record! {
                    "name" => string("Pigeon"),
                    "nested" => Value::test_record(record! { "deep" => Value::test_bool(true) }),
                }),
                "empty" => Value::test_record(Record::new()),
            })
        );
    }

    #[test]
    fn listings() {
        assert_eq!(
            parse_ok("list {\n  1\n  \"two\"\n  new { a = 1 }\n  new Listing { 3 }\n}"),
            Value::test_record(record! {
                "list" => Value::test_list(vec![
                    int(1),
                    string("two"),
                    Value::test_record(record! { "a" => int(1) }),
                    Value::test_list(vec![int(3)]),
                ]),
            })
        );
        assert_eq!(property("new Listing {}"), Value::test_list(vec![]));
        assert_eq!(property("new Listing<String> {}"), Value::test_list(vec![]));
        assert_eq!(
            property("new { 1; -2; NaN }").as_list().unwrap().len(),
            3,
            "elements may be separated by semicolons"
        );
    }

    #[test]
    fn mappings() {
        assert_eq!(
            parse_ok(
                "map {\n  [\"key\"] = \"value\"\n  [1] = \"int\"\n  [true] = 0\n  [5.min] = 1\n  [\"obj\"] { a = 1 }\n}"
            ),
            Value::test_record(record! {
                "map" => Value::test_record(record! {
                    "key" => string("value"),
                    "1" => string("int"),
                    "true" => int(0),
                    "5.min" => int(1),
                    "obj" => Value::test_record(record! { "a" => int(1) }),
                }),
            })
        );
        assert_eq!(
            property("new Mapping {}"),
            Value::test_record(Record::new())
        );
    }

    #[test]
    fn constructors() {
        assert_eq!(
            property("List(1, 2, new Dynamic { a = 1 },)"),
            Value::test_list(vec![
                int(1),
                int(2),
                Value::test_record(record! { "a" => int(1) })
            ])
        );
        assert_eq!(property("List()"), Value::test_list(vec![]));
        assert_eq!(
            property("Set(1, 2, 1)"),
            Value::test_list(vec![int(1), int(2)])
        );
        assert_eq!(
            property("Map(\"a\", 1, 2, List(1), \"a\", 3)"),
            Value::test_record(record! {
                "a" => int(3),
                "2" => Value::test_list(vec![int(1)]),
            })
        );
        assert_eq!(
            property("Pair(1, \"two\")"),
            Value::test_record(record! { "first" => int(1), "second" => string("two") })
        );
        assert_eq!(property("Regex(#\"\\d+\"#)"), string("\\d+"));
        assert_eq!(
            property("Bytes(1, 2, 255)"),
            Value::test_binary(vec![1, 2, 255])
        );
    }

    #[test]
    fn int_seqs() {
        assert_eq!(
            property("IntSeq(1, 10)"),
            Value::test_range(nu_protocol::Range::new_int(1, 2, Bound::Included(10)))
        );
        assert_eq!(
            property("IntSeq(10, 1).step(-3)"),
            Value::test_range(nu_protocol::Range::new_int(10, 7, Bound::Included(1)))
        );
        assert_eq!(
            error("x = IntSeq(1, 10).step(0)"),
            "`IntSeq` step must not be zero"
        );
    }

    #[test]
    fn constructor_errors() {
        assert_eq!(
            error("x = Map(1)"),
            "`Map` expects alternating keys and values, got 1"
        );
        assert_eq!(error("x = Pair(1)"), "`Pair` expects 2 arguments, got 1");
        assert_eq!(
            error("x = Bytes(256)"),
            "`Bytes` expects integers between 0 and 255"
        );
        assert_eq!(error("x = Regex(1)"), "`Regex` expects a string pattern");
        assert_eq!(error("x = IntSeq(1.5, 2)"), "expected an integer");
        assert_eq!(
            error("x = Map(List(1), 2)"),
            "keys of type list<int> cannot be converted to record keys"
        );
    }

    #[test]
    fn structural_errors() {
        assert_eq!(
            error("1"),
            "only properties are allowed at the top level of a PCF document"
        );
        assert_eq!(
            error("[\"a\"] = 1"),
            "only properties are allowed at the top level of a PCF document"
        );
        assert_eq!(
            error("x { a = 1; 2 }"),
            "objects that mix elements with properties or entries cannot be converted to Nushell values"
        );
        assert_eq!(
            error("x { 2; a = 1 }"),
            "objects that mix elements with properties or entries cannot be converted to Nushell values"
        );
        assert_eq!(error("a = 1\na = 2"), "duplicate definition of `a`");
        assert_eq!(
            error("x { a = 1; [\"a\"] = 2 }"),
            "duplicate definition of `a`"
        );
        assert_eq!(
            error("x = new Listing { a = 1 }"),
            "a `Listing` cannot contain properties"
        );
        assert_eq!(
            error("x = new Listing { [0] = 1 }"),
            "a `Listing` cannot contain entries"
        );
        assert_eq!(
            error("x = new Mapping { 1 }"),
            "a `Mapping` cannot contain elements"
        );
        assert_eq!(error("x {"), "unclosed object body, expected `}`");
        assert_eq!(error("x 1"), "expected `=` or `{`, found a number");
        assert_eq!(error("x = "), "expected a value, found end of input");
        assert_eq!(
            error("x = -\"a\""),
            "`-` can only be applied to numbers, durations and data sizes"
        );
    }

    #[test]
    fn non_static_pkl_is_rejected() {
        assert!(error("amends \"base.pkl\"").starts_with("`amends` is not supported in PCF"));
        assert!(error("import \"foo.pkl\"").starts_with("`import` is not supported in PCF"));
        assert!(error("local x = 1").starts_with("`local` is not supported in PCF"));
        assert!(error("x = y").starts_with("unexpected identifier `y`"));
        assert_eq!(error("x { y }"), "expected `=` or `{`, found `}`");
    }

    #[test]
    fn keywords_must_be_quoted_to_be_property_names() {
        assert_eq!(
            error("null = 1"),
            "keyword `null` cannot be a property name unless it is quoted with backticks"
        );
        assert_eq!(
            error("x { class { a = 1 } }"),
            "keyword `class` cannot be a property name unless it is quoted with backticks"
        );
    }

    #[test]
    fn nesting_is_limited() {
        let nested = |depth: usize, open: &str, close: &str| {
            format!("x = {}1{}", open.repeat(depth), close.repeat(depth))
        };
        let too_deep = "values are nested more than 128 levels deep";

        for (open, close) in [("new { ", " }"), ("List(", ")")] {
            assert!(parse(&nested(MAX_DEPTH, open, close), Span::test_data()).is_ok());
            assert_eq!(error(&nested(MAX_DEPTH + 1, open, close)), too_deep);
        }
        assert_eq!(error(&nested(10_000, "-", "")), too_deep);
        assert_eq!(error(&"a { ".repeat(100_000)), too_deep);
    }

    #[test]
    fn windows_line_endings() {
        assert_eq!(
            parse_ok(
                "a = 1\r\n// comment\r\nb {\r\n  \"x\"\r\n}\r\nc = \"\"\"\r\n  y\r\n  \"\"\"\r\n"
            ),
            Value::test_record(record! {
                "a" => int(1),
                "b" => Value::test_list(vec![string("x")]),
                "c" => string("y"),
            })
        );
    }

    #[test]
    fn value_words_can_be_property_names() {
        assert_eq!(
            parse_ok("NaN = 1\nList { 2 }\nIntSeq = 3\nx { Infinity; List(4) }"),
            Value::test_record(record! {
                "NaN" => int(1),
                "List" => Value::test_list(vec![int(2)]),
                "IntSeq" => int(3),
                "x" => Value::test_list(vec![
                    Value::test_float(f64::INFINITY),
                    Value::test_list(vec![int(4)]),
                ]),
            })
        );
    }

    #[test]
    fn error_span_points_at_offending_member() {
        let source = "a = 1\nb { c = 1; 2 }";
        let err = parse(source, Span::test_data()).unwrap_err();
        assert_eq!(&source[err.span()], "2");
        assert_eq!(err.position(source), (2, 12));
        assert_eq!(err.snippet(source), "2 | b { c = 1; 2 }\n  |            ^");
    }
}
