use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityBinding {
    pub capability_id: String,
    pub provider: String,
    pub tool_name: String,
    #[serde(default)]
    pub tool_name_aliases: Vec<String>,
    #[serde(default)]
    pub request_transform: Option<Value>,
    #[serde(default)]
    pub response_transform: Option<Value>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityBindingsFile {
    pub schema_version: String,
    #[serde(default)]
    pub generated_at: Option<String>,
    #[serde(default)]
    pub bindings: Vec<CapabilityBinding>,
}

impl Default for CapabilityBindingsFile {
    fn default() -> Self {
        Self {
            schema_version: "v1".to_string(),
            generated_at: None,
            bindings: default_spine_bindings(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityToolAvailability {
    pub provider: String,
    pub tool_name: String,
    #[serde(default)]
    pub schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityResolveInput {
    #[serde(default)]
    pub workflow_id: Option<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub optional_capabilities: Vec<String>,
    #[serde(default)]
    pub provider_preference: Vec<String>,
    #[serde(default)]
    pub available_tools: Vec<CapabilityToolAvailability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityResolution {
    pub capability_id: String,
    pub provider: String,
    pub tool_name: String,
    pub binding_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityResolveOutput {
    #[serde(default)]
    pub resolved: Vec<CapabilityResolution>,
    #[serde(default)]
    pub missing_required: Vec<String>,
    #[serde(default)]
    pub missing_optional: Vec<String>,
    #[serde(default)]
    pub considered_bindings: usize,
}

#[derive(Clone)]
pub struct CapabilityResolver {
    bindings_path: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl CapabilityResolver {
    pub fn new(root: PathBuf) -> Self {
        Self {
            bindings_path: root.join("bindings").join("capability_bindings.json"),
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn list_bindings(&self) -> anyhow::Result<CapabilityBindingsFile> {
        self.read_bindings().await
    }

    pub async fn set_bindings(&self, file: CapabilityBindingsFile) -> anyhow::Result<()> {
        let _guard = self.lock.lock().await;
        validate_bindings(&file)?;
        if let Some(parent) = self.bindings_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let payload = serde_json::to_string_pretty(&file)?;
        tokio::fs::write(&self.bindings_path, format!("{}\n", payload)).await?;
        Ok(())
    }

    pub async fn resolve(
        &self,
        input: CapabilityResolveInput,
        discovered_tools: Vec<CapabilityToolAvailability>,
    ) -> anyhow::Result<CapabilityResolveOutput> {
        let bindings = self.read_bindings().await?;
        validate_bindings(&bindings)?;
        let preference = if input.provider_preference.is_empty() {
            vec![
                "composio".to_string(),
                "arcade".to_string(),
                "mcp".to_string(),
                "custom".to_string(),
            ]
        } else {
            input.provider_preference.clone()
        };
        let pref_rank = preference
            .iter()
            .enumerate()
            .map(|(i, provider)| (provider.to_ascii_lowercase(), i))
            .collect::<HashMap<_, _>>();
        let available = if input.available_tools.is_empty() {
            discovered_tools
        } else {
            input.available_tools.clone()
        };
        let available_set = available
            .iter()
            .map(|row| {
                (
                    row.provider.to_ascii_lowercase(),
                    canonical_tool_name(&row.tool_name),
                )
            })
            .collect::<HashSet<_>>();

        let mut all_capabilities = input.required_capabilities.clone();
        for cap in &input.optional_capabilities {
            if !all_capabilities.contains(cap) {
                all_capabilities.push(cap.clone());
            }
        }

        let mut resolved = Vec::new();
        let mut missing_required = Vec::new();
        let mut missing_optional = Vec::new();

        let by_capability = group_bindings(&bindings.bindings);
        for capability_id in all_capabilities {
            let Some(candidates) = by_capability.get(&capability_id) else {
                if input.required_capabilities.contains(&capability_id) {
                    missing_required.push(capability_id);
                } else {
                    missing_optional.push(capability_id);
                }
                continue;
            };
            let mut chosen: Option<(usize, &CapabilityBinding)> = None;
            for (idx, candidate) in candidates {
                let provider = candidate.provider.to_ascii_lowercase();
                if !binding_matches_available(candidate, &provider, &available_set) {
                    continue;
                }
                if let Some((chosen_idx, chosen_binding)) = chosen {
                    let chosen_rank = pref_rank
                        .get(&chosen_binding.provider.to_ascii_lowercase())
                        .copied()
                        .unwrap_or(usize::MAX);
                    let this_rank = pref_rank.get(&provider).copied().unwrap_or(usize::MAX);
                    if this_rank < chosen_rank || (this_rank == chosen_rank && *idx < chosen_idx) {
                        chosen = Some((*idx, candidate));
                    }
                } else {
                    chosen = Some((*idx, candidate));
                }
            }
            if let Some((binding_index, binding)) = chosen {
                resolved.push(CapabilityResolution {
                    capability_id: capability_id.clone(),
                    provider: binding.provider.clone(),
                    tool_name: binding.tool_name.clone(),
                    binding_index,
                });
            } else if input.required_capabilities.contains(&capability_id) {
                missing_required.push(capability_id);
            } else {
                missing_optional.push(capability_id);
            }
        }

        resolved.sort_by(|a, b| a.capability_id.cmp(&b.capability_id));
        missing_required.sort();
        missing_optional.sort();
        Ok(CapabilityResolveOutput {
            resolved,
            missing_required,
            missing_optional,
            considered_bindings: bindings.bindings.len(),
        })
    }

    pub async fn discover_from_runtime(
        &self,
        mcp_tools: Vec<tandem_runtime::McpRemoteTool>,
        local_tools: Vec<tandem_types::ToolSchema>,
    ) -> Vec<CapabilityToolAvailability> {
        let mut out = Vec::new();
        for tool in mcp_tools {
            out.push(CapabilityToolAvailability {
                provider: provider_from_tool_name(&tool.namespaced_name),
                tool_name: tool.namespaced_name,
                schema: tool.input_schema,
            });
        }
        for tool in local_tools {
            out.push(CapabilityToolAvailability {
                provider: "custom".to_string(),
                tool_name: tool.name,
                schema: tool.input_schema,
            });
        }
        out.sort_by(|a, b| {
            a.provider
                .cmp(&b.provider)
                .then_with(|| a.tool_name.cmp(&b.tool_name))
        });
        out.dedup_by(|a, b| {
            a.provider.eq_ignore_ascii_case(&b.provider)
                && a.tool_name.eq_ignore_ascii_case(&b.tool_name)
        });
        out
    }

    pub fn missing_capability_error(
        workflow_id: &str,
        missing_capabilities: &[String],
        available_capability_bindings: &HashMap<String, Vec<String>>,
    ) -> Value {
        let suggestions = missing_capabilities
            .iter()
            .map(|cap| {
                let bindings = available_capability_bindings
                    .get(cap)
                    .cloned()
                    .unwrap_or_default();
                serde_json::json!({
                    "capability_id": cap,
                    "available_bindings": bindings,
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "code": "missing_capability",
            "workflow_id": workflow_id,
            "missing_capabilities": missing_capabilities,
            "suggestions": suggestions,
        })
    }

    async fn read_bindings(&self) -> anyhow::Result<CapabilityBindingsFile> {
        if !self.bindings_path.exists() {
            let default = CapabilityBindingsFile::default();
            self.set_bindings(default.clone()).await?;
            return Ok(default);
        }
        let raw = tokio::fs::read_to_string(&self.bindings_path).await?;
        let parsed = serde_json::from_str::<CapabilityBindingsFile>(&raw)?;
        Ok(parsed)
    }
}

fn group_bindings(
    bindings: &[CapabilityBinding],
) -> BTreeMap<String, Vec<(usize, &CapabilityBinding)>> {
    let mut map = BTreeMap::<String, Vec<(usize, &CapabilityBinding)>>::new();
    for (idx, binding) in bindings.iter().enumerate() {
        map.entry(binding.capability_id.clone())
            .or_default()
            .push((idx, binding));
    }
    map
}

fn provider_from_tool_name(tool_name: &str) -> String {
    let normalized = tool_name.to_ascii_lowercase();
    if normalized.starts_with("mcp.composio.") {
        return "composio".to_string();
    }
    if normalized.starts_with("mcp.arcade.") {
        return "arcade".to_string();
    }
    if normalized.starts_with("mcp.") {
        return "mcp".to_string();
    }
    "custom".to_string()
}

fn validate_bindings(file: &CapabilityBindingsFile) -> anyhow::Result<()> {
    if file.schema_version.trim().is_empty() {
        return Err(anyhow!("schema_version is required"));
    }
    for binding in &file.bindings {
        if binding.capability_id.trim().is_empty() {
            return Err(anyhow!("binding capability_id is required"));
        }
        if binding.provider.trim().is_empty() {
            return Err(anyhow!("binding provider is required"));
        }
        if binding.tool_name.trim().is_empty() {
            return Err(anyhow!("binding tool_name is required"));
        }
        for alias in &binding.tool_name_aliases {
            if alias.trim().is_empty() {
                return Err(anyhow!(
                    "binding tool_name_aliases cannot contain empty values"
                ));
            }
        }
    }
    Ok(())
}

fn default_spine_bindings() -> Vec<CapabilityBinding> {
    vec![
        make_binding(
            "github.create_pull_request",
            "composio",
            "mcp.composio.github_create_pull_request",
            &[
                "mcp.composio.github.create_pull_request",
                "mcp.composio.github_create_pr",
            ],
        ),
        make_binding(
            "github.create_pull_request",
            "arcade",
            "mcp.arcade.github_create_pull_request",
            &["mcp.arcade.github.create_pull_request"],
        ),
        make_binding(
            "github.create_pull_request",
            "mcp",
            "mcp.github.create_pull_request",
            &["mcp.github_create_pull_request"],
        ),
        make_binding(
            "github.create_issue",
            "composio",
            "mcp.composio.github_create_issue",
            &["mcp.composio.github.create_issue"],
        ),
        make_binding(
            "github.create_issue",
            "arcade",
            "mcp.arcade.github_create_issue",
            &["mcp.arcade.github.create_issue"],
        ),
        make_binding(
            "github.create_issue",
            "mcp",
            "mcp.github.create_issue",
            &["mcp.github_create_issue"],
        ),
        make_binding(
            "github.list_issues",
            "composio",
            "mcp.composio.github_list_issues",
            &["mcp.composio.github.list_issues"],
        ),
        make_binding(
            "github.get_issue",
            "composio",
            "mcp.composio.github_get_issue",
            &["mcp.composio.github.get_issue"],
        ),
        make_binding(
            "github.close_issue",
            "composio",
            "mcp.composio.github_close_issue",
            &["mcp.composio.github.close_issue"],
        ),
        make_binding(
            "github.create_branch",
            "composio",
            "mcp.composio.github_create_branch",
            &["mcp.composio.github.create_branch"],
        ),
        make_binding(
            "github.list_pull_requests",
            "composio",
            "mcp.composio.github_list_pull_requests",
            &["mcp.composio.github.list_pull_requests"],
        ),
        make_binding(
            "github.get_pull_request",
            "composio",
            "mcp.composio.github_get_pull_request",
            &["mcp.composio.github.get_pull_request"],
        ),
        make_binding(
            "github.comment_on_issue",
            "composio",
            "mcp.composio.github_create_issue_comment",
            &["mcp.composio.github.comment_on_issue"],
        ),
        make_binding(
            "github.comment_on_pull_request",
            "composio",
            "mcp.composio.github_create_pull_request_review_comment",
            &["mcp.composio.github.comment_on_pull_request"],
        ),
        make_binding(
            "github.list_repositories",
            "composio",
            "mcp.composio.github_list_repositories",
            &["mcp.composio.github.list_repositories"],
        ),
        make_binding(
            "slack.post_message",
            "composio",
            "mcp.composio.slack_post_message",
            &["mcp.composio.slack.post_message"],
        ),
        make_binding(
            "slack.post_message",
            "arcade",
            "mcp.arcade.slack_post_message",
            &["mcp.arcade.slack.post_message"],
        ),
        make_binding(
            "slack.reply_in_thread",
            "composio",
            "mcp.composio.slack_reply_to_thread",
            &[
                "mcp.composio.slack_reply_in_thread",
                "mcp.composio.slack.reply_in_thread",
            ],
        ),
        make_binding(
            "slack.update_message",
            "composio",
            "mcp.composio.slack_update_message",
            &["mcp.composio.slack.update_message"],
        ),
        make_binding(
            "slack.list_channels",
            "composio",
            "mcp.composio.slack_list_channels",
            &["mcp.composio.slack.list_channels"],
        ),
        make_binding(
            "slack.get_channel_history",
            "composio",
            "mcp.composio.slack_get_channel_history",
            &["mcp.composio.slack.get_channel_history"],
        ),
    ]
}

fn make_binding(
    capability_id: &str,
    provider: &str,
    tool_name: &str,
    aliases: &[&str],
) -> CapabilityBinding {
    CapabilityBinding {
        capability_id: capability_id.to_string(),
        provider: provider.to_string(),
        tool_name: tool_name.to_string(),
        tool_name_aliases: aliases.iter().map(|row| row.to_string()).collect(),
        request_transform: None,
        response_transform: None,
        metadata: serde_json::json!({"spine": true}),
    }
}

fn canonical_tool_name(name: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in name.chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }
    out.trim_matches('_').to_string()
}

fn binding_matches_available(
    binding: &CapabilityBinding,
    provider: &str,
    available_set: &HashSet<(String, String)>,
) -> bool {
    let mut names = Vec::with_capacity(1 + binding.tool_name_aliases.len());
    names.push(binding.tool_name.as_str());
    for alias in &binding.tool_name_aliases {
        names.push(alias.as_str());
    }
    names.into_iter().any(|tool_name| {
        available_set.contains(&(provider.to_string(), canonical_tool_name(tool_name)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolve_prefers_composio_over_arcade_by_default() {
        let root =
            std::env::temp_dir().join(format!("tandem-cap-resolver-{}", uuid::Uuid::new_v4()));
        let resolver = CapabilityResolver::new(root.clone());
        let result = resolver
            .resolve(
                CapabilityResolveInput {
                    workflow_id: Some("wf-1".to_string()),
                    required_capabilities: vec!["github.create_pull_request".to_string()],
                    optional_capabilities: vec![],
                    provider_preference: vec![],
                    available_tools: vec![
                        CapabilityToolAvailability {
                            provider: "arcade".to_string(),
                            tool_name: "mcp.arcade.github_create_pull_request".to_string(),
                            schema: Value::Null,
                        },
                        CapabilityToolAvailability {
                            provider: "composio".to_string(),
                            tool_name: "mcp.composio.github_create_pull_request".to_string(),
                            schema: Value::Null,
                        },
                    ],
                },
                Vec::new(),
            )
            .await
            .expect("resolve");
        assert_eq!(result.missing_required, Vec::<String>::new());
        assert_eq!(result.resolved.len(), 1);
        assert_eq!(result.resolved[0].provider, "composio");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn resolve_returns_missing_capability_when_unavailable() {
        let root =
            std::env::temp_dir().join(format!("tandem-cap-resolver-{}", uuid::Uuid::new_v4()));
        let resolver = CapabilityResolver::new(root.clone());
        let result = resolver
            .resolve(
                CapabilityResolveInput {
                    workflow_id: Some("wf-2".to_string()),
                    required_capabilities: vec!["github.create_pull_request".to_string()],
                    optional_capabilities: vec![],
                    provider_preference: vec!["arcade".to_string()],
                    available_tools: vec![],
                },
                Vec::new(),
            )
            .await
            .expect("resolve");
        assert_eq!(
            result.missing_required,
            vec!["github.create_pull_request".to_string()]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn resolve_matches_alias_with_name_normalization() {
        let root =
            std::env::temp_dir().join(format!("tandem-cap-resolver-{}", uuid::Uuid::new_v4()));
        let resolver = CapabilityResolver::new(root.clone());
        let result = resolver
            .resolve(
                CapabilityResolveInput {
                    workflow_id: Some("wf-3".to_string()),
                    required_capabilities: vec!["slack.reply_in_thread".to_string()],
                    optional_capabilities: vec![],
                    provider_preference: vec![],
                    available_tools: vec![CapabilityToolAvailability {
                        provider: "composio".to_string(),
                        tool_name: "mcp.composio.slack.reply.in.thread".to_string(),
                        schema: Value::Null,
                    }],
                },
                Vec::new(),
            )
            .await
            .expect("resolve");
        assert_eq!(result.missing_required, Vec::<String>::new());
        assert_eq!(result.resolved.len(), 1);
        assert_eq!(result.resolved[0].capability_id, "slack.reply_in_thread");
        let _ = std::fs::remove_dir_all(root);
    }
}
