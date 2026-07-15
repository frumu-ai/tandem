// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[derive(Debug, Clone, serde::Serialize)]
struct ContextRunToolManifest {
    offered: Vec<String>,
    used: Vec<String>,
    hidden_by_scope: Vec<String>,
    used_subset_offered: bool,
    used_unoffered: Vec<String>,
}

fn context_run_tool_manifest(
    events: &[ContextRunEventRecord],
    records: &[ContextRunLedgerEventView],
) -> ContextRunToolManifest {
    let mut offered = BTreeSet::<String>::new();
    let mut hidden_by_scope = BTreeSet::<String>::new();
    let mut used = BTreeSet::<String>::new();

    for event in events {
        if event.event_type != "tool_routing_decision" {
            continue;
        }
        for tool in string_array_field(&event.payload, &["offeredTools", "offered_tools"]) {
            offered.insert(tool);
        }
        for tool in string_array_field(&event.payload, &["hiddenByScope", "hidden_by_scope"]) {
            hidden_by_scope.insert(tool);
        }
    }

    for row in records {
        if matches!(row.record.status, ToolEffectLedgerStatus::Blocked) {
            continue;
        }
        used.insert(row.record.tool.clone());
    }

    if offered.is_empty() {
        offered.extend(used.iter().cloned());
    }
    hidden_by_scope.retain(|tool| !offered.contains(tool));

    let used_unoffered = used
        .iter()
        .filter(|tool| !offered.contains(*tool))
        .cloned()
        .collect::<Vec<_>>();

    ContextRunToolManifest {
        offered: offered.into_iter().collect(),
        used: used.into_iter().collect(),
        hidden_by_scope: hidden_by_scope.into_iter().collect(),
        used_subset_offered: used_unoffered.is_empty(),
        used_unoffered,
    }
}

fn string_array_field(value: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_array))
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}
