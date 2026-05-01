//! Short, structured summary used as the *fallback* body for Bug
//! Monitor recurrence comments — when no LLM-produced
//! `what_happened` is available (triage timed out, hasn't run yet,
//! or the artifact is missing).
//!
//! The previous fallback dumped the verbose event payload from
//! `draft.detail`, which just repeated the parent issue body and
//! added zero new information per recurrence. See
//! https://github.com/frumu-ai/tandem/issues/46 comment chain for
//! the symptom.

use crate::app::state::truncate_text;
use crate::{BugMonitorDraftRecord, BugMonitorIncidentRecord};

pub(crate) fn build_comment_recurrence_summary(
    draft: &BugMonitorDraftRecord,
    incident: Option<&BugMonitorIncidentRecord>,
) -> String {
    let mut lines = vec![lead_in_line(draft.github_status.as_deref()).to_string()];
    if let Some(incident) = incident {
        let mut meta = Vec::new();
        if incident.occurrence_count > 1 {
            meta.push(format!(
                "**Occurrences so far:** {}",
                incident.occurrence_count
            ));
        }
        if let Some(last_seen) = incident.last_seen_at_ms {
            meta.push(format!("**Last seen:** {}", format_ms(last_seen)));
        }
        if !meta.is_empty() {
            lines.push(String::new());
            lines.extend(meta);
        }
    }
    if let Some(reason) = draft.detail.as_deref().and_then(reason_line_from_detail) {
        lines.push(String::new());
        lines.push(format!(
            "**Failure reason:** {}",
            truncate_text(reason, 400)
        ));
    }
    lines.join("\n")
}

fn lead_in_line(github_status: Option<&str>) -> &'static str {
    match github_status {
        Some(s) if s.eq_ignore_ascii_case("triage_timed_out") => {
            "Failure recurred — triage timed out for this occurrence; no new LLM analysis beyond the original issue."
        }
        Some(s) if s.eq_ignore_ascii_case("github_post_failed") => {
            "Failure recurred — the previous publish attempt failed before completing."
        }
        _ => "Failure recurred — same fingerprint as the original issue.",
    }
}

fn format_ms(ms: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms as i64)
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| ms.to_string())
}

fn reason_line_from_detail(detail: &str) -> Option<&str> {
    detail
        .lines()
        .find_map(|line| line.strip_prefix("reason: "))
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft_with_status(status: Option<&str>, detail: Option<&str>) -> BugMonitorDraftRecord {
        BugMonitorDraftRecord {
            github_status: status.map(str::to_string),
            detail: detail.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn lead_in_line_picks_triage_timed_out_message() {
        assert!(lead_in_line(Some("triage_timed_out")).contains("triage timed out"));
        assert!(lead_in_line(Some("TRIAGE_TIMED_OUT")).contains("triage timed out"));
    }

    #[test]
    fn lead_in_line_picks_post_failed_message() {
        assert!(lead_in_line(Some("github_post_failed")).contains("publish attempt failed"));
    }

    #[test]
    fn lead_in_line_defaults_to_generic_recurrence() {
        assert!(lead_in_line(None).contains("same fingerprint"));
        assert!(lead_in_line(Some("github_issue_created")).contains("same fingerprint"));
    }

    #[test]
    fn reason_line_extracts_from_event_detail() {
        let detail = "source: automation_v2\nlevel: error\nreason: automation node `X` timed out after 240000 ms\nattempt: 3";
        assert_eq!(
            reason_line_from_detail(detail),
            Some("automation node `X` timed out after 240000 ms")
        );
    }

    #[test]
    fn reason_line_returns_none_when_absent() {
        assert!(reason_line_from_detail("source: foo\nlevel: error").is_none());
    }

    #[test]
    fn summary_includes_occurrence_count_when_greater_than_one() {
        let draft = draft_with_status(Some("triage_timed_out"), Some("reason: test failure"));
        let incident = BugMonitorIncidentRecord {
            occurrence_count: 5,
            last_seen_at_ms: Some(1_700_000_000_000),
            ..Default::default()
        };
        let body = build_comment_recurrence_summary(&draft, Some(&incident));
        assert!(body.contains("triage timed out"));
        assert!(body.contains("**Occurrences so far:** 5"));
        assert!(body.contains("**Last seen:**"));
        assert!(body.contains("**Failure reason:** test failure"));
    }

    #[test]
    fn summary_omits_occurrence_count_when_only_one() {
        let draft = draft_with_status(None, None);
        let incident = BugMonitorIncidentRecord {
            occurrence_count: 1,
            ..Default::default()
        };
        let body = build_comment_recurrence_summary(&draft, Some(&incident));
        assert!(!body.contains("Occurrences so far"));
    }

    #[test]
    fn summary_handles_missing_incident_and_detail() {
        let draft = draft_with_status(None, None);
        let body = build_comment_recurrence_summary(&draft, None);
        assert!(body.contains("same fingerprint"));
        assert!(!body.contains("Failure reason"));
        assert!(!body.contains("Occurrences"));
    }
}
