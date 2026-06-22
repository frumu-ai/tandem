use super::*;
use serde::Deserialize;
use serde_json::Value;
use tandem_tools::approval_classifier::classify;

#[derive(Debug, Deserialize)]
struct ToolInvocationFixture {
    input: String,
    expected_tool: String,
}

fn parse_fixture_invocation(input: &str) -> Vec<(String, Value)> {
    if input.trim_start().starts_with("/tool ") {
        parse_tool_invocation(input).into_iter().collect()
    } else {
        parse_tool_invocations_from_response(input)
    }
}

#[test]
fn tool_invocation_fixture_corpus_round_trips_shared_normalization() {
    let fixtures: Vec<ToolInvocationFixture> =
        serde_json::from_str(include_str!("fixtures/tool_invocation_corpus.json"))
            .expect("fixture corpus is valid JSON");
    assert!(
        fixtures.len() >= 30,
        "TAN-206 fixture corpus must keep at least 30 cases"
    );

    for fixture in fixtures {
        let parsed = parse_fixture_invocation(&fixture.input);
        assert_eq!(
            parsed.len(),
            1,
            "expected one parsed invocation for {:?}, got {parsed:?}",
            fixture.input
        );
        let expected = tandem_types::canonical_tool_name(&fixture.expected_tool);
        assert_eq!(parsed[0].0, expected, "{:?}", fixture.input);
        assert_eq!(
            classify(&fixture.expected_tool),
            classify(&parsed[0].0),
            "classifier drift for {:?}",
            fixture.input
        );
    }
}

#[test]
fn generated_invocation_text_never_panics_or_drifts_from_shared_classification() {
    let names = [
        "read",
        "default_api:read",
        "functions.shell",
        "default_api:run_command",
        "tools.write",
        "tool.edit",
        "builtin:websearch",
        "mcp.linear.list_issues",
        "functions.mcp.linear.create_issue",
        "mcp.github.list_issues",
        "tools.mcp.github.create_issue",
        "todowrite",
        "update_todos",
    ];
    let args = [
        "path=\"README.md\"",
        "{\"path\":\"Cargo.toml\"}",
        "query=\"enterprise mcp\"",
        "command=\"echo hi\"",
        "title=\"Follow up\", description=\"Check state\"",
        "task_id=2, status=\"completed\"",
    ];

    for name in names {
        let expected = tandem_types::canonical_tool_name(name);
        for arg in args {
            let samples = [
                format!("{name}({arg})"),
                format!("Tool call: {name}({arg}) after."),
                format!(
                    r#"{{"name":"{name}","args":{{"path":"README.md","command":"echo hi","query":"enterprise mcp","title":"Follow up","description":"Check state","task_id":2,"status":"completed"}}}}"#
                ),
            ];
            for sample in samples {
                let parsed = parse_tool_invocations_from_response(&sample);
                for (tool, _) in parsed {
                    assert_eq!(tool, expected, "{sample}");
                    assert_eq!(classify(name), classify(&tool), "{sample}");
                }
            }
        }
    }

    let mut seed = 0x5eed_u64;
    for _ in 0..512 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let name = random_ascii_fragment(seed);
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let arg = random_ascii_fragment(seed);
        let sample = format!("{name}({arg})");
        for (tool, _) in parse_tool_invocations_from_response(&sample) {
            let canonical = tandem_types::canonical_tool_name(&tool);
            assert_eq!(tool, canonical, "{sample}");
            assert_eq!(classify(&tool), classify(&canonical), "{sample}");
        }
    }
}

#[test]
fn function_style_parser_ignores_prose_and_fenced_code() {
    let prose = r#"
The helper read(path: &str) should stay in the final answer.

```rust
fn read(path: &str) -> String {
    path.to_string()
}
```
"#;
    assert!(parse_tool_invocations_from_response(prose).is_empty());

    let explicit = r#"Tool call: read(path="README.md")"#;
    let parsed = parse_tool_invocation_from_response(explicit).expect("explicit tool call");
    assert_eq!(parsed.0, "read");
}

fn random_ascii_fragment(mut seed: u64) -> String {
    const ALPHABET: &[u8] =
        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-. :=,{}[]\"";
    let len = (seed % 48 + 1) as usize;
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        seed = seed
            .wrapping_mul(2862933555777941757)
            .wrapping_add(3037000493);
        let idx = (seed % ALPHABET.len() as u64) as usize;
        out.push(ALPHABET[idx] as char);
    }
    out
}
