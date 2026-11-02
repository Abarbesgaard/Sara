//! Generated arguments for every MCP tool: the server answers each call with
//! an error or a result that says something, and keeps serving.

use proptest::prelude::*;
use proptest::test_runner::{Config, FileFailurePersistence, TestRunner};
use serde_json::{Map, Value, json};

use crate::harness::Sara;

/// Uppercase only, so a string that lands in a `verify` command and gets run
/// by the shell never names a real program. "1" is the seeded task's id.
fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        "[A-Z0-9 _-]{0,8}",
        Just("1".to_string()),
        Just(String::new())
    ]
}

fn value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        (-1e9f64..1e9).prop_map(Value::from),
        text().prop_map(Value::from),
    ];
    leaf.prop_recursive(2, 8, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..3).prop_map(Value::from),
            prop::collection::btree_map(text(), inner, 0..3)
                .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

/// A value of the property's schema type, or now and then any value.
fn typed(schema: &Value) -> BoxedStrategy<Value> {
    let types: Vec<&str> = match &schema["type"] {
        Value::String(t) => vec![t.as_str()],
        Value::Array(ts) => ts.iter().filter_map(Value::as_str).collect(),
        _ => vec![],
    };
    let matching: Vec<BoxedStrategy<Value>> = types
        .iter()
        .map(|t| match *t {
            "string" => text().prop_map(Value::from).boxed(),
            "integer" => (-2i64..50).prop_map(Value::from).boxed(),
            "number" => (-2.0f64..50.0).prop_map(Value::from).boxed(),
            "boolean" => any::<bool>().prop_map(Value::from).boxed(),
            "array" => prop::collection::vec(typed(&schema["items"]), 0..3)
                .prop_map(Value::from)
                .boxed(),
            "null" => Just(Value::Null).boxed(),
            _ => value().boxed(),
        })
        .collect();
    if matching.is_empty() {
        return value().boxed();
    }
    prop_oneof![4 => prop::strategy::Union::new(matching), 1 => value()].boxed()
}

/// A random tool and arguments: each schema property present more often than
/// not and usually of its type, plus now and then a stray key.
fn call(tools: Vec<(String, Value)>) -> impl Strategy<Value = (String, Value)> {
    prop::sample::select(tools).prop_flat_map(|(name, schema)| {
        let mut fields: Vec<(String, BoxedStrategy<Value>)> = schema["properties"]
            .as_object()
            .map(|p| p.iter().map(|(k, v)| (k.clone(), typed(v))).collect())
            .unwrap_or_default();
        fields.push(("UNKNOWN".into(), value().boxed()));
        let fields: Vec<_> = fields
            .into_iter()
            .map(|(k, v)| prop::option::weighted(0.6, v.prop_map(move |v| (k.clone(), v))))
            .collect();
        let args = fields
            .prop_map(|pairs| Value::Object(pairs.into_iter().flatten().collect::<Map<_, _>>()));
        (Just(name), args)
    })
}

fn says_something(response: &Value) -> bool {
    let error = response["error"]["message"].as_str();
    let text = response["result"]["content"][0]["text"].as_str();
    error.or(text).is_some_and(|m| !m.trim().is_empty())
}

#[test]
fn every_tool_answers_generated_arguments() {
    let s = Sara::new();
    s.run(&["add", "-y", "Seed task"]);
    let mut mcp = s.mcp();
    let project = s.project().to_string_lossy().into_owned();
    let tools = mcp.tool_schemas();

    let config = Config {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/mcp_args.txt",
        ))),
        ..Config::default()
    };
    let mcp = std::cell::RefCell::new(mcp);
    TestRunner::new(config)
        .run(&call(tools), |(name, mut args)| {
            args.as_object_mut()
                .unwrap()
                .insert("project_path".into(), json!(project));
            let response = mcp.borrow_mut().call(&name, args.clone());
            prop_assert!(
                says_something(&response),
                "{name} {args} gave no message: {response}"
            );
            Ok(())
        })
        .unwrap();
    assert!(
        !mcp.borrow_mut().tools().is_empty(),
        "the server still serves"
    );
}
