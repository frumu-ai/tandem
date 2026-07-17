// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

/// Returns `true` when a node should be skipped entirely because an upstream
/// node that is marked as a triage gate found no work.
///
/// The triage node signals this by having `metadata.triage_gate == true` in
/// the automation spec, and outputting `has_work:false` either directly in
/// `content`, in the structured artifact handoff under
/// `content.structured_handoff`, or in structured JSON embedded in the
/// captured assistant text.
/// When skipped, downstream nodes are also unconditionally skipped via the
/// same check (`should_skip_due_to_triage_gate` is called for every pending
/// node each loop iteration after the triage output lands).
fn triage_value_has_work(value: &serde_json::Value) -> Option<bool> {
    value
        .get("has_work")
        .and_then(serde_json::Value::as_bool)
        .or_else(|| {
            value
                .pointer("/structured_handoff/has_work")
                .and_then(serde_json::Value::as_bool)
        })
        .or_else(|| {
            value
                .pointer("/content/has_work")
                .and_then(serde_json::Value::as_bool)
        })
        .or_else(|| {
            value
                .pointer("/content/structured_handoff/has_work")
                .and_then(serde_json::Value::as_bool)
        })
        .or_else(|| {
            value
                .pointer("/content/data/has_work")
                .and_then(serde_json::Value::as_bool)
        })
}

fn triage_output_has_work(output: &serde_json::Value) -> Option<bool> {
    triage_value_has_work(output).or_else(|| {
        [
            output.pointer("/content/text"),
            output.pointer("/content/raw_assistant_text"),
            output.pointer("/content/raw_text"),
            output.get("text"),
        ]
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .find_map(|text| {
            crate::app::state::automation::extraction::extract_structured_handoff_json(text)
                .as_ref()
                .and_then(triage_value_has_work)
        })
    })
}
