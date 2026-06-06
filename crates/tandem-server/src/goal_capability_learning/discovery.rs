//! Capability discovery: match goals to available capabilities and compose solutions.

use serde_json::json;
use tandem_types::{
    AvailableCapability, CapabilityDiscoveryReport, CapabilityRequirement, CompositionPath,
    GoalSpec,
};

/// Hardcoded capabilities for MVP discovery.
fn all_capabilities() -> Vec<AvailableCapability> {
    vec![
        AvailableCapability {
            capability_id: "file_read".to_string(),
            tool_name: "FileRead".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to read" }
                },
                "required": ["path"]
            }),
            output_schema: json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "File contents" },
                    "size_bytes": { "type": "integer" }
                },
                "required": ["content"]
            }),
            tags: vec!["file_io".to_string(), "read".to_string()],
        },
        AvailableCapability {
            capability_id: "csv_parse".to_string(),
            tool_name: "CSVParse".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "CSV content" },
                    "delimiter": { "type": "string", "default": "," }
                },
                "required": ["content"]
            }),
            output_schema: json!({
                "type": "object",
                "properties": {
                    "records": {
                        "type": "array",
                        "items": { "type": "object" }
                    },
                    "record_count": { "type": "integer" }
                },
                "required": ["records"]
            }),
            tags: vec![
                "data_transform".to_string(),
                "parse".to_string(),
                "csv".to_string(),
            ],
        },
    ]
}

/// Discover capabilities for a goal and generate composition paths.
pub fn discover_capabilities_for_goal(goal: &GoalSpec) -> CapabilityDiscoveryReport {
    let all_caps = all_capabilities();
    let text = format!(
        "{} {} {}",
        goal.title.to_lowercase(),
        goal.description.to_lowercase(),
        goal.expected_output_format.to_lowercase()
    );

    let mut discovered = Vec::new();
    let mut required_ids = Vec::new();
    let mut requirements = Vec::new();

    // Keyword matching for file_read.
    if text.contains("read") || text.contains("file") || text.contains("open") {
        requirements.push(CapabilityRequirement {
            requirement_id: "read_source".to_string(),
            description: "Read the source content from a file".to_string(),
            required_tags: vec!["file_io".to_string(), "read".to_string()],
            mandatory: true,
        });
        if let Some(cap) = all_caps.iter().find(|c| c.capability_id == "file_read") {
            discovered.push(cap.clone());
            required_ids.push("file_read".to_string());
        }
    }

    // Keyword matching for csv_parse.
    if text.contains("csv") || text.contains("parse") {
        requirements.push(CapabilityRequirement {
            requirement_id: "parse_csv".to_string(),
            description: "Parse CSV content into structured records".to_string(),
            required_tags: vec!["parse".to_string(), "csv".to_string()],
            mandatory: true,
        });
        if let Some(cap) = all_caps.iter().find(|c| c.capability_id == "csv_parse") {
            discovered.push(cap.clone());
            required_ids.push("csv_parse".to_string());
        }
    }

    let mut candidates = Vec::new();

    // Standard CSV pipeline.
    if required_ids.contains(&"file_read".to_string())
        && required_ids.contains(&"csv_parse".to_string())
    {
        candidates.push(CompositionPath {
            sequence: vec!["file_read".to_string(), "csv_parse".to_string()],
            compatibility_score: 0.95,
            reasoning: "Standard pipeline: read file, parse as CSV".to_string(),
        });
    }

    let overall_confidence = if !candidates.is_empty() { 0.9 } else { 0.3 };

    let reasoning = if !candidates.is_empty() {
        format!(
            "Found {} composition path(s) for {} required capability(ies)",
            candidates.len(),
            required_ids.len()
        )
    } else {
        "No composition paths found for this goal".to_string()
    };

    CapabilityDiscoveryReport {
        goal_id: goal.goal_id.clone(),
        requirements,
        discovered_capabilities: discovered,
        composition_candidates: candidates,
        gaps: vec![],
        overall_confidence_score: overall_confidence,
        reasoning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_finds_csv_capabilities() {
        let goal = GoalSpec {
            goal_id: "demo".to_string(),
            title: "Read and parse CSV file".to_string(),
            description: "Parse CSV data".to_string(),
            input_parameters: vec![],
            expected_output_format: "Records".to_string(),
            constraints: vec![],
        };

        let report = discover_capabilities_for_goal(&goal);
        assert_eq!(report.discovered_capabilities.len(), 2);
        assert!(!report.composition_candidates.is_empty());
    }

    #[test]
    fn discovery_generates_correct_path() {
        let goal = GoalSpec {
            goal_id: "demo".to_string(),
            title: "Read and parse CSV".to_string(),
            description: "Read CSV file".to_string(),
            input_parameters: vec![],
            expected_output_format: "JSON records".to_string(),
            constraints: vec![],
        };

        let report = discover_capabilities_for_goal(&goal);
        let primary = report.primary_recommendation();
        assert!(primary.is_some());
        assert_eq!(primary.unwrap().sequence, vec!["file_read", "csv_parse"]);
    }
}
