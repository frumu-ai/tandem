//! Capability discovery: match goals to available capabilities and compose solutions.

use super::fixtures;
use serde_json::json;
use tandem_types::{
    AvailableCapability, CapabilityDiscoveryReport, CapabilityGap, CompositionPath, GoalSpec,
};
use uuid::Uuid;

/// Keywords that suggest which capabilities are needed.
struct CapabilityMatcher {
    file_read_keywords: Vec<&'static str>,
    csv_parse_keywords: Vec<&'static str>,
}

impl CapabilityMatcher {
    fn new() -> Self {
        Self {
            file_read_keywords: vec!["read", "file", "open", "load", "read_file"],
            csv_parse_keywords: vec!["csv", "parse", "parse_csv", "csv_parse"],
        }
    }

    fn needs_file_read(&self, goal: &GoalSpec) -> bool {
        let text = format!(
            "{} {} {}",
            goal.title.to_lowercase(),
            goal.description.to_lowercase(),
            goal.input_parameters
                .iter()
                .map(|p| p.name.to_lowercase())
                .collect::<Vec<_>>()
                .join(" ")
        );

        self.file_read_keywords.iter().any(|kw| text.contains(kw))
    }

    fn needs_csv_parse(&self, goal: &GoalSpec) -> bool {
        let text = format!(
            "{} {} {}",
            goal.title.to_lowercase(),
            goal.description.to_lowercase(),
            goal.expected_output_format.to_lowercase()
        );

        self.csv_parse_keywords.iter().any(|kw| text.contains(kw))
    }
}

/// Discover capabilities for a goal and generate composition paths.
pub fn discover_capabilities_for_goal(goal: &GoalSpec) -> CapabilityDiscoveryReport {
    let all_caps = fixtures::all_capabilities();
    let matcher = CapabilityMatcher::new();

    let mut discovered = Vec::new();
    let mut required_ids = Vec::new();

    if matcher.needs_file_read(goal) {
        if let Some(cap) = all_caps.iter().find(|c| c.capability_id == "file_read") {
            discovered.push(cap.clone());
            required_ids.push("file_read".to_string());
        }
    }

    if matcher.needs_csv_parse(goal) {
        if let Some(cap) = all_caps.iter().find(|c| c.capability_id == "csv_parse") {
            discovered.push(cap.clone());
            required_ids.push("csv_parse".to_string());
        }
    }

    // Generate composition paths based on required capabilities.
    let mut candidates = Vec::new();
    let gaps = Vec::new();

    // Standard CSV read-parse pipeline.
    if required_ids.contains(&"file_read".to_string())
        && required_ids.contains(&"csv_parse".to_string())
    {
        candidates.push(CompositionPath {
            sequence: vec!["file_read".to_string(), "csv_parse".to_string()],
            compatibility_score: 0.95,
            reasoning: "Standard pipeline: read file content, parse as CSV".to_string(),
        });
    }

    // If only one required capability, it's the path.
    if required_ids.len() == 1 {
        candidates.push(CompositionPath {
            sequence: required_ids.clone(),
            compatibility_score: 0.85,
            reasoning: format!("Single capability: {}", required_ids[0].replace('_', " ")),
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
        discovered_capabilities: discovered,
        composition_candidates: candidates,
        gaps,
        overall_confidence_score: overall_confidence,
        reasoning,
    }
}

/// Generate a discovery decision ID for audit trails.
pub fn generate_discovery_decision_id() -> String {
    format!(
        "gcl_{}",
        Uuid::new_v4().to_string().replace('-', "")[..12].to_string()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn csv_demo_goal() -> GoalSpec {
        GoalSpec {
            goal_id: "demo_csv_parse".to_string(),
            title: "Read and parse CSV file".to_string(),
            description: "Given a CSV file path, read its contents and parse into records"
                .to_string(),
            input_parameters: vec![],
            expected_output_format: "Array of CSV records as JSON objects".to_string(),
            constraints: vec![],
        }
    }

    #[test]
    fn discovery_finds_file_read_and_csv_parse_for_csv_goal() {
        let goal = csv_demo_goal();
        let report = discover_capabilities_for_goal(&goal);

        assert_eq!(report.goal_id, "demo_csv_parse");
        assert_eq!(report.discovered_capabilities.len(), 2);
        assert!(report
            .discovered_capabilities
            .iter()
            .any(|c| c.capability_id == "file_read"));
        assert!(report
            .discovered_capabilities
            .iter()
            .any(|c| c.capability_id == "csv_parse"));
    }

    #[test]
    fn discovery_generates_correct_composition_path() {
        let goal = csv_demo_goal();
        let report = discover_capabilities_for_goal(&goal);

        assert!(!report.composition_candidates.is_empty());

        let primary = report.primary_recommendation();
        assert!(primary.is_some());

        let path = primary.unwrap();
        assert_eq!(path.sequence, vec!["file_read", "csv_parse"]);
        assert_eq!(path.compatibility_score, 0.95);
    }

    #[test]
    fn discovery_sets_high_confidence_when_paths_found() {
        let goal = csv_demo_goal();
        let report = discover_capabilities_for_goal(&goal);

        assert!(report.overall_confidence_score >= 0.9);
    }

    #[test]
    fn discovery_handles_unrecognized_goal() {
        let goal = GoalSpec {
            goal_id: "unknown".to_string(),
            title: "Unknown operation".to_string(),
            description: "Something we don't recognize".to_string(),
            input_parameters: vec![],
            expected_output_format: "Unknown".to_string(),
            constraints: vec![],
        };

        let report = discover_capabilities_for_goal(&goal);

        assert!(report.composition_candidates.is_empty());
        assert!(report.overall_confidence_score < 0.5);
    }

    #[test]
    fn discovery_generates_valid_decision_id() {
        let id = generate_discovery_decision_id();
        assert!(id.starts_with("gcl_"));
        assert!(id.len() > 10); // "gcl_" + uuid variant
    }
}
