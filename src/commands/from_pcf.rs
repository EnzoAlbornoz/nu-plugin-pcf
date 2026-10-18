use nu_plugin::{EngineInterface, EvaluatedCall, SimplePluginCommand};
use nu_protocol::{
    Category, Example, LabeledError, Record, ShellError, Signature, Span, Type, Value, record,
};

use crate::{PcfPlugin, pcf};

/// `from pcf`: parses PCF text into structured data.
pub struct FromPcf;

impl SimplePluginCommand for FromPcf {
    type Plugin = PcfPlugin;

    fn name(&self) -> &str {
        "from pcf"
    }

    fn description(&self) -> &str {
        "Parse text as PCF (Pkl Configuration Format) and create a record."
    }

    fn extra_description(&self) -> &str {
        "PCF is the static subset of Pkl produced by `pkl eval -f pcf`. Objects become records, \
objects with elements become lists, durations and data sizes become durations and filesizes, \
`IntSeq` becomes a range, `Bytes` becomes binary, `Pair` becomes a record with `first` and \
`second`, and `Regex` becomes its pattern string. Non-string entry keys are converted to their PCF \
representation. Expressions such as references, interpolation or `amends` need to be evaluated by \
Pkl first."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["pkl", "configuration", "parse", "deserialize"]
    }

    fn signature(&self) -> Signature {
        Signature::build(self.name())
            .input_output_types(vec![
                (Type::String, Type::record()),
                (Type::Binary, Type::record()),
            ])
            .category(Category::Formats)
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                example: r#"'name = "Pkl"
port = 8080' | from pcf"#,
                description: "Convert PCF properties to a record",
                result: Some(Value::test_record(record! {
                    "name" => Value::test_string("Pkl"),
                    "port" => Value::test_int(8080),
                })),
            },
            Example {
                example: r#"'server {
  hosts {
    "a.example.com"
    "b.example.com"
  }
  timeout = 30.s
  maxBody = 5.mb
}' | from pcf"#,
                description: "Convert nested objects, listings, durations and data sizes",
                result: Some(Value::test_record(record! {
                    "server" => Value::test_record(record! {
                        "hosts" => Value::test_list(vec![
                            Value::test_string("a.example.com"),
                            Value::test_string("b.example.com"),
                        ]),
                        "timeout" => Value::test_duration(30_000_000_000),
                        "maxBody" => Value::test_filesize(5_000_000),
                    }),
                })),
            },
            Example {
                example: r#"'ports { ["http"] = 80; ["https"] = 443 }' | from pcf"#,
                description: "Convert a mapping to a record",
                result: Some(Value::test_record(record! {
                    "ports" => Value::test_record(record! {
                        "http" => Value::test_int(80),
                        "https" => Value::test_int(443),
                    }),
                })),
            },
            Example {
                example: "'' | from pcf",
                description: "An empty document is an empty record",
                result: Some(Value::test_record(Record::new())),
            },
        ]
    }

    fn run(
        &self,
        _plugin: &PcfPlugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        input: &Value,
    ) -> Result<Value, LabeledError> {
        let source = match input {
            Value::String { val, .. } => val.as_str(),
            Value::Binary { val, .. } => std::str::from_utf8(val).map_err(|err| {
                LabeledError::new("Input is not valid UTF-8")
                    .with_label(err.to_string(), input.span())
            })?,
            other => {
                return Err(ShellError::OnlySupportsThisInputType {
                    exp_input_type: "string".into(),
                    wrong_type: other.get_type().to_string(),
                    dst_span: call.head,
                    src_span: other.span(),
                }
                .into());
            }
        };
        pcf::parse(source, call.head).map_err(|err| parse_error(&err, source, input.span()))
    }
}

fn parse_error(err: &pcf::ParseError, source: &str, span: Span) -> LabeledError {
    let (line, column) = err.position(source);
    LabeledError::new("Failed to parse PCF")
        .with_label(
            format!("{} at line {line}, column {column}", err.message()),
            span,
        )
        .with_help(err.snippet(source))
}
