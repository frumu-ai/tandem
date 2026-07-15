// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub fn workflow_step_expects_connector_source_capture(
    step_id: &str,
    kind: &str,
    objective: &str,
) -> bool {
    let text = format!("{step_id} {kind} {objective}").to_ascii_lowercase();
    if !workflow_plan_mentions_connector_backed_sources(&text) {
        return false;
    }
    let collection_intent = [
        "collect",
        "extract",
        "search",
        "query",
        "fetch",
        "retrieve",
        "scan",
        "gather",
        "harvest",
        "find",
        "list",
        "source",
        "research",
        "lead",
        "signal",
        "candidate",
        "thread",
        "post",
        "issue",
        "ticket",
        "record",
        "dataset",
        "results",
    ]
    .iter()
    .any(|needle| text.contains(needle));
    let writer_intent = workflow_step_text_has_writer_intent(&text);
    collection_intent && !writer_intent
}

fn workflow_step_text_has_writer_intent(text: &str) -> bool {
    let phrase_intent = [
        "write to",
        "save to",
        "create page",
        "send ",
        "post to",
        "draft email",
    ]
    .iter()
    .any(|needle| text.contains(needle));
    if phrase_intent {
        return true;
    }

    let tokens = text
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    ["insert", "upsert", "update", "publish", "outreach"]
        .iter()
        .any(|needle| tokens.contains(needle))
}
