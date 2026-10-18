//! Runs the commands inside a Nushell engine, the way users call them.

use nu_plugin_pcf::{FromPcf, PcfPlugin, ToPcf};
use nu_plugin_test_support::PluginTest;
use nu_protocol::{ShellError, Span, Value};

fn plugin_test() -> PluginTest {
    PluginTest::new("pcf", PcfPlugin.into()).expect("failed to load the plugin")
}

/// Evaluates `source`, describing any error with its debug representation, which includes the
/// error code, labels and help text.
fn eval_with(test: &mut PluginTest, source: &str) -> Result<Value, String> {
    let describe = |err: ShellError| format!("{err:?}");
    let output = test.eval(source).map_err(describe)?;
    output.into_value(Span::test_data()).map_err(describe)
}

fn eval_ok(source: &str) -> Value {
    eval_with(&mut plugin_test(), source).unwrap_or_else(|err| panic!("`{source}` failed: {err}"))
}

fn eval_err(source: &str) -> String {
    match eval_with(&mut plugin_test(), source) {
        Ok(value) => panic!("`{source}` should fail, got {value:?}"),
        Err(err) => err,
    }
}

/// Asserts that `data | to pcf | from pcf` gives back `data`.
fn assert_round_trip(data: &str) {
    let mut test = plugin_test();
    let expected = eval_with(&mut test, data)
        .unwrap_or_else(|err| panic!("failed to evaluate `{data}`: {err}"));
    let actual = eval_with(&mut test, &format!("{data} | to pcf | from pcf"))
        .unwrap_or_else(|err| panic!("round trip of `{data}` failed: {err}"));
    assert_eq!(actual, expected, "round trip of `{data}`");
}

#[test]
fn from_pcf_examples() {
    plugin_test()
        .test_command_examples(&FromPcf)
        .expect("`from pcf` examples failed");
}

#[test]
fn to_pcf_examples() {
    plugin_test()
        .test_command_examples(&ToPcf)
        .expect("`to pcf` examples failed");
}

#[test]
fn round_trips_records() {
    assert_round_trip("{name: Pkl, port: 8080, ratio: 0.5, enabled: true, comment: null}");
    assert_round_trip("{server: {hosts: [a b], limits: {cpu: 2, memory: 512MiB}}}");
    assert_round_trip(
        "{timeouts: [1sec 1.5sec 90min 3day 250ms 42ns], sizes: [1kB 1KiB 1.5MB 0B]}",
    );
    assert_round_trip("{seq: 1..10, stepped: 1..3..9, down: 10..7..1, bytes: 0x[00 7f ff]}");
    assert_round_trip("{empty_list: [], empty_record: {}, nested: [[1 [2]] [] {}]}");
    assert_round_trip("{table: [[name size]; [a 1] [b 2]]}");
    assert_round_trip(
        r#"{"kebab-case": 1, class: 2, "": 3, nested: {"back`tick": 4, "line\nbreak": 5}}"#,
    );
    assert_round_trip(
        r#"{text: "tab\t quote\" backslash\\ \\(x) π", lines: "first\n  second \"\"\"\n\nlast\n"}"#,
    );
}

#[test]
fn dates_become_strings() {
    assert_eq!(
        eval_ok("{when: 2024-01-02T03:04:05Z} | to pcf"),
        Value::test_string("`when` = \"2024-01-02T03:04:05+00:00\"\n")
    );
}

#[test]
fn indentation_flags() {
    let data = "{a: {b: [1]}}";
    assert_eq!(
        eval_ok(&format!("{data} | to pcf --tabs 1")),
        Value::test_string("a {\n\tb {\n\t\t1\n\t}\n}\n")
    );
    assert_eq!(
        eval_ok(&format!("{data} | to pcf -i 0")),
        Value::test_string("a {\nb {\n1\n}\n}\n")
    );
    let err = eval_err(&format!("{data} | to pcf --indent 2 --tabs 1"));
    assert!(err.contains("nu::shell::incompatible_parameters"), "{err}");
    let err = eval_err(&format!("{data} | to pcf --indent -1"));
    assert!(err.contains("nu::shell::needs_positive_value"), "{err}");
}

#[test]
fn from_pcf_reports_parse_errors() {
    let err = eval_err("'a = 1\nb { c = 1; 2 }' | from pcf");
    assert!(err.contains("Failed to parse PCF"), "{err}");
    assert!(
        err.contains(
            "objects that mix elements with properties or entries cannot be converted to Nushell \
             values at line 2, column 12"
        ),
        "{err}"
    );

    let err = eval_err("'name = \"\\(other)\"' | from pcf");
    assert!(
        err.contains("string interpolation is not supported in PCF"),
        "{err}"
    );
}

#[test]
fn from_pcf_accepts_binary_input() {
    assert_eq!(eval_ok("0x[61 20 3d 20 31] | from pcf"), eval_ok("{a: 1}"));
}

#[test]
fn to_pcf_reports_unsupported_values() {
    let err = eval_err("{f: {|| 1}} | to pcf");
    assert!(err.contains("closures cannot be converted to PCF"), "{err}");

    let err = eval_err("{r: 1..} | to pcf");
    assert!(
        err.contains("unbounded ranges cannot be converted to PCF"),
        "{err}"
    );

    let err = eval_err("{r: 1.0..2.0} | to pcf");
    assert!(
        err.contains("float ranges cannot be converted to PCF"),
        "{err}"
    );
}

#[test]
fn to_pcf_requires_a_record() {
    let err = eval_err("[1 2] | to pcf");
    assert!(err.contains("list<int>"), "{err}");
}
