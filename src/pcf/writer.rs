//! Renders Nushell values as PCF, following the layout of Pkl's own `PcfRenderer`.

use std::{borrow::Cow, fmt::Write as _, ops::Bound};

use nu_protocol::{Range, Record, Value};

use super::{
    WriteError,
    syntax::{DATA_SIZE_UNITS, DURATION_UNITS, is_plain_identifier, is_quotable_identifier},
};

/// Options that control how PCF is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOptions {
    /// Text used for one level of indentation.
    pub indent: String,
    /// Leave out record fields whose value is `null`.
    pub omit_null_properties: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            indent: "  ".into(),
            omit_null_properties: false,
        }
    }
}

/// Renders a record as a PCF document.
pub fn write(value: &Value, options: &WriteOptions) -> Result<String, WriteError> {
    let value = resolve(value)?;
    let Value::Record { val: record, .. } = value.as_ref() else {
        return Err(WriteError::new(
            format!(
                "PCF documents must be records at the top level, got {}",
                value.get_type()
            ),
            value.span(),
        )
        .with_help("wrap the value in a record, for example `{items: $data} | to pcf`"));
    };
    let mut writer = Writer {
        out: String::new(),
        options,
        level: 0,
    };
    for (key, value) in record.iter() {
        if !is_quotable_identifier(key) {
            return Err(WriteError::new(
                format!(
                    "`{}` cannot be used as a top-level property name",
                    key.escape_debug()
                ),
                value.span(),
            )
            .with_help("top-level property names cannot contain backticks or line breaks"));
        }
        writer.member(key, value)?;
    }
    Ok(writer.out)
}

/// Renders a scalar as it appears in PCF source, or `None` for values that need more context.
pub(crate) fn scalar_to_string(value: &Value) -> Option<String> {
    Some(match value {
        Value::Bool { val, .. } => val.to_string(),
        Value::Int { val, .. } => val.to_string(),
        Value::Float { val, .. } => format_float(*val),
        Value::Nothing { .. } => "null".into(),
        Value::Duration { val, .. } => format_with_unit(*val, DURATION_UNITS, "s"),
        Value::Filesize { val, .. } => format_with_unit(val.get(), DATA_SIZE_UNITS, "b"),
        _ => return None,
    })
}

struct Writer<'a> {
    out: String,
    options: &'a WriteOptions,
    level: usize,
}

impl Writer<'_> {
    fn indent(&mut self) {
        for _ in 0..self.level {
            self.out.push_str(&self.options.indent);
        }
    }

    fn skips(&self, value: &Value) -> bool {
        self.options.omit_null_properties && matches!(value, Value::Nothing { .. })
    }

    /// Writes a record field on its own line, as a property or (if the key cannot be an
    /// identifier) as an entry.
    fn member(&mut self, key: &str, value: &Value) -> Result<(), WriteError> {
        if self.skips(value) {
            return Ok(());
        }
        let value = resolve(value)?;
        self.indent();
        if is_plain_identifier(key) {
            self.out.push_str(key);
        } else if is_quotable_identifier(key) {
            let _ = write!(self.out, "`{key}`");
        } else {
            self.out.push('[');
            single_line_string(key, &mut self.out);
            self.out.push(']');
        }
        match value.as_ref() {
            Value::Record { val, .. } => {
                self.out.push(' ');
                self.object(val)?;
            }
            Value::List { vals, .. } if !vals.is_empty() => {
                self.out.push(' ');
                self.listing(vals)?;
            }
            other => {
                self.out.push_str(" = ");
                self.inline(other)?;
            }
        }
        self.out.push('\n');
        Ok(())
    }

    fn object(&mut self, record: &Record) -> Result<(), WriteError> {
        if record.values().all(|value| self.skips(value)) {
            self.out.push_str("{}");
            return Ok(());
        }
        self.out.push_str("{\n");
        self.level += 1;
        for (key, value) in record.iter() {
            self.member(key, value)?;
        }
        self.level -= 1;
        self.indent();
        self.out.push('}');
        Ok(())
    }

    fn listing(&mut self, values: &[Value]) -> Result<(), WriteError> {
        self.out.push_str("{\n");
        self.level += 1;
        for value in values {
            let value = resolve(value)?;
            self.indent();
            match value.as_ref() {
                Value::Record { val, .. } => {
                    self.out.push_str("new ");
                    self.object(val)?;
                }
                Value::List { vals, .. } if !vals.is_empty() => {
                    self.out.push_str("new ");
                    self.listing(vals)?;
                }
                other => self.inline(other)?,
            }
            self.out.push('\n');
        }
        self.level -= 1;
        self.indent();
        self.out.push('}');
        Ok(())
    }

    /// Writes a value that does not need a block of its own.
    fn inline(&mut self, value: &Value) -> Result<(), WriteError> {
        if let Some(text) = scalar_to_string(value) {
            self.out.push_str(&text);
            return Ok(());
        }
        let span = value.span();
        match value {
            Value::String { val, .. } | Value::Glob { val, .. } => self.string(val),
            Value::Date { val, .. } => single_line_string(&val.to_rfc3339(), &mut self.out),
            Value::CellPath { val, .. } => single_line_string(&val.to_string(), &mut self.out),
            Value::List { .. } => self.out.push_str("new Listing {}"),
            Value::Binary { val, .. } => {
                self.out.push_str("Bytes(");
                for (i, byte) in val.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    let _ = write!(self.out, "{byte}");
                }
                self.out.push(')');
            }
            Value::Range { val, .. } => {
                let Range::IntRange(range) = val.as_ref() else {
                    return Err(
                        WriteError::new("float ranges cannot be converted to PCF", span)
                            .with_help("only integer ranges have a PCF equivalent (`IntSeq`)"),
                    );
                };
                let step = range.step();
                let end = match range.end() {
                    Bound::Included(end) => Some(end),
                    Bound::Excluded(end) => end.checked_sub(step.signum()),
                    Bound::Unbounded => {
                        return Err(WriteError::new(
                            "unbounded ranges cannot be converted to PCF",
                            span,
                        ));
                    }
                }
                .ok_or_else(|| WriteError::new("range end is out of bounds", span))?;
                let _ = write!(self.out, "IntSeq({}, {end})", range.start());
                if step != 1 {
                    let _ = write!(self.out, ".step({step})");
                }
            }
            Value::Closure { .. } => {
                return Err(WriteError::new("closures cannot be converted to PCF", span));
            }
            Value::Error { error, .. } => {
                return Err(WriteError::new(error.to_string(), span));
            }
            other => {
                return Err(WriteError::new(
                    format!(
                        "values of type {} cannot be converted to PCF",
                        other.get_type()
                    ),
                    span,
                ));
            }
        }
        Ok(())
    }

    /// Writes a string, using a multi-line literal when it contains line breaks.
    fn string(&mut self, value: &str) {
        if !value.contains('\n') {
            single_line_string(value, &mut self.out);
            return;
        }
        self.level += 1;
        self.out.push_str("\"\"\"\n");
        for line in value.split('\n') {
            self.indent();
            escape(line, true, &mut self.out);
            self.out.push('\n');
        }
        self.indent();
        self.out.push_str("\"\"\"");
        self.level -= 1;
    }
}

/// Replaces custom values with their base value.
fn resolve(value: &Value) -> Result<Cow<'_, Value>, WriteError> {
    match value {
        Value::Custom { val, .. } => val
            .to_base_value(value.span())
            .map(Cow::Owned)
            .map_err(|err| WriteError::new(err.to_string(), value.span())),
        _ => Ok(Cow::Borrowed(value)),
    }
}

fn single_line_string(value: &str, out: &mut String) {
    out.push('"');
    escape(value, false, out);
    out.push('"');
}

fn escape(value: &str, multi_line: bool, out: &mut String) {
    // In multi-line strings only a run of three quotes is significant, so every third one is
    // escaped, just like Pkl does it.
    let mut quotes = 0;
    for c in value.chars() {
        if c == '"' && multi_line {
            quotes += 1;
            if quotes == 3 {
                out.push_str("\\\"");
                quotes = 0;
            } else {
                out.push('"');
            }
            continue;
        }
        quotes = 0;
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:X}}}", u32::from(c));
            }
            c => out.push(c),
        }
    }
}

fn format_float(value: f64) -> String {
    if value.is_nan() {
        "NaN".into()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else {
        // Debug formatting is the shortest representation that round-trips and always contains
        // a `.` or an exponent, which keeps the literal a float in Pkl.
        format!("{value:?}")
    }
}

/// Formats `amount` with the largest unit that represents it exactly.
fn format_with_unit(amount: i64, units: &[(&str, i64)], zero_unit: &str) -> String {
    if amount == 0 {
        return format!("0.{zero_unit}");
    }
    let (unit, scale) = units
        .iter()
        .find(|(_, scale)| amount % scale == 0)
        .expect("the smallest unit has a scale of 1");
    format!("{}.{unit}", amount / scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nu_protocol::{Span, record};

    fn render(value: Value) -> String {
        write(&value, &WriteOptions::default()).unwrap_or_else(|err| panic!("{err}"))
    }

    fn property(value: Value) -> String {
        let rendered = render(Value::test_record(record! { "x" => value }));
        rendered
            .strip_prefix("x = ")
            .unwrap_or(&rendered)
            .trim_end_matches('\n')
            .to_owned()
    }

    #[test]
    fn scalars() {
        assert_eq!(property(Value::test_int(-42)), "-42");
        assert_eq!(property(Value::test_bool(true)), "true");
        assert_eq!(property(Value::test_nothing()), "null");
        assert_eq!(property(Value::test_float(1.0)), "1.0");
        assert_eq!(property(Value::test_float(13.37)), "13.37");
        assert_eq!(property(Value::test_float(1e20)), "1e20");
        assert_eq!(property(Value::test_float(1.5e-7)), "1.5e-7");
        assert_eq!(property(Value::test_float(f64::NAN)), "NaN");
        assert_eq!(property(Value::test_float(f64::INFINITY)), "Infinity");
        assert_eq!(property(Value::test_float(f64::NEG_INFINITY)), "-Infinity");
    }

    #[test]
    fn durations() {
        assert_eq!(property(Value::test_duration(0)), "0.s");
        assert_eq!(property(Value::test_duration(300_000_000_000)), "5.min");
        assert_eq!(property(Value::test_duration(5_400_000_000_000)), "90.min");
        assert_eq!(property(Value::test_duration(-1_500_000)), "-1500.us");
        assert_eq!(property(Value::test_duration(86_400_000_000_000)), "1.d");
        assert_eq!(property(Value::test_duration(7)), "7.ns");
    }

    #[test]
    fn data_sizes() {
        assert_eq!(property(Value::test_filesize(0)), "0.b");
        assert_eq!(property(Value::test_filesize(1000)), "1.kb");
        assert_eq!(property(Value::test_filesize(1024)), "1.kib");
        assert_eq!(property(Value::test_filesize(5_000_000)), "5.mb");
        assert_eq!(property(Value::test_filesize(1536)), "1536.b");
        assert_eq!(property(Value::test_filesize(-2048)), "-2.kib");
    }

    #[test]
    fn strings() {
        assert_eq!(property(Value::test_string("")), r#""""#);
        assert_eq!(
            property(Value::test_string("tab\t \"q\" \\ \r \\(x) bell\u{7}")),
            r#""tab\t \"q\" \\ \r \\(x) bell\u{7}""#
        );
        assert_eq!(property(Value::test_string("π 😀")), r#""π 😀""#);
    }

    #[test]
    fn multi_line_strings() {
        assert_eq!(
            render(Value::test_record(record! {
                "x" => Value::test_string("line one\nline \"two\" \"\"\"\"\n\nend\n"),
            })),
            "x = \"\"\"\n  line one\n  line \"two\" \"\"\\\"\"\n  \n  end\n  \n  \"\"\"\n"
        );
        assert_eq!(
            render(Value::test_record(record! {
                "o" => Value::test_record(record! { "x" => Value::test_string("a\nb") }),
            })),
            "o {\n  x = \"\"\"\n    a\n    b\n    \"\"\"\n}\n"
        );
    }

    #[test]
    fn property_names() {
        assert_eq!(
            render(Value::test_record(record! {
                "plain_$1" => Value::test_int(1),
                "my-key" => Value::test_int(2),
                "class" => Value::test_int(3),
                "π" => Value::test_int(4),
                "" => Value::test_int(5),
            })),
            "plain_$1 = 1\n`my-key` = 2\n`class` = 3\n`π` = 4\n`` = 5\n"
        );
    }

    #[test]
    fn unquotable_keys_become_entries() {
        assert_eq!(
            render(Value::test_record(record! {
                "o" => Value::test_record(record! {
                    "back`tick" => Value::test_int(1),
                    "line\nbreak" => Value::test_int(2),
                }),
            })),
            "o {\n  [\"back`tick\"] = 1\n  [\"line\\nbreak\"] = 2\n}\n"
        );
        let err = write(
            &Value::test_record(record! { "a`b" => Value::test_int(1) }),
            &WriteOptions::default(),
        )
        .unwrap_err();
        assert_eq!(
            err.message(),
            "`a`b` cannot be used as a top-level property name"
        );
    }

    #[test]
    fn nested_structures() {
        let value = Value::test_record(record! {
            "bird" => Value::test_record(record! {
                "name" => Value::test_string("Pigeon"),
                "tags" => Value::test_list(vec![Value::test_string("a"), Value::test_string("b")]),
            }),
            "people" => Value::test_list(vec![
                Value::test_record(record! { "name" => Value::test_string("Ann") }),
                Value::test_record(Record::new()),
                Value::test_list(vec![Value::test_int(1)]),
                Value::test_list(vec![]),
            ]),
            "empty_record" => Value::test_record(Record::new()),
            "empty_list" => Value::test_list(vec![]),
        });
        assert_eq!(
            render(value),
            "\
bird {
  name = \"Pigeon\"
  tags {
    \"a\"
    \"b\"
  }
}
people {
  new {
    name = \"Ann\"
  }
  new {}
  new {
    1
  }
  new Listing {}
}
empty_record {}
empty_list = new Listing {}
"
        );
    }

    #[test]
    fn indentation_options() {
        let value = Value::test_record(record! {
            "o" => Value::test_record(record! { "a" => Value::test_int(1) }),
        });
        let options = WriteOptions {
            indent: "\t".into(),
            ..WriteOptions::default()
        };
        assert_eq!(write(&value, &options).unwrap(), "o {\n\ta = 1\n}\n");
    }

    #[test]
    fn omit_null_properties() {
        let value = Value::test_record(record! {
            "a" => Value::test_nothing(),
            "o" => Value::test_record(record! { "b" => Value::test_nothing() }),
            "l" => Value::test_list(vec![Value::test_nothing()]),
        });
        let options = WriteOptions {
            omit_null_properties: true,
            ..WriteOptions::default()
        };
        assert_eq!(write(&value, &options).unwrap(), "o {}\nl {\n  null\n}\n");
    }

    #[test]
    fn other_types() {
        assert_eq!(
            property(Value::test_binary(vec![0, 127, 255])),
            "Bytes(0, 127, 255)"
        );
        assert_eq!(property(Value::test_binary(vec![])), "Bytes()");
        assert_eq!(
            property(Value::test_range(Range::new_int(1, 2, Bound::Included(10)))),
            "IntSeq(1, 10)"
        );
        assert_eq!(
            property(Value::test_range(Range::new_int(1, 3, Bound::Excluded(9)))),
            "IntSeq(1, 8).step(2)"
        );
        assert_eq!(
            property(Value::test_range(Range::new_int(5, 4, Bound::Excluded(1)))),
            "IntSeq(5, 2).step(-1)"
        );
        assert_eq!(property(Value::test_glob("*.pkl")), r#""*.pkl""#);
    }

    #[test]
    fn unsupported_values() {
        let err = write(
            &Value::test_record(record! {
                "r" => Value::test_range(Range::new_int(1, 2, Bound::Unbounded)),
            }),
            &WriteOptions::default(),
        )
        .unwrap_err();
        assert_eq!(err.message(), "unbounded ranges cannot be converted to PCF");

        let err = write(&Value::test_list(vec![]), &WriteOptions::default()).unwrap_err();
        assert_eq!(
            err.message(),
            "PCF documents must be records at the top level, got list<any>"
        );
        assert_eq!(err.span(), Span::test_data());
    }
}
