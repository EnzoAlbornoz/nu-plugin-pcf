//! Checks the plugin against output of Pkl's own PCF renderer.
//!
//! The golden tests read committed fixtures and always run. The interop tests call the `pkl` CLI
//! and are skipped when it is not installed (`mise install` provides it).

mod common;

use std::ops::Bound;

use common::{Pkl, fixture, read_fixture};
use nu_plugin_pcf::pcf::{self, WriteOptions};
use nu_protocol::{Range, Record, Span, Value, record};

fn parse(source: &str) -> Value {
    pcf::parse(source, Span::test_data()).unwrap_or_else(|err| {
        panic!(
            "failed to parse: {err}\n{}\n--- source ---\n{source}",
            err.snippet(source)
        )
    })
}

fn write(value: &Value) -> String {
    pcf::write(value, &WriteOptions::default()).unwrap_or_else(|err| panic!("{err}"))
}

/// Removes the field `key` from a record value, asserting that it is NaN.
fn take_nan(value: &mut Value, key: &str) {
    let Value::Record { val, .. } = value else {
        panic!("expected a record");
    };
    match val.to_mut().remove(key) {
        Some(Value::Float { val, .. }) if val.is_nan() => {}
        other => panic!("expected `{key}` to be NaN, got {other:?}"),
    }
}

fn s(value: &str) -> Value {
    Value::test_string(value)
}

fn int(value: i64) -> Value {
    Value::test_int(value)
}

fn list(values: Vec<Value>) -> Value {
    Value::test_list(values)
}

fn rec(record: Record) -> Value {
    Value::test_record(record)
}

#[test]
fn parses_every_value_type_rendered_by_pkl() {
    let mut value = parse(&read_fixture("all_types.pcf"));
    take_nan(&mut value, "notANumber");

    let expected = rec(record! {
        "string" => s("Pkl"),
        "int" => int(100),
        "negativeInt" => int(-5),
        "hexInt" => int(255),
        "float" => Value::test_float(13.37),
        "exponentFloat" => Value::test_float(1.0e20),
        "tinyFloat" => Value::test_float(1.5e-7),
        "wholeFloat" => Value::test_float(100.0),
        "infinity" => Value::test_float(f64::INFINITY),
        "negativeInfinity" => Value::test_float(f64::NEG_INFINITY),
        "boolean" => Value::test_bool(true),
        "null" => Value::test_nothing(),
        "duration" => Value::test_duration(300_000_000_000),
        "fractionalDuration" => Value::test_duration(5_400_000_000_000),
        "dataSize" => Value::test_filesize(5_000_000),
        "binaryDataSize" => Value::test_filesize(2_684_354_560_i64),
        "quoted-name" => s("quoted"),
        "class" => s("keyword"),
        "escapes" => s("tab\t quote\" backslash\\ cr\r emoji😀 \\(not interpolated)"),
        "multiLine" => s("line one\nline \"two\" with \"\"\"\n\nlast"),
        "trailingNewline" => s("a\nb\n"),
        "dynamic" => rec(record! {
            "name" => s("Pigeon"),
            "nested" => rec(record! { "deep" => Value::test_bool(true) }),
        }),
        "typed" => rec(record! { "name" => s("Ann"), "age" => Value::test_nothing() }),
        "listing" => list(vec![
            int(1),
            s("two"),
            rec(record! { "a" => int(1) }),
            list(vec![s("nested")]),
        ]),
        "mapping" => rec(record! {
            "key" => s("value"),
            "1" => s("int key"),
            "true" => s("bool key"),
            "object" => rec(record! { "a" => int(1) }),
        }),
        "empty" => rec(Record::new()),
        "list" => list(vec![int(1), int(2), rec(record! { "a" => int(1) })]),
        "emptyList" => list(vec![]),
        "set" => list(vec![int(1), int(2)]),
        "map" => rec(record! { "a" => int(1), "b" => list(vec![int(1)]) }),
        "pair" => rec(record! { "first" => int(1), "second" => s("two") }),
        "intSeq" => Value::test_range(Range::new_int(1, 2, Bound::Included(10))),
        "steppedIntSeq" => Value::test_range(Range::new_int(10, 7, Bound::Included(1))),
        "regex" => s("\\d+"),
        "bytes" => Value::test_binary(vec![1, 2, 255]),
    });
    assert_eq!(value, expected);
}

#[test]
fn fixture_matches_current_pkl_output() {
    let Some(pkl) = Pkl::find() else { return };
    assert_eq!(
        pkl.render_pcf(&fixture("all_types.pkl")),
        read_fixture("all_types.pcf"),
        "all_types.pcf is out of date, regenerate it with `mise run fixtures`"
    );
}

/// `pkl` → `from pcf` → `to pcf` → `pkl` must reproduce Pkl's original rendering.
#[test]
fn round_trips_through_pkl() {
    let Some(pkl) = Pkl::find() else { return };
    let rendered = pkl.render_pcf(&fixture("round_trip.pkl"));
    let written = write(&parse(&rendered));
    let rerendered = pkl.render_pcf_source("round_trip_written", &written);
    assert_eq!(rerendered, rendered, "PCF written by `to pcf`:\n{written}");
}

/// Everything `to pcf` writes must be valid Pkl that evaluates to the same data.
#[test]
fn written_pcf_is_valid_pkl() {
    let Some(pkl) = Pkl::find() else { return };
    let date = "2024-01-02T03:04:05+01:00";
    let value = rec(record! {
        "string" => s("hello"),
        "escapes" => s("tab\t quote\" backslash\\ cr\r \\(x) nul\0 bell\u{7} del\u{7f} π 😀"),
        "multiLine" => s("a\n\"\"\"\"\n\n\tindented \\ \"quoted\"\r\nlast\n"),
        "onlyNewline" => s("\n"),
        "maxInt" => int(i64::MAX),
        "minInt" => int(i64::MIN),
        "float" => Value::test_float(1.5e300),
        "negativeZero" => Value::test_float(-0.0),
        "infinity" => Value::test_float(f64::NEG_INFINITY),
        "bool" => Value::test_bool(true),
        "nothing" => Value::test_nothing(),
        "durations" => list(vec![
            Value::test_duration(0),
            Value::test_duration(90 * 60 * 1_000_000_000),
            Value::test_duration(-1_500_000),
            Value::test_duration(7),
        ]),
        "filesizes" => list(vec![
            Value::test_filesize(0),
            Value::test_filesize(1_000),
            Value::test_filesize(3 << 20),
            Value::test_filesize(1_536),
        ]),
        "binary" => Value::test_binary(vec![0, 1, 255]),
        "emptyBinary" => Value::test_binary(vec![]),
        "range" => Value::test_range(Range::new_int(1, 2, Bound::Included(5))),
        "exclusiveRange" => Value::test_range(Range::new_int(0, 2, Bound::Excluded(10))),
        "date" => Value::test_date(date.parse().unwrap()),
        // Keys that cannot be quoted identifiers are written as entries, which Pkl renders
        // after the properties, so they come last here to keep the order comparable.
        "keys" => rec(record! {
            "plain" => int(1),
            "kebab-case" => int(2),
            "class" => int(3),
            "" => int(4),
            "π" => int(5),
            "back`tick" => int(6),
            "line\nbreak" => int(7),
        }),
        "table" => list(vec![
            rec(record! { "name" => s("a"), "size" => int(1) }),
            rec(record! { "name" => s("b"), "size" => int(2) }),
        ]),
        "nested" => list(vec![
            list(vec![int(1), list(vec![int(2)])]),
            list(vec![]),
            rec(Record::new()),
            rec(record! { "inner" => list(vec![rec(record! { "deep" => s("x\ny") })]) }),
        ]),
        "emptyRecord" => rec(Record::new()),
    });

    let written = write(&value);
    let rendered = pkl.render_pcf_source("written", &written);
    let mut actual = parse(&rendered);

    // Pkl renders an empty `Listing` as `{}`, which reads back as an empty record. PCF has no date
    // type, so dates are written as strings, and `IntSeq` ends are inclusive.
    let mut expected = value;
    expected
        .recurse_mut(&mut |value| {
            match value {
                Value::List { vals, .. } if vals.is_empty() => {
                    *value = rec(Record::new());
                }
                Value::Date { val, .. } => *value = s(&val.to_rfc3339()),
                Value::Range { val, .. } => {
                    if let Range::IntRange(range) = **val
                        && let Bound::Excluded(end) = range.end()
                    {
                        let step = range.step();
                        let inclusive_end = Bound::Included(end - step.signum());
                        **val = Range::new_int(range.start(), range.start() + step, inclusive_end);
                    }
                }
                _ => {}
            }
            Ok::<(), std::convert::Infallible>(())
        })
        .unwrap();
    actual
        .recurse_mut(&mut |value| {
            value.set_span(Span::test_data());
            Ok::<(), std::convert::Infallible>(())
        })
        .unwrap();
    assert_eq!(actual, expected, "PCF written by `to pcf`:\n{written}");
}
