use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use serde_json::Value;

use crate::types::{
    BugMonitorLogCandidate, BugMonitorLogFormat, BugMonitorLogMinimumLevel, BugMonitorLogSource,
    BugMonitorMonitoredProject,
};

#[derive(Debug, Clone, Default)]
pub struct BugMonitorLogParseResult {
    pub candidates: Vec<BugMonitorLogCandidate>,
    pub next_partial_line: Option<String>,
    pub next_partial_line_offset_start: Option<u64>,
}

#[derive(Debug, Clone)]
struct ParsedLine {
    text: String,
    offset_start: u64,
    offset_end: u64,
}

#[derive(Debug, Clone, Default)]
struct ParsedSafetyContext {
    actor: Option<String>,
    model: Option<String>,
    tool_name: Option<String>,
    action: Option<String>,
    policy: Option<String>,
    approval_state: Option<String>,
    risk_category: Option<String>,
    blast_radius: Option<String>,
    external_correlation_ids: Vec<String>,
}

pub fn parse_log_candidates(
    project: &BugMonitorMonitoredProject,
    source: &BugMonitorLogSource,
    absolute_path: &Path,
    inode: Option<String>,
    offset_start: u64,
    bytes: &[u8],
    partial_line: Option<String>,
    partial_line_offset_start: Option<u64>,
) -> BugMonitorLogParseResult {
    let decoded = String::from_utf8_lossy(bytes);
    let mut combined = String::new();
    if let Some(partial) = partial_line.as_ref() {
        combined.push_str(partial);
    }
    combined.push_str(&decoded);
    let mut next_partial_line = None;
    let mut next_partial_line_offset_start = None;
    let complete_text = if combined.ends_with('\n') || combined.ends_with('\r') {
        combined
    } else if let Some((complete, partial)) = combined.rsplit_once('\n') {
        let complete_with_newline = format!("{complete}\n");
        let partial_offset = offset_start
            .saturating_add(bytes.len() as u64)
            .saturating_sub(partial.len() as u64);
        next_partial_line = Some(partial.to_string());
        next_partial_line_offset_start = Some(partial_offset);
        complete_with_newline
    } else {
        let start = partial_line_offset_start.unwrap_or(offset_start);
        next_partial_line = Some(combined);
        next_partial_line_offset_start = Some(start);
        String::new()
    };

    let base_offset = partial_line_offset_start.unwrap_or(offset_start);
    let mut cursor = base_offset;
    let lines = complete_text
        .split_inclusive('\n')
        .map(|line| {
            let clean = line.trim_end_matches(['\r', '\n']).to_string();
            let start = cursor;
            cursor = cursor.saturating_add(line.len() as u64);
            ParsedLine {
                text: clean,
                offset_start: start,
                offset_end: cursor,
            }
        })
        .collect::<Vec<_>>();

    let candidates = match source.format {
        BugMonitorLogFormat::Json => {
            parse_json_lines(project, source, absolute_path, inode, &lines)
        }
        BugMonitorLogFormat::Plaintext => {
            parse_plaintext(project, source, absolute_path, inode, &lines)
        }
        BugMonitorLogFormat::Auto => {
            let mut out = parse_json_lines(project, source, absolute_path, inode.clone(), &lines);
            out.extend(parse_plaintext(
                project,
                source,
                absolute_path,
                inode,
                &lines
                    .into_iter()
                    .filter(|line| serde_json::from_str::<Value>(&line.text).is_err())
                    .collect::<Vec<_>>(),
            ));
            out
        }
    };

    BugMonitorLogParseResult {
        candidates,
        next_partial_line,
        next_partial_line_offset_start,
    }
}

fn parse_json_lines(
    project: &BugMonitorMonitoredProject,
    source: &BugMonitorLogSource,
    absolute_path: &Path,
    inode: Option<String>,
    lines: &[ParsedLine],
) -> Vec<BugMonitorLogCandidate> {
    lines
        .iter()
        .filter_map(|line| {
            let value = serde_json::from_str::<Value>(&line.text).ok()?;
            let level = first_json_string(&value, &["level", "severity", "log.level", "lvl"])
                .unwrap_or_else(|| "info".to_string());
            if !level_allowed(&level, &source.minimum_level) {
                return None;
            }
            let message =
                first_json_string(&value, &["message", "msg", "error", "exception.message"])
                    .unwrap_or_else(|| line.text.clone());
            let event = first_json_string(
                &value,
                &["event", "event.name", "error.kind", "exception.type"],
            )
            .unwrap_or_else(|| "log.error".to_string());
            let component = first_json_string(
                &value,
                &["component", "service", "logger", "target", "module"],
            );
            let process = first_json_string(&value, &["process", "process.name", "service"]);
            let stack = first_json_string(
                &value,
                &["stack", "stacktrace", "exception.stacktrace", "error.stack"],
            );
            let mut excerpt = vec![message.clone()];
            if let Some(stack) = stack {
                excerpt.extend(stack.lines().take(20).map(ToString::to_string));
            }
            Some(candidate_from_block(
                project,
                source,
                absolute_path,
                inode.clone(),
                line.offset_start,
                line.offset_end,
                event,
                level,
                component,
                process,
                excerpt,
                safety_context_from_json(&value),
            ))
        })
        .collect()
}

fn parse_plaintext(
    project: &BugMonitorMonitoredProject,
    source: &BugMonitorLogSource,
    absolute_path: &Path,
    inode: Option<String>,
    lines: &[ParsedLine],
) -> Vec<BugMonitorLogCandidate> {
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let line = &lines[index];
        let Some(level) = detect_level(&line.text) else {
            index += 1;
            continue;
        };
        if !level_allowed(&level, &source.minimum_level) {
            index += 1;
            continue;
        }
        let start = index.saturating_sub(5);
        let mut end = index + 1;
        while end < lines.len() && end - index < 50 {
            let text = lines[end].text.trim_start();
            if end > index && is_new_log_line(text) && !is_continuation(text) {
                break;
            }
            if end > index && !is_continuation(text) && detect_level(text).is_some() {
                break;
            }
            end += 1;
        }
        let block = &lines[start..end];
        let excerpt = block.iter().map(|row| row.text.clone()).collect::<Vec<_>>();
        out.push(candidate_from_block(
            project,
            source,
            absolute_path,
            inode.clone(),
            block
                .first()
                .map(|row| row.offset_start)
                .unwrap_or(line.offset_start),
            block
                .last()
                .map(|row| row.offset_end)
                .unwrap_or(line.offset_end),
            "log.error".to_string(),
            level,
            None,
            None,
            excerpt,
            ParsedSafetyContext::default(),
        ));
        index = end.max(index + 1);
    }
    out
}

fn candidate_from_block(
    project: &BugMonitorMonitoredProject,
    source: &BugMonitorLogSource,
    absolute_path: &Path,
    inode: Option<String>,
    offset_start: u64,
    offset_end: u64,
    event: String,
    level: String,
    component: Option<String>,
    process: Option<String>,
    excerpt: Vec<String>,
    safety: ParsedSafetyContext,
) -> BugMonitorLogCandidate {
    let binding = project.source_binding(Some(source));
    let raw_excerpt_redacted = excerpt
        .iter()
        .take(200)
        .map(|line| redact_text(line))
        .collect::<Vec<_>>();
    let excerpt = raw_excerpt_redacted
        .iter()
        .take(50)
        .cloned()
        .collect::<Vec<_>>();
    let first = excerpt.first().cloned().unwrap_or_else(|| event.clone());
    let fingerprint = build_fingerprint(project, source, &event, &first, excerpt.get(1));
    BugMonitorLogCandidate {
        project_id: project.project_id.clone(),
        source_id: source.source_id.clone(),
        source_kind: binding.source_kind.clone(),
        repo: project.repo.clone(),
        workspace_root: project.workspace_root.clone(),
        path: absolute_path.display().to_string(),
        offset_start,
        offset_end,
        inode,
        title: format!("{} reported {}", project.name, summarize_title(&first)),
        detail: format!(
            "Detected {} log candidate in {} at byte offsets {}-{}.",
            level,
            absolute_path.display(),
            offset_start,
            offset_end
        ),
        source: format!("bug_monitor.log.{}", source.source_id),
        process,
        component,
        event,
        level,
        excerpt,
        raw_excerpt_redacted,
        fingerprint,
        confidence: "high".to_string(),
        risk_level: "medium".to_string(),
        actor: safety.actor,
        model: safety.model,
        tool_name: safety.tool_name,
        action: safety.action,
        policy: safety.policy,
        approval_state: safety.approval_state,
        risk_category: safety.risk_category,
        blast_radius: safety.blast_radius,
        external_correlation_ids: safety.external_correlation_ids,
        expected_destination: "bug_monitor_issue_draft".to_string(),
        evidence_refs: vec![format!(
            "tandem://bug-monitor/{}/logs/{}#offset={}-{}",
            project.project_id, source.source_id, offset_start, offset_end
        )],
        timestamp_ms: Some(crate::now_ms()),
        route_tags: binding.default_route_tags,
        allowed_destination_ids: binding.allowed_destination_ids,
        default_destination_ids: binding.default_destination_ids,
        tenant_id: binding.tenant_id,
        workspace_id: binding.workspace_id,
        event_schema_version: binding.event_schema_version,
        source_approval_policy: Some(binding.approval_policy),
        redaction_profile: binding.redaction_profile,
        retention_profile: binding.retention_profile,
    }
}

fn safety_context_from_json(value: &Value) -> ParsedSafetyContext {
    let mut external_correlation_ids = first_json_string_list(
        value,
        &[
            "external_correlation_ids",
            "externalCorrelationIds",
            "external_ids",
            "externalIds",
            "external.correlation_ids",
            "external.correlationIds",
        ],
        20,
        160,
    );
    if let Some(external_correlation_id) = first_json_safety_string(
        value,
        &[
            "external_correlation_id",
            "externalCorrelationId",
            "external_id",
            "externalId",
            "external.correlation_id",
        ],
        160,
    ) {
        if !external_correlation_ids
            .iter()
            .any(|row| row == &external_correlation_id)
        {
            external_correlation_ids.push(external_correlation_id);
        }
    }

    ParsedSafetyContext {
        actor: first_json_safety_string(
            value,
            &[
                "actor",
                "actor_id",
                "actorID",
                "principal",
                "principal_id",
                "agent",
                "agent_id",
                "agentID",
                "user_id",
                "userID",
            ],
            160,
        ),
        model: first_json_safety_string(
            value,
            &["model", "model_id", "modelID", "model.name", "llm.model"],
            160,
        ),
        tool_name: first_json_safety_string(
            value,
            &[
                "tool",
                "tool_name",
                "toolName",
                "mcp_tool",
                "mcpTool",
                "action.tool",
            ],
            160,
        ),
        action: first_json_safety_string(
            value,
            &["action", "operation", "tool_action", "toolAction", "intent"],
            160,
        ),
        policy: first_json_safety_string(
            value,
            &[
                "policy",
                "policy_id",
                "policyID",
                "policy.name",
                "approval_policy",
                "approvalPolicy",
                "approval.policy",
            ],
            160,
        ),
        approval_state: first_json_safety_string(
            value,
            &[
                "approval_state",
                "approvalState",
                "approval_status",
                "approvalStatus",
                "approval.status",
            ],
            120,
        ),
        risk_category: first_json_safety_string(
            value,
            &[
                "risk_category",
                "riskCategory",
                "risk_type",
                "riskType",
                "risk.category",
                "category",
            ],
            120,
        ),
        blast_radius: first_json_safety_string(
            value,
            &[
                "blast_radius",
                "blastRadius",
                "blast.radius",
                "impact",
                "scope",
            ],
            240,
        ),
        external_correlation_ids,
    }
}

fn first_json_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        let mut current = value;
        for part in key.split('.') {
            current = current.get(part)?;
        }
        current
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
    })
}

fn first_json_safety_string(value: &Value, keys: &[&str], max_len: usize) -> Option<String> {
    first_json_string(value, keys)
        .map(|value| truncate_redacted_text(&redact_text(&value), max_len))
        .filter(|value| !value.trim().is_empty())
}

fn first_json_string_list(
    value: &Value,
    keys: &[&str],
    max_items: usize,
    max_len: usize,
) -> Vec<String> {
    let Some(row) = keys.iter().find_map(|key| {
        let mut current = value;
        for part in key.split('.') {
            current = current.get(part)?;
        }
        Some(current)
    }) else {
        return Vec::new();
    };
    let values = match row {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(ToString::to_string))
            .collect::<Vec<_>>(),
        Value::String(text) => text
            .split(',')
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for value in values {
        let value = truncate_redacted_text(&redact_text(&value), max_len);
        if value.trim().is_empty() || out.iter().any(|row| row == &value) {
            continue;
        }
        out.push(value);
        if out.len() >= max_items {
            break;
        }
    }
    out
}

fn truncate_redacted_text(value: &str, max_len: usize) -> String {
    crate::truncate_text(value, max_len)
}

fn detect_level(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("fatal")
        || lower.contains("panic")
        || lower.contains("critical")
        || lower.contains("error")
        || lower.contains("exception")
        || lower.contains("traceback")
        || lower.contains("typeerror")
        || lower.contains("referenceerror")
        || lower.contains("syntaxerror")
    {
        Some("error".to_string())
    } else if lower.contains("warn") || lower.contains("[warn]") {
        Some("warn".to_string())
    } else {
        None
    }
}

fn level_allowed(level: &str, minimum: &BugMonitorLogMinimumLevel) -> bool {
    let level = level.to_ascii_lowercase();
    match minimum {
        BugMonitorLogMinimumLevel::Error => {
            matches!(level.as_str(), "error" | "fatal" | "panic" | "critical")
                || level.contains("error")
                || level.contains("fatal")
                || level.contains("panic")
        }
        BugMonitorLogMinimumLevel::Warn => {
            level_allowed(&level, &BugMonitorLogMinimumLevel::Error) || level.contains("warn")
        }
    }
}

fn is_continuation(text: &str) -> bool {
    text.is_empty()
        || text.starts_with("at ")
        || text.starts_with("File \"")
        || text.starts_with("Caused by")
        || text.starts_with("Traceback")
        || text.starts_with("stack backtrace")
        || text.starts_with(char::is_whitespace)
}

fn is_new_log_line(text: &str) -> bool {
    text.len() >= 10
        && text.as_bytes().get(4) == Some(&b'-')
        && text.as_bytes().get(7) == Some(&b'-')
}

fn redact_text(text: &str) -> String {
    static KEY_VALUE_SECRET_RE: OnceLock<Regex> = OnceLock::new();
    static BEARER_SECRET_RE: OnceLock<Regex> = OnceLock::new();
    let key_value = KEY_VALUE_SECRET_RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(api[_-]?key|token|password|secret|credential|authorization)\s*[:=]\s*\S+",
        )
        .expect("valid bug monitor secret regex")
    });
    let bearer = BEARER_SECRET_RE.get_or_init(|| {
        Regex::new(r"(?i)(authorization\s*:\s*)?bearer\s+\S+")
            .expect("valid bug monitor bearer regex")
    });

    let out = key_value.replace_all(text, |caps: &regex::Captures<'_>| {
        format!("{}=[redacted]", &caps[1])
    });
    bearer.replace_all(&out, "Bearer [redacted]").to_string()
}

fn build_fingerprint(
    project: &BugMonitorMonitoredProject,
    source: &BugMonitorLogSource,
    event: &str,
    message: &str,
    stack_hint: Option<&String>,
) -> String {
    let normalized = normalize_dynamic(message);
    let stack = stack_hint.map(|s| normalize_dynamic(s)).unwrap_or_default();
    format!(
        "{}:{}:{}:{}",
        project.project_id,
        source.source_id,
        event,
        &crate::sha256_hex(&[&normalized, &stack])[..16]
    )
}

fn normalize_dynamic(value: &str) -> String {
    value
        .split_whitespace()
        .map(|token| {
            if token.chars().any(|ch| ch.is_ascii_digit()) || token.len() > 40 {
                "<var>"
            } else {
                token
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn summarize_title(value: &str) -> String {
    let clean = value.trim();
    if clean.len() > 100 {
        format!("{}...", &clean[..100])
    } else {
        clean.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> BugMonitorMonitoredProject {
        BugMonitorMonitoredProject {
            project_id: "customer-api".to_string(),
            name: "Customer API".to_string(),
            repo: "owner/customer-api".to_string(),
            workspace_root: "/tmp/customer-api".to_string(),
            ..BugMonitorMonitoredProject::default()
        }
    }

    fn source(format: BugMonitorLogFormat) -> BugMonitorLogSource {
        BugMonitorLogSource {
            source_id: "api-log".to_string(),
            path: "logs/app.log".to_string(),
            format,
            ..BugMonitorLogSource::default()
        }
    }

    #[test]
    fn json_error_line_becomes_candidate() {
        let line = br#"{"level":"error","message":"upload failed","exception":{"type":"TypeError","stacktrace":"TypeError: bad\n at normalize src/uploads.ts:42:1"},"service":"api"}"#;
        let parsed = parse_log_candidates(
            &project(),
            &source(BugMonitorLogFormat::Json),
            Path::new("/tmp/customer-api/logs/app.log"),
            Some("1".to_string()),
            10,
            &[line.as_slice(), b"\n"].concat(),
            None,
            None,
        );
        assert_eq!(parsed.candidates.len(), 1);
        let candidate = &parsed.candidates[0];
        assert_eq!(candidate.repo, "owner/customer-api");
        assert_eq!(candidate.level, "error");
        assert!(candidate
            .excerpt
            .iter()
            .any(|line| line.contains("upload failed")));
        assert!(candidate.fingerprint.contains("customer-api:api-log"));
    }

    #[test]
    fn json_error_line_extracts_safety_context_with_redaction() {
        let line = br#"{"level":"error","message":"blocked egress","event":"agent.risk","actor_id":"agent-release","model":"gpt-5","tool_name":"slack.post_message","action":"send_message","approval_state":"denied","risk_category":"data_exfiltration","blast_radius":"customer channel","external_correlation_ids":["case-123","token=SECRET_TOKEN_123"]}"#;
        let parsed = parse_log_candidates(
            &project(),
            &source(BugMonitorLogFormat::Json),
            Path::new("/tmp/customer-api/logs/app.log"),
            Some("1".to_string()),
            10,
            &[line.as_slice(), b"\n"].concat(),
            None,
            None,
        );

        assert_eq!(parsed.candidates.len(), 1);
        let candidate = &parsed.candidates[0];
        assert_eq!(candidate.actor.as_deref(), Some("agent-release"));
        assert_eq!(candidate.model.as_deref(), Some("gpt-5"));
        assert_eq!(candidate.tool_name.as_deref(), Some("slack.post_message"));
        assert_eq!(
            candidate.risk_category.as_deref(),
            Some("data_exfiltration")
        );
        assert_eq!(candidate.approval_state.as_deref(), Some("denied"));
        assert_eq!(candidate.blast_radius.as_deref(), Some("customer channel"));
        assert_eq!(
            candidate.external_correlation_ids,
            vec!["case-123".to_string(), "token=[redacted]".to_string()]
        );
    }

    #[test]
    fn json_safety_context_truncates_multibyte_values_safely() {
        let line = serde_json::json!({
            "level": "error",
            "message": "blocked egress",
            "actor_id": "界".repeat(80),
        })
        .to_string();
        let parsed = parse_log_candidates(
            &project(),
            &source(BugMonitorLogFormat::Json),
            Path::new("/tmp/customer-api/logs/app.log"),
            Some("1".to_string()),
            10,
            &[line.as_bytes(), b"\n"].concat(),
            None,
            None,
        );

        assert_eq!(parsed.candidates.len(), 1);
        let actor = parsed.candidates[0].actor.as_deref().unwrap_or_default();
        assert!(actor.ends_with("...<truncated>"));
        assert!(actor.is_char_boundary(actor.len()));
    }

    #[test]
    fn plaintext_redacts_secret_and_groups_stack() {
        let raw = b"INFO booted\nERROR upload failed token=super-secret\n    at normalize src/uploads.ts:42:1\n";
        let parsed = parse_log_candidates(
            &project(),
            &source(BugMonitorLogFormat::Plaintext),
            Path::new("/tmp/customer-api/logs/app.log"),
            None,
            0,
            raw,
            None,
            None,
        );
        assert_eq!(parsed.candidates.len(), 1);
        let candidate = &parsed.candidates[0];
        assert!(candidate
            .raw_excerpt_redacted
            .iter()
            .any(|line| line.contains("token=[redacted]")));
        assert!(candidate
            .excerpt
            .iter()
            .any(|line| line.contains("normalize")));
    }

    #[test]
    fn partial_line_tracks_start_offset() {
        let parsed = parse_log_candidates(
            &project(),
            &source(BugMonitorLogFormat::Plaintext),
            Path::new("/tmp/customer-api/logs/app.log"),
            None,
            100,
            b"ERROR half",
            None,
            None,
        );
        assert!(parsed.candidates.is_empty());
        assert_eq!(parsed.next_partial_line.as_deref(), Some("ERROR half"));
        assert_eq!(parsed.next_partial_line_offset_start, Some(100));
    }
}
