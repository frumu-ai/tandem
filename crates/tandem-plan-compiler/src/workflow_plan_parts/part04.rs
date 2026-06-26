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
    let writer_intent = [
        "write to",
        "save to",
        "insert",
        "upsert",
        "update",
        "create page",
        "send ",
        "post to",
        "publish",
        "draft email",
        "outreach",
    ]
    .iter()
    .any(|needle| text.contains(needle));
    collection_intent && !writer_intent
}
