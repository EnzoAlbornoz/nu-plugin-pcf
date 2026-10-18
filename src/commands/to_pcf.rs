use nu_plugin::{EngineInterface, EvaluatedCall, SimplePluginCommand};
use nu_protocol::{
    Category, Example, LabeledError, ShellError, Signature, SyntaxShape, Type, Value,
};

use crate::{PcfPlugin, pcf};

/// `to pcf`: renders a record as PCF text.
pub struct ToPcf;

impl SimplePluginCommand for ToPcf {
    type Plugin = PcfPlugin;

    fn name(&self) -> &str {
        "to pcf"
    }

    fn description(&self) -> &str {
        "Convert a record to PCF (Pkl Configuration Format) text."
    }

    fn extra_description(&self) -> &str {
        "The output is valid Pkl and uses the same layout as `pkl eval -f pcf`. Records become \
objects, lists become listings, durations and filesizes keep their units, ranges become `IntSeq`, \
binary becomes `Bytes` and dates become RFC 3339 strings. Field names that are not valid Pkl \
identifiers are quoted with backticks."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["pkl", "configuration", "serialize", "render"]
    }

    fn signature(&self) -> Signature {
        Signature::build(self.name())
            .input_output_types(vec![(Type::record(), Type::String)])
            .named(
                "indent",
                SyntaxShape::Int,
                "Number of spaces used for each level of indentation (default: 2)",
                Some('i'),
            )
            .named(
                "tabs",
                SyntaxShape::Int,
                "Number of tabs used for each level of indentation",
                Some('t'),
            )
            .switch(
                "omit-null-properties",
                "Leave out fields whose value is null",
                Some('n'),
            )
            .category(Category::Formats)
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                example: "{name: Pkl, port: 8080} | to pcf",
                description: "Convert a record to PCF",
                result: Some(Value::test_string("name = \"Pkl\"\nport = 8080\n")),
            },
            Example {
                example: "{server: {hosts: [a b], timeout: 30sec}} | to pcf",
                description: "Nested records become objects and lists become listings",
                result: Some(Value::test_string(
                    "server {\n  hosts {\n    \"a\"\n    \"b\"\n  }\n  timeout = 30.s\n}\n",
                )),
            },
            Example {
                example: "{server: {port: 80}} | to pcf --indent 4",
                description: "Indent with four spaces",
                result: Some(Value::test_string("server {\n    port = 80\n}\n")),
            },
            Example {
                example: "{name: Pkl, comment: null} | to pcf --omit-null-properties",
                description: "Leave out null fields",
                result: Some(Value::test_string("name = \"Pkl\"\n")),
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
        let options = pcf::WriteOptions {
            indent: indent(call)?,
            omit_null_properties: call.has_flag("omit-null-properties")?,
        };
        let text = pcf::write(input, &options).map_err(|err| {
            let error = LabeledError::new("Failed to convert to PCF")
                .with_label(err.message().to_owned(), err.span());
            match err.help() {
                Some(help) => error.with_help(help.to_owned()),
                None => error,
            }
        })?;
        Ok(Value::string(text, call.head))
    }
}

fn indent(call: &EvaluatedCall) -> Result<String, LabeledError> {
    let flag = |name: &str| -> Result<Option<(usize, nu_protocol::Span)>, LabeledError> {
        let Some(count) = call.get_flag::<i64>(name)? else {
            return Ok(None);
        };
        let span = call.get_flag_span(name).unwrap_or(call.head);
        let count = usize::try_from(count).map_err(|_| ShellError::NeedsPositiveValue { span })?;
        Ok(Some((count, span)))
    };
    match (flag("indent")?, flag("tabs")?) {
        (Some((_, indent_span)), Some((_, tabs_span))) => Err(ShellError::IncompatibleParameters {
            left_message: "can't use `--indent`".into(),
            left_span: indent_span,
            right_message: "together with `--tabs`".into(),
            right_span: tabs_span,
        }
        .into()),
        (Some((spaces, _)), None) => Ok(" ".repeat(spaces)),
        (None, Some((tabs, _))) => Ok("\t".repeat(tabs)),
        (None, None) => Ok(pcf::WriteOptions::default().indent),
    }
}
