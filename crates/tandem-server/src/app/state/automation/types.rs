// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[derive(Clone, Copy)]
pub(crate) struct SplitResearchTemplateConfig {
    pub template_id: &'static str,
    pub final_node_id: &'static str,
    pub final_agent_id: &'static str,
    pub discover_node_id: &'static str,
    pub discover_agent_id: &'static str,
    pub discover_title: &'static str,
    pub discover_objective: &'static str,
    pub discover_display_name: &'static str,
    pub local_node_id: &'static str,
    pub local_agent_id: &'static str,
    pub local_title: &'static str,
    pub local_objective: &'static str,
    pub local_display_name: &'static str,
    pub external_node_id: &'static str,
    pub external_agent_id: &'static str,
    pub external_title: &'static str,
    pub external_objective: &'static str,
    pub external_display_name: &'static str,
    pub final_title: &'static str,
    pub final_objective: &'static str,
}

#[derive(Clone, Debug)]
pub(crate) struct AutomationVerificationStep {
    pub kind: String,
    pub command: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ArtifactCandidateAssessment {
    pub source: String,
    pub text: String,
    pub length: usize,
    pub score: i64,
    pub substantive: bool,
    pub placeholder_like: bool,
    pub heading_count: usize,
    pub list_count: usize,
    pub paragraph_count: usize,
    pub required_section_count: usize,
    pub files_reviewed_present: bool,
    pub reviewed_paths: Vec<String>,
    pub reviewed_paths_backed_by_read: Vec<String>,
    pub unreviewed_relevant_paths: Vec<String>,
    pub citation_count: usize,
    pub web_sources_reviewed_present: bool,
    pub evidence_anchor_count: usize,
}
