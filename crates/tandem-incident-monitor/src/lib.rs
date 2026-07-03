#![allow(clippy::too_many_arguments)]

pub mod comment_summary;
pub mod error_provenance;
pub mod github;
pub mod governance_metrics;
pub mod log_artifacts;
pub mod log_parser;
pub mod reassessment;
pub mod scenarios;
pub mod types;

pub use governance_metrics::IncidentMonitorGovernanceThresholds;
pub use reassessment::{
    IncidentMonitorReassessmentConfig, ReassessmentComparison, ReassessmentFinding,
    ReassessmentRecord, ReassessmentScheduleStatus, ReassessmentTrigger,
};

pub use scenarios::{
    default_scenario_pack, IncidentMonitorScenario, IncidentMonitorScenarioExpectation,
    IncidentMonitorScenarioInput, IncidentMonitorScenarioPack, DEFAULT_SCENARIO_PACK_JSON,
};
pub use types::*;

pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn sha256_hex(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0u8]);
    }
    format!("{:x}", hasher.finalize())
}

pub fn truncate_text(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        return input.to_string();
    }
    let mut end = 0usize;
    for (idx, ch) in input.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_len {
            break;
        }
        end = next;
    }
    let mut out = input[..end].to_string();
    out.push_str("...<truncated>");
    out
}
