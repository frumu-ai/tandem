// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CapabilitySetInput {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub optional: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CapabilitySummaryInput {
    #[serde(default)]
    pub agent: CapabilitySetInput,
    #[serde(default)]
    pub tasks: Vec<CapabilitySetInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitySummaryOutput {
    pub agent: CapabilitySetInput,
    pub automation: CapabilitySetInput,
    pub totals: CapabilityTotals,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityTotals {
    pub required_count: usize,
    pub optional_count: usize,
    pub task_count: usize,
}

pub fn summarize(input: CapabilitySummaryInput) -> CapabilitySummaryOutput {
    let agent = normalize(input.agent);
    let task_count = input.tasks.len();
    let mut automation_required = BTreeSet::<String>::new();
    let mut automation_optional = BTreeSet::<String>::new();
    for task in input.tasks {
        let normalized = normalize(task);
        for cap in normalized.required {
            automation_required.insert(cap);
        }
        for cap in normalized.optional {
            if !automation_required.contains(&cap) {
                automation_optional.insert(cap);
            }
        }
    }
    // Agent capabilities are also required for automation when tasks bind that agent.
    for cap in &agent.required {
        automation_required.insert(cap.clone());
        automation_optional.remove(cap);
    }
    for cap in &agent.optional {
        if !automation_required.contains(cap) {
            automation_optional.insert(cap.clone());
        }
    }
    let automation = CapabilitySetInput {
        required: automation_required.iter().cloned().collect(),
        optional: automation_optional.iter().cloned().collect(),
    };
    let totals = CapabilityTotals {
        required_count: automation.required.len(),
        optional_count: automation.optional.len(),
        task_count,
    };
    CapabilitySummaryOutput {
        agent,
        automation,
        totals,
    }
}

fn normalize(input: CapabilitySetInput) -> CapabilitySetInput {
    let mut required = BTreeSet::<String>::new();
    let mut optional = BTreeSet::<String>::new();
    for cap in input.required {
        let id = cap.trim();
        if !id.is_empty() {
            required.insert(id.to_string());
        }
    }
    for cap in input.optional {
        let id = cap.trim();
        if id.is_empty() {
            continue;
        }
        if !required.contains(id) {
            optional.insert(id.to_string());
        }
    }
    CapabilitySetInput {
        required: required.into_iter().collect(),
        optional: optional.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_dominates_optional_across_agent_and_tasks() {
        let out = summarize(CapabilitySummaryInput {
            agent: CapabilitySetInput {
                required: vec!["github.create_pull_request".to_string()],
                optional: vec!["slack.post_message".to_string()],
            },
            tasks: vec![
                CapabilitySetInput {
                    required: vec!["slack.post_message".to_string()],
                    optional: vec!["github.create_pull_request".to_string()],
                },
                CapabilitySetInput {
                    required: vec![],
                    optional: vec!["jira.create_issue".to_string()],
                },
            ],
        });
        assert_eq!(
            out.automation.required,
            vec![
                "github.create_pull_request".to_string(),
                "slack.post_message".to_string()
            ]
        );
        assert_eq!(
            out.automation.optional,
            vec!["jira.create_issue".to_string()]
        );
    }
}
