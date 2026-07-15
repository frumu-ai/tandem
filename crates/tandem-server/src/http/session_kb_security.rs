// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub(super) fn suspicious_kb_retrieval_query_reason(query: &str) -> Option<&'static str> {
    let normalized = query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    let blocked_contains = [
        ("dump", "broad export"),
        ("everything", "broad export"),
        ("entire knowledgebase", "broad export"),
        ("entire knowledge base", "broad export"),
        ("all knowledge", "broad export"),
        ("all documents", "broad export"),
        ("all records", "broad export"),
        ("full database", "broad export"),
        ("bulk", "broad export"),
    ];
    for (needle, reason) in blocked_contains {
        if normalized.contains(needle) {
            return Some(reason);
        }
    }
    let export_patterns = [
        "export all",
        "export everything",
        "export entire",
        "export the entire",
        "export full",
        "export the full",
        "bulk export",
    ];
    if export_patterns
        .iter()
        .any(|pattern| normalized.contains(pattern))
    {
        return Some("broad export");
    }
    let blocked_prefixes = [
        "list all",
        "show all",
        "give me all",
        "print all",
        "return all",
    ];
    if blocked_prefixes
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
    {
        return Some("broad export");
    }
    None
}
