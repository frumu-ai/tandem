use super::*;
use serde_json::Value;
use std::path::PathBuf;

impl App {
    pub(super) fn format_local_agent_team_bindings(team_filter: Option<&str>) -> String {
        let root = Self::agent_team_workspace_root();
        if !root.exists() {
            return "No local agent-team state found.".to_string();
        }
        let filter = team_filter.map(str::trim).filter(|s| !s.is_empty());
        let Ok(entries) = std::fs::read_dir(&root) else {
            return "Failed to read local agent-team state.".to_string();
        };
        let mut output = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(team_name) = path.file_name().and_then(|v| v.to_str()) else {
                continue;
            };
            if let Some(filter_name) = filter {
                if team_name != filter_name {
                    continue;
                }
            }
            let members_path = path.join("members.json");
            if !members_path.exists() {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&members_path) else {
                continue;
            };
            let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            let Some(items) = parsed.as_array() else {
                continue;
            };
            let mut lines = Vec::new();
            for item in items {
                let Some(name) = item.get("name").and_then(|v| v.as_str()) else {
                    continue;
                };
                let session = item
                    .get("sessionID")
                    .or_else(|| item.get("session_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("-");
                lines.push(format!("  - {} -> {}", name, session));
            }
            if !lines.is_empty() {
                output.push(format!("{}:\n{}", team_name, lines.join("\n")));
            }
        }
        if output.is_empty() {
            return "No local agent-team bindings found.".to_string();
        }
        format!("Agent-Team Bindings:\n{}", output.join("\n"))
    }

    pub(super) async fn load_agent_team_mailbox_prompt(
        team_name: &str,
        recipient: &str,
    ) -> Option<String> {
        let mailbox_path = Self::agent_team_workspace_root()
            .join(team_name)
            .join("mailboxes")
            .join(format!("{}.jsonl", recipient));
        let raw = tokio::fs::read_to_string(mailbox_path).await.ok()?;
        let line = raw
            .lines()
            .rev()
            .map(str::trim)
            .find(|line| !line.is_empty())?;
        let payload = serde_json::from_str::<Value>(line).ok()?;
        let msg_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if !matches!(msg_type, "task_prompt" | "message" | "broadcast") {
            return None;
        }
        let content = payload
            .get("content")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)?;
        let summary = payload
            .get("summary")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let from = payload
            .get("from")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("team-lead");
        let prompt = if let Some(summary) = summary {
            format!(
                "Agent-team assignment from {}.\nSummary: {}\n\n{}",
                from, summary, content
            )
        } else {
            format!("Agent-team assignment from {}.\n\n{}", from, content)
        };
        Some(prompt)
    }

    pub(super) fn agent_team_workspace_root() -> PathBuf {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".tandem")
            .join("agent-teams")
    }

    pub(super) fn member_name_matches_recipient(member_name: &str, recipient: &str) -> bool {
        if member_name.eq_ignore_ascii_case(recipient.trim()) {
            return true;
        }
        match (
            Self::normalize_recipient_agent_id(member_name),
            Self::normalize_recipient_agent_id(recipient),
        ) {
            (Some(left), Some(right)) => left == right,
            _ => false,
        }
    }

    pub(super) async fn load_agent_team_member_session_binding(
        team_name: &str,
        recipient: &str,
    ) -> Option<String> {
        let members_path = Self::agent_team_workspace_root()
            .join(team_name)
            .join("members.json");
        let raw = tokio::fs::read_to_string(members_path).await.ok()?;
        let parsed = serde_json::from_str::<Value>(&raw).ok()?;
        let entries = parsed.as_array()?;
        for entry in entries {
            let Some(name) = entry.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            if !Self::member_name_matches_recipient(name, recipient) {
                continue;
            }
            if let Some(session_id) = entry
                .get("sessionID")
                .or_else(|| entry.get("session_id"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(session_id.to_string());
            }
        }
        None
    }

    pub(super) async fn persist_agent_team_member_session_binding(
        team_name: &str,
        recipient: &str,
        session_id: &str,
    ) -> bool {
        let members_path = Self::agent_team_workspace_root()
            .join(team_name)
            .join("members.json");
        let mut entries = if members_path.exists() {
            let Ok(raw) = tokio::fs::read_to_string(&members_path).await else {
                return false;
            };
            serde_json::from_str::<Value>(&raw)
                .ok()
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut updated = false;
        for entry in &mut entries {
            let Some(name) = entry.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            if !Self::member_name_matches_recipient(name, recipient) {
                continue;
            }
            if let Some(obj) = entry.as_object_mut() {
                obj.insert(
                    "sessionID".to_string(),
                    Value::String(session_id.to_string()),
                );
                obj.insert("updatedAtMs".to_string(), Value::Number(now_ms.into()));
                updated = true;
                break;
            }
        }
        if !updated {
            let member_name = Self::normalize_recipient_agent_id(recipient)
                .unwrap_or_else(|| recipient.to_string());
            entries.push(serde_json::json!({
                "name": member_name,
                "sessionID": session_id,
                "updatedAtMs": now_ms
            }));
        }

        if let Some(parent) = members_path.parent() {
            if tokio::fs::create_dir_all(parent).await.is_err() {
                return false;
            }
        }
        tokio::fs::write(
            members_path,
            serde_json::to_vec_pretty(&Value::Array(entries)).unwrap_or_default(),
        )
        .await
        .is_ok()
    }

    pub(super) async fn persist_agent_team_session_context(
        team_name: &str,
        session_id: &str,
    ) -> bool {
        let context_path = Self::agent_team_workspace_root()
            .join("session-context")
            .join(format!("{}.json", session_id));
        if let Some(parent) = context_path.parent() {
            if tokio::fs::create_dir_all(parent).await.is_err() {
                return false;
            }
        }
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let payload = serde_json::json!({
            "team_name": team_name,
            "updatedAtMs": now_ms
        });
        tokio::fs::write(
            context_path,
            serde_json::to_vec_pretty(&payload).unwrap_or_default(),
        )
        .await
        .is_ok()
    }

    pub(super) fn normalize_recipient_agent_id(recipient: &str) -> Option<String> {
        let trimmed = recipient.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Some(rest) = trimmed
            .strip_prefix('A')
            .or_else(|| trimmed.strip_prefix('a'))
        {
            if let Ok(index) = rest.parse::<u32>() {
                if index > 0 {
                    return Some(format!("A{}", index));
                }
            }
        }
        let lowered = trimmed.to_ascii_lowercase();
        if let Some(rest) = lowered.strip_prefix("agent-") {
            if let Ok(index) = rest.parse::<u32>() {
                if index > 0 {
                    return Some(format!("A{}", index));
                }
            }
        }
        None
    }

    pub(super) fn recipient_agent_number(recipient: &str) -> Option<usize> {
        let normalized = Self::normalize_recipient_agent_id(recipient)?;
        normalized
            .strip_prefix('A')
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n > 0)
    }

    pub(super) fn resolve_agent_target_for_recipient(
        agents: &[AgentPane],
        recipient: &str,
    ) -> Option<(String, String)> {
        if let Some(agent) = agents
            .iter()
            .find(|agent| agent.agent_id.eq_ignore_ascii_case(recipient.trim()))
        {
            return Some((agent.session_id.clone(), agent.agent_id.clone()));
        }
        let normalized = Self::normalize_recipient_agent_id(recipient)?;
        let agent = agents.iter().find(|agent| agent.agent_id == normalized)?;
        Some((agent.session_id.clone(), agent.agent_id.clone()))
    }

    pub(super) fn resolve_agent_target_for_bound_session(
        agents: &[AgentPane],
        recipient: &str,
        session_id: &str,
    ) -> Option<(String, String)> {
        if let Some(normalized) = Self::normalize_recipient_agent_id(recipient) {
            if let Some(agent) = agents
                .iter()
                .find(|agent| agent.session_id == session_id && agent.agent_id == normalized)
            {
                return Some((agent.session_id.clone(), agent.agent_id.clone()));
            }
        }
        let agent = agents.iter().find(|agent| agent.session_id == session_id)?;
        Some((agent.session_id.clone(), agent.agent_id.clone()))
    }

    pub(super) fn is_agent_team_assignment_prompt(text: &str) -> bool {
        text.trim_start().starts_with("Agent-team assignment from ")
    }
}
