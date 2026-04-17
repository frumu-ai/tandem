
impl<'a> GithubProjectsAdapter<'a> {
    async fn resolve_project_tools(
        &self,
        preferred_server: Option<&str>,
        workflow_id: &str,
        required_capabilities: &[&str],
    ) -> Result<(String, Vec<McpRemoteTool>, Vec<(String, String)>), StatusCode> {
        let _ = ensure_builtin_github_mcp_server(self.state).await;

        let mut server_candidates = if let Some(server_name) = preferred_server
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            vec![server_name.to_string()]
        } else {
            let mut servers = self
                .state
                .mcp
                .list()
                .await
                .into_values()
                .filter(|server| server.enabled && server.connected)
                .map(|server| server.name)
                .collect::<Vec<_>>();
            servers.sort();
            servers
        };
        if server_candidates.is_empty() {
            return Err(StatusCode::CONFLICT);
        }
        for server_name in server_candidates.drain(..) {
            let server_tools = self.state.mcp.server_tools(&server_name).await;
            if server_tools.is_empty() {
                continue;
            }
            let discovered = self
                .state
                .capability_resolver
                .discover_from_runtime(server_tools.clone(), Vec::new())
                .await;
            let resolved = self
                .state
                .capability_resolver
                .resolve(
                    crate::capability_resolver::CapabilityResolveInput {
                        workflow_id: Some(workflow_id.to_string()),
                        required_capabilities: required_capabilities
                            .iter()
                            .map(|value| value.to_string())
                            .collect(),
                        optional_capabilities: Vec::new(),
                        provider_preference: vec!["mcp".to_string()],
                        available_tools: discovered,
                    },
                    Vec::new(),
                )
                .await
                .map_err(|_| StatusCode::BAD_GATEWAY)?;
            let mut mapped = Vec::new();
            let mut all_present = true;
            for capability_id in required_capabilities {
                let Some(namespaced) = resolved
                    .resolved
                    .iter()
                    .find(|row| row.capability_id == *capability_id)
                    .map(|row| row.tool_name.clone())
                else {
                    all_present = false;
                    break;
                };
                let raw_tool = map_namespaced_to_raw_tool(&server_tools, &namespaced)?;
                mapped.push(((*capability_id).to_string(), raw_tool));
            }
            if all_present {
                return Ok((server_name, server_tools, mapped));
            }
        }
        Err(StatusCode::CONFLICT)
    }

    fn parse_project_schema(
        &self,
        result: &tandem_types::ToolResult,
    ) -> Result<(Value, CoderGithubProjectStatusMapping, String), StatusCode> {
        let schema = tool_result_values(result)
            .into_iter()
            .find(|value| value.is_object())
            .ok_or(StatusCode::BAD_GATEWAY)?;
        let fields = schema
            .get("fields")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let status_field = fields
            .iter()
            .find(|field| {
                field
                    .get("name")
                    .and_then(Value::as_str)
                    .map(|name| status_alias_matches(name, &["status"]))
                    .unwrap_or(false)
            })
            .cloned()
            .ok_or(StatusCode::BAD_GATEWAY)?;
        let field_id = status_field
            .get("id")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .ok_or(StatusCode::BAD_GATEWAY)?;
        let field_name = status_field
            .get("name")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .ok_or(StatusCode::BAD_GATEWAY)?;
        let options = status_field
            .get("options")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let resolve_option =
            |aliases: &[&str]| -> Result<CoderGithubProjectStatusOption, StatusCode> {
                options
                    .iter()
                    .find_map(|option| {
                        let name = option.get("name").and_then(Value::as_str)?;
                        if !status_alias_matches(name, aliases) {
                            return None;
                        }
                        Some(CoderGithubProjectStatusOption {
                            id: option.get("id").and_then(Value::as_str)?.to_string(),
                            name: name.to_string(),
                        })
                    })
                    .ok_or(StatusCode::BAD_GATEWAY)
            };
        let mapping = CoderGithubProjectStatusMapping {
            field_id,
            field_name,
            todo: resolve_option(&["todo", "todos", "backlog", "to do"])?,
            in_progress: resolve_option(&["inprogress", "in progress", "doing", "active"])?,
            in_review: resolve_option(&["inreview", "in review", "review"])?,
            blocked: resolve_option(&["blocked", "onhold", "on hold", "stalled"])?,
            done: resolve_option(&["done", "completed", "complete", "closed"])?,
        };
        let fingerprint = hash_json_fingerprint(&schema)?;
        Ok((schema, mapping, fingerprint))
    }

    async fn discover_binding(
        &self,
        request: &CoderGithubProjectBindingRequest,
    ) -> Result<CoderGithubProjectBinding, StatusCode> {
        let preferred_server = request.mcp_server.as_deref();
        let (server_name, _tools, mapped) = self
            .resolve_project_tools(
                preferred_server,
                "coder_github_project_bind",
                &[
                    "github.get_project",
                    "github.list_project_items",
                    "github.update_project_item_field",
                ],
            )
            .await?;
        let get_project_tool = mapped
            .iter()
            .find(|(capability_id, _)| capability_id == "github.get_project")
            .map(|(_, tool)| tool.clone())
            .ok_or(StatusCode::BAD_GATEWAY)?;
        let result = self
            .state
            .mcp
            .call_tool(
                &server_name,
                &get_project_tool,
                json!({
                    "owner": request.owner,
                    "project_number": request.project_number,
                }),
            )
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        let (schema_snapshot, status_mapping, schema_fingerprint) =
            self.parse_project_schema(&result)?;
        Ok(CoderGithubProjectBinding {
            owner: request.owner.clone(),
            project_number: request.project_number,
            repo_slug: request.repo_slug.clone(),
            mcp_server: Some(server_name.clone()),
            schema_snapshot,
            schema_fingerprint,
            status_mapping,
        })
    }

    async fn list_inbox_items(
        &self,
        binding: &CoderGithubProjectBinding,
    ) -> Result<Vec<GithubProjectInboxItemRecord>, StatusCode> {
        let preferred_server = binding.mcp_server.as_deref();
        let (server_name, _tools, mapped) = self
            .resolve_project_tools(
                preferred_server,
                "coder_github_project_inbox",
                &["github.list_project_items"],
            )
            .await?;
        let list_items_tool = mapped
            .iter()
            .find(|(capability_id, _)| capability_id == "github.list_project_items")
            .map(|(_, tool)| tool.clone())
            .ok_or(StatusCode::BAD_GATEWAY)?;
        let result = self
            .state
            .mcp
            .call_tool(
                &server_name,
                &list_items_tool,
                json!({
                    "owner": binding.owner,
                    "project_number": binding.project_number,
                }),
            )
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        let mut out = Vec::new();
        for candidate in tool_result_values(&result) {
            collect_project_items(&candidate, &mut out);
        }
        let mut deduped = Vec::new();
        let mut seen = HashSet::new();
        for item in out {
            if seen.insert(item.project_item_id.clone()) {
                deduped.push(item);
            }
        }
        Ok(deduped)
    }

    async fn update_project_item_status(
        &self,
        binding: &CoderGithubProjectBinding,
        project_item_id: &str,
        option: &CoderGithubProjectStatusOption,
    ) -> Result<(), StatusCode> {
        let preferred_server = binding.mcp_server.as_deref();
        let (server_name, _tools, mapped) = self
            .resolve_project_tools(
                preferred_server,
                "coder_github_project_status_sync",
                &["github.update_project_item_field"],
            )
            .await?;
        let update_tool = mapped
            .iter()
            .find(|(capability_id, _)| capability_id == "github.update_project_item_field")
            .map(|(_, tool)| tool.clone())
            .ok_or(StatusCode::BAD_GATEWAY)?;
        self.state
            .mcp
            .call_tool(
                &server_name,
                &update_tool,
                json!({
                    "owner": binding.owner,
                    "project_number": binding.project_number,
                    "project_item_id": project_item_id,
                    "field_id": binding.status_mapping.field_id,
                    "single_select_option_id": option.id,
                }),
            )
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        Ok(())
    }
}

fn collect_project_items(value: &Value, out: &mut Vec<GithubProjectInboxItemRecord>) {
    match value {
        Value::Object(map) => {
            let project_item_id = map
                .get("id")
                .or_else(|| map.get("item_id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let title = map
                .get("title")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| {
                    map.get("content")
                        .and_then(Value::as_object)
                        .and_then(|content| content.get("title"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })
                .unwrap_or_default();
            let status_name = map
                .get("status")
                .and_then(Value::as_object)
                .and_then(|status| status.get("name"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| {
                    map.get("field_values")
                        .and_then(Value::as_object)
                        .and_then(|fields| fields.get("status"))
                        .and_then(Value::as_object)
                        .and_then(|status| status.get("name"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })
                .unwrap_or_default();
            let status_option_id = map
                .get("status")
                .and_then(Value::as_object)
                .and_then(|status| status.get("id"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| {
                    map.get("field_values")
                        .and_then(Value::as_object)
                        .and_then(|fields| fields.get("status"))
                        .and_then(Value::as_object)
                        .and_then(|status| status.get("id"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                });
            let issue = map
                .get("content")
                .and_then(Value::as_object)
                .and_then(|content| {
                    let type_name = content
                        .get("type")
                        .or_else(|| content.get("__typename"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !type_name.eq_ignore_ascii_case("issue") {
                        return None;
                    }
                    Some(GithubProjectIssueSummary {
                        number: content
                            .get("number")
                            .or_else(|| content.get("issue_number"))
                            .and_then(Value::as_u64)?,
                        title: content
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        html_url: content
                            .get("url")
                            .or_else(|| content.get("html_url"))
                            .and_then(Value::as_str)
                            .map(ToString::to_string),
                    })
                });
            if !project_item_id.is_empty() {
                out.push(GithubProjectInboxItemRecord {
                    project_item_id,
                    title,
                    status_name,
                    status_option_id,
                    issue,
                    raw: value.clone(),
                });
                return;
            }
            for nested in map.values() {
                collect_project_items(nested, out);
            }
        }
        Value::Array(rows) => {
            for row in rows {
                collect_project_items(row, out);
            }
        }
        _ => {}
    }
}

fn split_owner_repo(repo: &str) -> Result<(&str, &str), StatusCode> {
    let mut parts = repo.split('/');
    let owner = parts
        .next()
        .filter(|value| !value.trim().is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let repo_name = parts
        .next()
        .filter(|value| !value.trim().is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    if parts.next().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok((owner, repo_name))
}

fn map_namespaced_to_raw_tool(
    tools: &[McpRemoteTool],
    namespaced_name_or_raw_tool: &str,
) -> Result<String, StatusCode> {
    tools
        .iter()
        .find(|row| {
            row.namespaced_name == namespaced_name_or_raw_tool
                || row.tool_name == namespaced_name_or_raw_tool
        })
        .map(|row| row.tool_name.clone())
        .ok_or(StatusCode::BAD_GATEWAY)
}

async fn resolve_github_create_pr_tool(
    state: &AppState,
    preferred_server: Option<&str>,
) -> Result<(String, String), StatusCode> {
    let mut server_candidates = if let Some(server_name) = preferred_server
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        vec![server_name.to_string()]
    } else {
        let mut servers = state
            .mcp
            .list()
            .await
            .into_values()
            .filter(|server| server.enabled && server.connected)
            .map(|server| server.name)
            .collect::<Vec<_>>();
        servers.sort();
        servers
    };
    if server_candidates.is_empty() {
        return Err(StatusCode::CONFLICT);
    }
    for server_name in server_candidates.drain(..) {
        let server_tools = state.mcp.server_tools(&server_name).await;
        if server_tools.is_empty() {
            continue;
        }
        let discovered = state
            .capability_resolver
            .discover_from_runtime(server_tools.clone(), Vec::new())
            .await;
        let resolved = state
            .capability_resolver
            .resolve(
                crate::capability_resolver::CapabilityResolveInput {
                    workflow_id: Some("coder_issue_fix_pr_submit".to_string()),
                    required_capabilities: vec!["github.create_pull_request".to_string()],
                    optional_capabilities: Vec::new(),
                    provider_preference: vec!["mcp".to_string()],
                    available_tools: discovered,
                },
                Vec::new(),
            )
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        let Some(namespaced) = resolved
            .resolved
            .iter()
            .find(|row| row.capability_id == "github.create_pull_request")
            .map(|row| row.tool_name.clone())
        else {
            continue;
        };
        let raw_tool = map_namespaced_to_raw_tool(&server_tools, &namespaced)?;
        return Ok((server_name, raw_tool));
    }
    Err(StatusCode::CONFLICT)
}

async fn resolve_github_merge_pr_tool(
    state: &AppState,
    preferred_server: Option<&str>,
) -> Result<(String, String), StatusCode> {
    let mut server_candidates = if let Some(server_name) = preferred_server
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        vec![server_name.to_string()]
    } else {
        let mut servers = state
            .mcp
            .list()
            .await
            .into_values()
            .filter(|server| server.enabled && server.connected)
            .map(|server| server.name)
            .collect::<Vec<_>>();
        servers.sort();
        servers
    };
    if server_candidates.is_empty() {
        return Err(StatusCode::CONFLICT);
    }
    for server_name in server_candidates.drain(..) {
        let server_tools = state.mcp.server_tools(&server_name).await;
        if server_tools.is_empty() {
            continue;
        }
        let discovered = state
            .capability_resolver
            .discover_from_runtime(server_tools.clone(), Vec::new())
            .await;
        let resolved = state
            .capability_resolver
            .resolve(
                crate::capability_resolver::CapabilityResolveInput {
                    workflow_id: Some("coder_merge_submit".to_string()),
                    required_capabilities: vec!["github.merge_pull_request".to_string()],
                    optional_capabilities: Vec::new(),
                    provider_preference: vec!["mcp".to_string()],
                    available_tools: discovered,
                },
                Vec::new(),
            )
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        let Some(namespaced) = resolved
            .resolved
            .iter()
            .find(|row| row.capability_id == "github.merge_pull_request")
            .map(|row| row.tool_name.clone())
        else {
            continue;
        };
        let raw_tool = map_namespaced_to_raw_tool(&server_tools, &namespaced)?;
        return Ok((server_name, raw_tool));
    }
    Err(StatusCode::CONFLICT)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct GithubPullRequestSummary {
    number: u64,
    title: String,
    state: String,
    html_url: Option<String>,
    head_ref: Option<String>,
    base_ref: Option<String>,
}

fn tool_result_values(result: &tandem_types::ToolResult) -> Vec<Value> {
    let mut values = Vec::new();
    if let Some(value) = result.metadata.get("result") {
        values.push(value.clone());
    }
    if let Ok(parsed) = serde_json::from_str::<Value>(&result.output) {
        values.push(parsed);
    }
    values
}

fn extract_pull_requests_from_tool_result(
    result: &tandem_types::ToolResult,
) -> Vec<GithubPullRequestSummary> {
    let mut out = Vec::new();
    for candidate in tool_result_values(result) {
        collect_pull_requests(&candidate, &mut out);
    }
    dedupe_pull_requests(out)
}

fn extract_merge_result_from_tool_result(result: &tandem_types::ToolResult) -> Value {
    for candidate in tool_result_values(result) {
        if candidate.is_object()
            && (candidate.get("merged").is_some()
                || candidate.get("sha").is_some()
                || candidate.get("message").is_some())
        {
            return candidate;
        }
    }
    json!({
        "output": result.output,
        "metadata": result.metadata,
    })
}

fn collect_pull_requests(value: &Value, out: &mut Vec<GithubPullRequestSummary>) {
    match value {
        Value::Object(map) => {
            let number = map
                .get("number")
                .or_else(|| map.get("pull_number"))
                .and_then(Value::as_u64);
            let title = map
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let state = map
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let html_url = map
                .get("html_url")
                .or_else(|| map.get("url"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let head_ref = map
                .get("head")
                .and_then(Value::as_object)
                .and_then(|head| head.get("ref"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| {
                    map.get("head_ref")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                });
            let base_ref = map
                .get("base")
                .and_then(Value::as_object)
                .and_then(|base| base.get("ref"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| {
                    map.get("base_ref")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                });
            if let Some(number) = number {
                out.push(GithubPullRequestSummary {
                    number,
                    title,
                    state,
                    html_url,
                    head_ref,
                    base_ref,
                });
            }
            for nested in map.values() {
                collect_pull_requests(nested, out);
            }
        }
        Value::Array(rows) => {
            for row in rows {
                collect_pull_requests(row, out);
            }
        }
        _ => {}
    }
}

fn dedupe_pull_requests(rows: Vec<GithubPullRequestSummary>) -> Vec<GithubPullRequestSummary> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        if seen.insert(row.number) {
            out.push(row);
        }
    }
    out
}

fn github_ref_from_pull_request(pull: &GithubPullRequestSummary) -> Value {
    json!({
        "kind": "pull_request",
        "number": pull.number,
        "url": pull.html_url,
    })
}

fn parse_coder_github_ref(value: &Value) -> Option<CoderGithubRef> {
    let kind = match value.get("kind").and_then(Value::as_str)? {
        "issue" => CoderGithubRefKind::Issue,
        "pull_request" => CoderGithubRefKind::PullRequest,
        _ => return None,
    };
    Some(CoderGithubRef {
        kind,
        number: value.get("number").and_then(Value::as_u64)?,
        url: value
            .get("url")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    })
}

fn build_follow_on_run_templates(
    record: &CoderRunRecord,
    github_ref: &CoderGithubRef,
    mcp_servers: &[String],
    requested_follow_on_runs: &[CoderWorkflowMode],
    allow_auto_merge_recommendation: bool,
    project_auto_merge_enabled: bool,
    skipped_follow_on_runs: &[Value],
) -> Vec<Value> {
    [
        CoderWorkflowMode::PrReview,
        CoderWorkflowMode::MergeRecommendation,
    ]
    .into_iter()
    .map(|workflow_mode| {
        let requires_explicit_auto_spawn =
            matches!(workflow_mode, CoderWorkflowMode::MergeRecommendation);
        let required_completed_workflow_modes =
            if matches!(workflow_mode, CoderWorkflowMode::MergeRecommendation) {
                vec![json!("pr_review")]
            } else {
                Vec::new()
            };
        let merge_submit_policy_preview =
            if matches!(workflow_mode, CoderWorkflowMode::MergeRecommendation) {
                json!({
                    "manual": blocked_merge_submit_policy("manual", json!({
                        "reason": "requires_merge_execution_request",
                    })),
                    "auto": blocked_merge_submit_policy("auto", json!({
                        "reason": "requires_merge_execution_request",
                        "merge_auto_spawn_opted_in": allow_auto_merge_recommendation,
                    })),
                    "preferred_submit_mode": "manual",
                    "explicit_submit_required": true,
                    "auto_execute_after_approval": false,
                    "auto_execute_eligible": false,
                    "auto_execute_policy_enabled": project_auto_merge_enabled,
                    "auto_execute_block_reason": if project_auto_merge_enabled {
                        "requires_merge_execution_request"
                    } else {
                        "project_auto_merge_policy_disabled"
                    },
                })
            } else {
                Value::Null
            };
        json!({
            "workflow_mode": workflow_mode,
            "repo_binding": record.repo_binding,
            "github_ref": github_ref,
            "source_client": record.source_client,
            "model_provider": record.model_provider,
            "model_id": record.model_id,
            "mcp_servers": mcp_servers,
            "parent_coder_run_id": record.coder_run_id,
            "origin": "issue_fix_pr_submit_template",
            "origin_artifact_type": "coder_pr_submission",
            "origin_policy": {
                "source": "issue_fix_pr_submit",
                "spawn_mode": "template",
                "merge_auto_spawn_opted_in": allow_auto_merge_recommendation,
                "requested_follow_on_runs": requested_follow_on_runs,
                "skipped_follow_on_runs": skipped_follow_on_runs,
                "template_workflow_mode": workflow_mode,
                "requires_explicit_auto_spawn": requires_explicit_auto_spawn,
                "required_completed_workflow_modes": required_completed_workflow_modes,
            },
            "auto_spawn_allowed_by_default": !requires_explicit_auto_spawn,
            "requires_explicit_auto_spawn": requires_explicit_auto_spawn,
            "required_completed_workflow_modes": required_completed_workflow_modes,
            "execution_policy_preview": follow_on_execution_policy_preview(
                &workflow_mode,
                &required_completed_workflow_modes,
            ),
            "merge_submit_policy_preview": merge_submit_policy_preview,
        })
    })
    .collect::<Vec<_>>()
}

fn normalize_follow_on_workflow_modes(requested: &[CoderWorkflowMode]) -> Vec<CoderWorkflowMode> {
    let wants_review = requested
        .iter()
        .any(|mode| matches!(mode, CoderWorkflowMode::PrReview));
    let wants_merge = requested
        .iter()
        .any(|mode| matches!(mode, CoderWorkflowMode::MergeRecommendation));
    let mut normalized = Vec::new();
    if wants_review || wants_merge {
        normalized.push(CoderWorkflowMode::PrReview);
    }
    if wants_merge {
        normalized.push(CoderWorkflowMode::MergeRecommendation);
    }
    normalized
}

fn split_auto_spawn_follow_on_workflow_modes(
    requested: &[CoderWorkflowMode],
    allow_auto_merge_recommendation: bool,
) -> (Vec<CoderWorkflowMode>, Vec<Value>) {
    let mut auto_spawn_modes = Vec::new();
    let mut skipped = Vec::new();
    for workflow_mode in normalize_follow_on_workflow_modes(requested) {
        if matches!(workflow_mode, CoderWorkflowMode::MergeRecommendation)
            && !allow_auto_merge_recommendation
        {
            skipped.push(json!({
                "workflow_mode": workflow_mode,
                "reason": "requires_explicit_auto_merge_recommendation_opt_in",
            }));
            continue;
        }
        auto_spawn_modes.push(workflow_mode);
    }
    (auto_spawn_modes, skipped)
}

fn build_follow_on_run_create_input(
    record: &CoderRunRecord,
    workflow_mode: CoderWorkflowMode,
    github_ref: CoderGithubRef,
    source_client: Option<String>,
    model_provider: Option<String>,
    model_id: Option<String>,
    mcp_servers: Option<Vec<String>>,
    parent_coder_run_id: Option<String>,
    origin: Option<String>,
    origin_artifact_type: Option<String>,
    origin_policy: Option<Value>,
) -> CoderRunCreateInput {
    CoderRunCreateInput {
        coder_run_id: None,
        workflow_mode,
        repo_binding: record.repo_binding.clone(),
        github_ref: Some(github_ref),
        objective: None,
        source_client,
        workspace: None,
        model_provider,
        model_id,
        mcp_servers,
        parent_coder_run_id,
        origin,
        origin_artifact_type,
        origin_policy,
    }
}

async fn call_create_pull_request(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    owner: &str,
    repo: &str,
    title: &str,
    body: &str,
    base_branch: &str,
    head_branch: &str,
) -> Result<tandem_types::ToolResult, StatusCode> {
    let preferred = json!({
        "method": "create",
        "owner": owner,
        "repo": repo,
        "title": title,
        "body": body,
        "base": base_branch,
        "head": head_branch,
        "draft": true,
    });
    let fallback = json!({
        "owner": owner,
        "repo": repo,
        "title": title,
        "body": body,
        "base": base_branch,
        "head": head_branch,
        "draft": true,
    });
    let first = state.mcp.call_tool(server_name, tool_name, preferred).await;
    match first {
        Ok(result) => Ok(result),
        Err(_) => state
            .mcp
            .call_tool(server_name, tool_name, fallback)
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY),
    }
}

async fn call_merge_pull_request(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    owner: &str,
    repo: &str,
    pull_number: u64,
) -> Result<tandem_types::ToolResult, StatusCode> {
    let preferred = json!({
        "owner": owner,
        "repo": repo,
        "pull_number": pull_number,
        "merge_method": "squash",
    });
    let fallback = json!({
        "owner": owner,
        "repo": repo,
        "number": pull_number,
    });
    let first = state.mcp.call_tool(server_name, tool_name, preferred).await;
    match first {
        Ok(result) => Ok(result),
        Err(_) => state
            .mcp
            .call_tool(server_name, tool_name, fallback)
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY),
    }
}

async fn record_coder_external_action(
    state: &AppState,
    record: &CoderRunRecord,
    operation: &str,
    capability_id: &str,
    provider: &str,
    target: &str,
    idempotency_key: &str,
    receipt: Value,
    metadata: Value,
) -> Option<ExternalActionRecord> {
    let action = ExternalActionRecord {
        action_id: format!("external-action-{}", Uuid::new_v4().simple()),
        operation: operation.to_string(),
        status: "posted".to_string(),
        source_kind: Some("coder".to_string()),
        source_id: Some(record.coder_run_id.clone()),
        routine_run_id: None,
        context_run_id: Some(record.linked_context_run_id.clone()),
        capability_id: Some(capability_id.to_string()),
        provider: Some(provider.to_string()),
        target: Some(target.to_string()),
        approval_state: Some("executed".to_string()),
        idempotency_key: Some(idempotency_key.to_string()),
        receipt: Some(receipt),
        error: None,
        metadata: Some(metadata),
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
    };
    match state.record_external_action(action).await {
        Ok(action) => Some(action),
        Err(error) => {
            tracing::warn!(
                "failed to record coder external action for run {}: {}",
                record.coder_run_id,
                error
            );
            None
        }
    }
}

pub(super) async fn coder_issue_fix_pr_draft_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderIssueFixPrDraftCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let summary_payload =
        load_latest_coder_artifact_payload(&state, &record, "coder_issue_fix_summary").await;
    let patch_summary_payload =
        load_latest_coder_artifact_payload(&state, &record, "coder_patch_summary").await;
    let validation_payload =
        load_latest_coder_artifact_payload(&state, &record, "coder_validation_report").await;
    let title =
        build_issue_fix_pr_draft_title(&record, input.title.as_deref(), summary_payload.as_ref());
    let body = build_issue_fix_pr_draft_body(
        &record,
        input.body.as_deref(),
        summary_payload.as_ref(),
        patch_summary_payload.as_ref(),
        validation_payload.as_ref(),
        &input.changed_files,
        input.notes.as_deref(),
    );
    let head_branch = input
        .head_branch
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "coder/issue-{}-fix",
                record
                    .github_ref
                    .as_ref()
                    .map(|row| row.number)
                    .unwrap_or_default()
            )
        });
    let base_branch = input
        .base_branch
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "main".to_string());
    let changed_files = if !input.changed_files.is_empty() {
        input.changed_files.clone()
    } else {
        patch_summary_payload
            .as_ref()
            .and_then(|payload| payload.get("changed_files"))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "title": title,
        "body": body,
        "base_branch": base_branch,
        "head_branch": head_branch,
        "changed_files": changed_files,
        "memory_hits_used": input.memory_hits_used,
        "approval_required": true,
        "summary_artifact_path": summary_payload
            .as_ref()
            .and_then(|_| load_context_blackboard(&state, &record.linked_context_run_id)
                .artifacts
                .iter()
                .rev()
                .find(|artifact| artifact.artifact_type == "coder_issue_fix_summary")
                .map(|artifact| artifact.path.clone())),
        "patch_summary_artifact_path": patch_summary_payload
            .as_ref()
            .and_then(|_| load_context_blackboard(&state, &record.linked_context_run_id)
                .artifacts
                .iter()
                .rev()
                .find(|artifact| artifact.artifact_type == "coder_patch_summary")
                .map(|artifact| artifact.path.clone())),
        "validation_artifact_path": validation_payload
            .as_ref()
            .and_then(|_| load_context_blackboard(&state, &record.linked_context_run_id)
                .artifacts
                .iter()
                .rev()
                .find(|artifact| artifact.artifact_type == "coder_validation_report")
                .map(|artifact| artifact.path.clone())),
        "worker_run_reference": patch_summary_payload
            .as_ref()
            .and_then(|payload| payload.get("worker_run_reference"))
            .cloned()
            .or_else(|| {
                patch_summary_payload
                    .as_ref()
                    .and_then(|payload| payload.get("worker_session_context_run_id"))
                    .cloned()
            }),
        "worker_session_context_run_id": patch_summary_payload
            .as_ref()
            .and_then(|payload| payload.get("worker_session_context_run_id"))
            .cloned(),
        "validation_run_reference": patch_summary_payload
            .as_ref()
            .and_then(|payload| payload.get("validation_run_reference"))
            .cloned()
            .or_else(|| {
                validation_payload
                    .as_ref()
                    .and_then(|payload| payload.get("validation_run_reference"))
                    .cloned()
            })
            .or_else(|| {
                patch_summary_payload
                    .as_ref()
                    .and_then(|payload| payload.get("validation_session_context_run_id"))
                    .cloned()
            })
            .or_else(|| {
                validation_payload
                    .as_ref()
                    .and_then(|payload| payload.get("validation_session_context_run_id"))
                    .cloned()
            }),
        "validation_session_context_run_id": patch_summary_payload
            .as_ref()
            .and_then(|payload| payload.get("validation_session_context_run_id"))
            .cloned()
            .or_else(|| {
                validation_payload
                    .as_ref()
                    .and_then(|payload| payload.get("validation_session_context_run_id"))
                    .cloned()
            }),
        "created_at_ms": crate::now_ms(),
    });
    let artifact = write_coder_artifact(
        &state,
        &record.linked_context_run_id,
        &format!("issue-fix-pr-draft-{}", Uuid::new_v4().simple()),
        "coder_pr_draft",
        "artifacts/issue_fix.pr_draft.json",
        &payload,
    )
    .await?;
    publish_coder_artifact_added(&state, &record, &artifact, Some("approval"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("pr_draft"));
        extra.insert("title".to_string(), json!(payload["title"]));
        extra.insert("approval_required".to_string(), json!(true));
        extra
    });
    publish_coder_run_event(
        &state,
        "coder.approval.required",
        &record,
        Some("approval"),
        {
            let mut extra = serde_json::Map::new();
            extra.insert("event_type".to_string(), json!("pr_draft_ready"));
            extra.insert("artifact_id".to_string(), json!(artifact.id));
            extra.insert("title".to_string(), json!(payload["title"]));
            extra
        },
    );
    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
        "approval_required": true,
        "coder_run": coder_run_payload(
            &record,
            &load_context_run_state(&state, &record.linked_context_run_id).await?,
        ),
        "run": load_context_run_state(&state, &record.linked_context_run_id).await?,
    })))
}

pub(super) async fn coder_issue_fix_pr_submit(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderIssueFixPrSubmitInput>,
) -> Result<Json<Value>, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let approved_by = input
        .approved_by
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let readiness = coder_pr_submit_readiness(&state, input.mcp_server.as_deref()).await?;
    if !readiness.runnable {
        return Ok(Json(json!({
            "ok": false,
            "code": "CODER_PR_SUBMIT_BLOCKED",
            "readiness": readiness,
        })));
    }
    let draft_payload = load_latest_coder_artifact_payload(&state, &record, "coder_pr_draft")
        .await
        .ok_or(StatusCode::CONFLICT)?;
    let title = draft_payload
        .get("title")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(StatusCode::CONFLICT)?;
    let body = draft_payload
        .get("body")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(StatusCode::CONFLICT)?;
    let base_branch = draft_payload
        .get("base_branch")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("main");
    let head_branch = draft_payload
        .get("head_branch")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("coder/issue-fix");
    let dry_run = input.dry_run.unwrap_or(true);
    let requested_follow_on_modes = normalize_follow_on_workflow_modes(&input.spawn_follow_on_runs);
    for workflow_mode in &requested_follow_on_modes {
        if !matches!(
            workflow_mode,
            CoderWorkflowMode::PrReview | CoderWorkflowMode::MergeRecommendation
        ) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    let allow_auto_merge_recommendation = input.allow_auto_merge_recommendation.unwrap_or(false);
    let (auto_spawn_follow_on_modes, skipped_follow_on_runs) =
        split_auto_spawn_follow_on_workflow_modes(
            &input.spawn_follow_on_runs,
            allow_auto_merge_recommendation,
        );
    let (owner, repo_name) = split_owner_repo(&record.repo_binding.repo_slug)?;
    let mut submission_payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "owner": owner,
        "repo": repo_name,
        "approved_by": approved_by,
        "approval_reason": input.reason,
        "title": title,
        "body": body,
        "base_branch": base_branch,
        "head_branch": head_branch,
        "dry_run": dry_run,
        "requested_spawn_follow_on_runs": requested_follow_on_modes,
        "allow_auto_merge_recommendation": allow_auto_merge_recommendation,
        "worker_run_reference": draft_payload
            .get("worker_run_reference")
            .cloned()
            .or_else(|| draft_payload.get("worker_session_context_run_id").cloned())
            .unwrap_or(Value::Null),
        "worker_session_context_run_id": draft_payload
            .get("worker_session_context_run_id")
            .cloned()
            .unwrap_or(Value::Null),
        "validation_run_reference": draft_payload
            .get("validation_run_reference")
            .cloned()
            .or_else(|| draft_payload.get("validation_session_context_run_id").cloned())
            .unwrap_or(Value::Null),
        "validation_session_context_run_id": draft_payload
            .get("validation_session_context_run_id")
            .cloned()
            .unwrap_or(Value::Null),
        "submitted_github_ref": Value::Null,
        "skipped_follow_on_runs": skipped_follow_on_runs,
        "spawned_follow_on_runs": [],
        "created_at_ms": crate::now_ms(),
        "readiness": readiness,
    });
    if !dry_run {
        let (server_name, tool_name) =
            resolve_github_create_pr_tool(&state, input.mcp_server.as_deref()).await?;
        let result = call_create_pull_request(
            &state,
            &server_name,
            &tool_name,
            owner,
            repo_name,
            title,
            body,
            base_branch,
            head_branch,
        )
        .await?;
        let pull_request = extract_pull_requests_from_tool_result(&result)
            .into_iter()
            .next()
            .ok_or(StatusCode::BAD_GATEWAY)?;
        let submitted_github_ref =
            parse_coder_github_ref(&github_ref_from_pull_request(&pull_request))
                .ok_or(StatusCode::BAD_GATEWAY)?;
        let project_policy =
            load_coder_project_policy(&state, &record.repo_binding.project_id).await?;
        let follow_on_templates = build_follow_on_run_templates(
            &record,
            &submitted_github_ref,
            &[server_name.clone()],
            &requested_follow_on_modes,
            allow_auto_merge_recommendation,
            project_policy.auto_merge_enabled,
            &skipped_follow_on_runs,
        );
        if let Some(obj) = submission_payload.as_object_mut() {
            obj.insert("server_name".to_string(), json!(server_name));
            obj.insert("tool_name".to_string(), json!(tool_name));
            obj.insert("submitted".to_string(), json!(true));
            obj.insert(
                "submitted_github_ref".to_string(),
                json!(submitted_github_ref),
            );
            obj.insert("pull_request".to_string(), json!(pull_request));
            obj.insert("follow_on_runs".to_string(), json!(follow_on_templates));
            obj.insert(
                "tool_result".to_string(),
                json!({
                    "output": result.output,
                    "metadata": result.metadata,
                }),
            );
        }
    } else if let Some(obj) = submission_payload.as_object_mut() {
        obj.insert("submitted".to_string(), json!(false));
        obj.insert("follow_on_runs".to_string(), json!([]));
        obj.insert(
            "dry_run_preview".to_string(),
            json!({
                "owner": owner,
                "repo": repo_name,
                "base": base_branch,
                "head": head_branch,
            }),
        );
    }
    let mut spawned_follow_on_runs = Vec::<Value>::new();
    let mut external_action = Value::Null;
    if !dry_run {
        let submitted_github_ref = submission_payload
            .get("submitted_github_ref")
            .and_then(parse_coder_github_ref);
        if let Some(submitted_github_ref) = submitted_github_ref {
            for workflow_mode in &auto_spawn_follow_on_modes {
                let create_input = build_follow_on_run_create_input(
                    &record,
                    workflow_mode.clone(),
                    submitted_github_ref.clone(),
                    record.source_client.clone(),
                    record.model_provider.clone(),
                    record.model_id.clone(),
                    input
                        .mcp_server
                        .as_ref()
                        .map(|server| vec![server.clone()])
                        .or_else(|| Some(vec!["github".to_string()])),
                    Some(record.coder_run_id.clone()),
                    Some("issue_fix_pr_submit_auto".to_string()),
                    Some("coder_pr_submission".to_string()),
                    Some(json!({
                        "source": "issue_fix_pr_submit",
                        "spawn_mode": "auto",
                        "merge_auto_spawn_opted_in": allow_auto_merge_recommendation,
                        "requested_follow_on_runs": requested_follow_on_modes,
                        "effective_auto_spawn_runs": auto_spawn_follow_on_modes,
                        "skipped_follow_on_runs": skipped_follow_on_runs,
                    })),
                );
                let response = coder_run_create(State(state.clone()), Json(create_input)).await?;
                let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                let mut payload: Value = serde_json::from_slice(&bytes)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                if let Some(obj) = payload.as_object_mut() {
                    let coder_run_id = obj
                        .get("coder_run")
                        .and_then(|row| row.get("coder_run_id"))
                        .and_then(Value::as_str)
                        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
                    let created_record = load_coder_run_record(&state, coder_run_id).await?;
                    obj.insert(
                        "execution_policy".to_string(),
                        coder_execution_policy_summary(&state, &created_record).await?,
                    );
                }
                spawned_follow_on_runs.push(payload);
            }
        }
        if let Some(pull_request) = submission_payload.get("pull_request").cloned() {
            let idempotency_key = crate::sha256_hex(&[&format!(
                "{}|{}|{}|{}|{}",
                record.repo_binding.repo_slug, title, base_branch, head_branch, approved_by
            )]);
            if let Some(action) = record_coder_external_action(
                &state,
                &record,
                "create_pull_request",
                "github.create_pull_request",
                submission_payload
                    .get("server_name")
                    .and_then(Value::as_str)
                    .unwrap_or("github"),
                &record.repo_binding.repo_slug,
                &idempotency_key,
                json!({
                    "pull_request": pull_request,
                    "submitted_github_ref": submission_payload
                        .get("submitted_github_ref")
                        .cloned()
                        .unwrap_or(Value::Null),
                }),
                json!({
                    "workflow_mode": record.workflow_mode,
                    "base_branch": base_branch,
                    "head_branch": head_branch,
                    "approved_by": approved_by,
                }),
            )
            .await
            {
                external_action = serde_json::to_value(&action).unwrap_or(Value::Null);
            }
        }
    }
    if let Some(obj) = submission_payload.as_object_mut() {
        obj.insert(
            "spawned_follow_on_runs".to_string(),
            json!(spawned_follow_on_runs),
        );
        obj.insert("external_action".to_string(), external_action.clone());
    }
    let artifact = write_coder_artifact(
        &state,
        &record.linked_context_run_id,
        &format!("issue-fix-pr-submit-{}", Uuid::new_v4().simple()),
        "coder_pr_submission",
        "artifacts/issue_fix.pr_submission.json",
        &submission_payload,
    )
    .await?;
    publish_coder_artifact_added(&state, &record, &artifact, Some("approval"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("pr_submission"));
        extra.insert("dry_run".to_string(), json!(dry_run));
        extra.insert(
            "submitted".to_string(),
            json!(submission_payload
                .get("submitted")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        extra
    });
    let mut duplicate_linkage_candidate = Value::Null;
    if !dry_run {
        if let (Some(submitted_github_ref), Some(pull_request)) = (
            submission_payload
                .get("submitted_github_ref")
                .and_then(parse_coder_github_ref),
            submission_payload
                .get("pull_request")
                .cloned()
                .and_then(|row| serde_json::from_value::<GithubPullRequestSummary>(row).ok()),
        ) {
            let summary = record
                .github_ref
                .as_ref()
                .filter(|reference| matches!(reference.kind, CoderGithubRefKind::Issue))
                .map(|reference| {
                    format!(
                        "{} issue #{} is linked to pull request #{}",
                        record.repo_binding.repo_slug, reference.number, pull_request.number
                    )
                });
            let (candidate_id, candidate_artifact) = write_coder_memory_candidate_artifact(
                &state,
                &record,
                CoderMemoryCandidateKind::DuplicateLinkage,
                summary,
                Some("submit_pr".to_string()),
                build_duplicate_linkage_payload(
                    &record,
                    &submitted_github_ref,
                    &pull_request,
                    &artifact.path,
                ),
            )
            .await?;
            duplicate_linkage_candidate = json!({
                "candidate_id": candidate_id,
                "kind": "duplicate_linkage",
                "artifact_path": candidate_artifact.path,
            });
        }
    }
    if let Some(obj) = submission_payload.as_object_mut() {
        obj.insert(
            "duplicate_linkage_candidate".to_string(),
            duplicate_linkage_candidate.clone(),
        );
    }
    if !duplicate_linkage_candidate.is_null() {
        tokio::fs::write(
            &artifact.path,
            serde_json::to_vec_pretty(&submission_payload)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    if !dry_run {
        publish_coder_run_event(&state, "coder.pr.submitted", &record, Some("approval"), {
            let mut extra = serde_json::Map::new();
            extra.insert("artifact_id".to_string(), json!(artifact.id));
            extra.insert("title".to_string(), json!(title));
            extra.insert(
                "submitted_github_ref".to_string(),
                submission_payload
                    .get("submitted_github_ref")
                    .cloned()
                    .unwrap_or(Value::Null),
            );
            extra.insert(
                "follow_on_runs".to_string(),
                submission_payload
                    .get("follow_on_runs")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            );
            extra.insert(
                "spawned_follow_on_runs".to_string(),
                submission_payload
                    .get("spawned_follow_on_runs")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            );
            extra.insert(
                "skipped_follow_on_runs".to_string(),
                submission_payload
                    .get("skipped_follow_on_runs")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            );
            extra.insert(
                "duplicate_linkage_candidate".to_string(),
                duplicate_linkage_candidate.clone(),
            );
            if let Some(number) = submission_payload
                .get("pull_request")
                .and_then(|row| row.get("number"))
                .and_then(Value::as_u64)
            {
                extra.insert("pull_request_number".to_string(), json!(number));
            }
            extra
        });
    }
    let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
        "submitted": submission_payload
            .get("submitted")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "dry_run": dry_run,
        "submitted_github_ref": submission_payload
            .get("submitted_github_ref")
            .cloned()
            .unwrap_or(Value::Null),
        "pull_request": submission_payload
            .get("pull_request")
            .cloned()
            .unwrap_or(Value::Null),
        "follow_on_runs": submission_payload
            .get("follow_on_runs")
            .cloned()
            .unwrap_or_else(|| json!([])),
        "spawned_follow_on_runs": submission_payload
            .get("spawned_follow_on_runs")
            .cloned()
            .unwrap_or_else(|| json!([])),
        "skipped_follow_on_runs": submission_payload
            .get("skipped_follow_on_runs")
            .cloned()
            .unwrap_or_else(|| json!([])),
        "duplicate_linkage_candidate": duplicate_linkage_candidate,
        "external_action": external_action,
        "coder_run": coder_run_payload(&record, &run),
        "run": run,
    })))
}

pub(super) async fn coder_merge_submit(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderMergeSubmitInput>,
) -> Result<Json<Value>, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::MergeRecommendation) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let approved_by = input
        .approved_by
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let submit_mode = input
        .submit_mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("manual")
        .to_ascii_lowercase();
    if !matches!(submit_mode.as_str(), "manual" | "auto") {
        return Err(StatusCode::BAD_REQUEST);
    }
    if submit_mode == "auto" {
        if let Some(policy) = merge_submit_auto_mode_policy_block(&record) {
            return Ok(Json(json!({
                "ok": false,
                "code": "CODER_MERGE_SUBMIT_POLICY_BLOCKED",
                "policy": policy,
            })));
        }
    }
    let readiness = coder_merge_submit_readiness(&state, input.mcp_server.as_deref()).await?;
    if !readiness.runnable {
        return Ok(Json(json!({
            "ok": false,
            "code": "CODER_MERGE_SUBMIT_BLOCKED",
            "readiness": readiness,
        })));
    }
    let merge_request_payload =
        load_latest_coder_artifact_payload(&state, &record, "coder_merge_execution_request")
            .await
            .ok_or(StatusCode::CONFLICT)?;
    if let Some(policy) = merge_submit_request_readiness_block(&merge_request_payload) {
        return Ok(Json(json!({
            "ok": false,
            "code": "CODER_MERGE_SUBMIT_POLICY_BLOCKED",
            "policy": policy,
        })));
    }
    if let Some(review_policy) = merge_submit_review_policy_block(&state, &record).await? {
        return Ok(Json(json!({
            "ok": false,
            "code": "CODER_MERGE_SUBMIT_POLICY_BLOCKED",
            "policy": review_policy,
        })));
    }
    let github_ref = record.github_ref.clone().ok_or(StatusCode::CONFLICT)?;
    if !matches!(github_ref.kind, CoderGithubRefKind::PullRequest) {
        return Err(StatusCode::CONFLICT);
    }
    let dry_run = input.dry_run.unwrap_or(true);
    let (owner, repo_name) = split_owner_repo(&record.repo_binding.repo_slug)?;
    let mut submission_payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "approved_by": approved_by,
        "approval_reason": input.reason,
        "submit_mode": submit_mode,
        "dry_run": dry_run,
        "owner": owner,
        "repo": repo_name,
        "pull_number": github_ref.number,
        "merge_execution_request": merge_request_payload,
        "worker_run_reference": merge_request_payload.get("worker_run_reference").cloned().unwrap_or(Value::Null),
        "worker_session_id": merge_request_payload.get("worker_session_id").cloned().unwrap_or(Value::Null),
        "worker_session_run_id": merge_request_payload.get("worker_session_run_id").cloned().unwrap_or(Value::Null),
        "worker_session_context_run_id": merge_request_payload.get("worker_session_context_run_id").cloned().unwrap_or(Value::Null),
        "validation_run_reference": merge_request_payload.get("validation_run_reference").cloned().unwrap_or(Value::Null),
        "validation_session_id": merge_request_payload.get("validation_session_id").cloned().unwrap_or(Value::Null),
        "validation_session_run_id": merge_request_payload.get("validation_session_run_id").cloned().unwrap_or(Value::Null),
        "validation_session_context_run_id": merge_request_payload.get("validation_session_context_run_id").cloned().unwrap_or(Value::Null),
        "merged_github_ref": Value::Null,
        "created_at_ms": crate::now_ms(),
        "readiness": readiness,
    });
    let mut external_action = Value::Null;
    if !dry_run {
        let (server_name, tool_name) =
            resolve_github_merge_pr_tool(&state, input.mcp_server.as_deref()).await?;
        let result = call_merge_pull_request(
            &state,
            &server_name,
            &tool_name,
            owner,
            repo_name,
            github_ref.number,
        )
        .await?;
        let merge_result = extract_merge_result_from_tool_result(&result);
        if let Some(obj) = submission_payload.as_object_mut() {
            obj.insert("server_name".to_string(), json!(server_name));
            obj.insert("tool_name".to_string(), json!(tool_name));
            obj.insert("submitted".to_string(), json!(true));
            obj.insert("merged_github_ref".to_string(), json!(github_ref));
            obj.insert("merge_result".to_string(), merge_result);
            obj.insert(
                "tool_result".to_string(),
                json!({
                    "output": result.output,
                    "metadata": result.metadata,
                }),
            );
        }
        let idempotency_key = crate::sha256_hex(&[&format!(
            "{}|{}|{}|{}",
            record.repo_binding.repo_slug, github_ref.number, submit_mode, approved_by
        )]);
        if let Some(action) = record_coder_external_action(
            &state,
            &record,
            "merge_pull_request",
            "github.merge_pull_request",
            submission_payload
                .get("server_name")
                .and_then(Value::as_str)
                .unwrap_or("github"),
            &record.repo_binding.repo_slug,
            &idempotency_key,
            json!({
                "merged_github_ref": submission_payload
                    .get("merged_github_ref")
                    .cloned()
                    .unwrap_or(Value::Null),
                "merge_result": submission_payload
                    .get("merge_result")
                    .cloned()
                    .unwrap_or(Value::Null),
            }),
            json!({
                "workflow_mode": record.workflow_mode,
                "submit_mode": submit_mode,
                "approved_by": approved_by,
            }),
        )
        .await
        {
            external_action = serde_json::to_value(&action).unwrap_or(Value::Null);
        }
    } else if let Some(obj) = submission_payload.as_object_mut() {
        obj.insert("submitted".to_string(), json!(false));
        obj.insert(
            "dry_run_preview".to_string(),
            json!({
                "owner": owner,
                "repo": repo_name,
                "pull_number": github_ref.number,
            }),
        );
    }
    if let Some(obj) = submission_payload.as_object_mut() {
        obj.insert("external_action".to_string(), external_action.clone());
    }
    let artifact = write_coder_artifact(
        &state,
        &record.linked_context_run_id,
        &format!("merge-submit-{}", Uuid::new_v4().simple()),
        "coder_merge_submission",
        "artifacts/merge_recommendation.merge_submission.json",
        &submission_payload,
    )
    .await?;
    publish_coder_artifact_added(&state, &record, &artifact, Some("approval"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("merge_submission"));
        extra.insert("dry_run".to_string(), json!(dry_run));
        extra.insert(
            "submitted".to_string(),
            json!(submission_payload
                .get("submitted")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        extra
    });
    if !dry_run {
        publish_coder_run_event(
            &state,
            "coder.merge.submitted",
            &record,
            Some("approval"),
            {
                let mut extra = serde_json::Map::new();
                extra.insert("artifact_id".to_string(), json!(artifact.id));
                extra.insert(
                    "merged_github_ref".to_string(),
                    submission_payload
                        .get("merged_github_ref")
                        .cloned()
                        .unwrap_or(Value::Null),
                );
                extra.insert(
                    "submit_mode".to_string(),
                    submission_payload
                        .get("submit_mode")
                        .cloned()
                        .unwrap_or_else(|| json!("manual")),
                );
                extra
            },
        );
    }
    let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
        "submitted": submission_payload
            .get("submitted")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "dry_run": dry_run,
        "worker_run_reference": submission_payload.get("worker_run_reference").cloned().unwrap_or(Value::Null),
        "worker_session_id": submission_payload.get("worker_session_id").cloned().unwrap_or(Value::Null),
        "worker_session_run_id": submission_payload.get("worker_session_run_id").cloned().unwrap_or(Value::Null),
        "worker_session_context_run_id": submission_payload.get("worker_session_context_run_id").cloned().unwrap_or(Value::Null),
        "validation_run_reference": submission_payload.get("validation_run_reference").cloned().unwrap_or(Value::Null),
        "validation_session_id": submission_payload.get("validation_session_id").cloned().unwrap_or(Value::Null),
        "validation_session_run_id": submission_payload.get("validation_session_run_id").cloned().unwrap_or(Value::Null),
        "validation_session_context_run_id": submission_payload.get("validation_session_context_run_id").cloned().unwrap_or(Value::Null),
        "merged_github_ref": submission_payload
            .get("merged_github_ref")
            .cloned()
            .unwrap_or(Value::Null),
        "merge_result": submission_payload
            .get("merge_result")
            .cloned()
            .unwrap_or(Value::Null),
        "external_action": external_action,
        "coder_run": coder_run_payload(&record, &run),
        "run": run,
    })))
}

pub(super) async fn coder_follow_on_run_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderFollowOnRunCreateInput>,
) -> Result<Response, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !matches!(
        input.workflow_mode,
        CoderWorkflowMode::PrReview | CoderWorkflowMode::MergeRecommendation
    ) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let submission_payload =
        load_latest_coder_artifact_payload(&state, &record, "coder_pr_submission")
            .await
            .ok_or(StatusCode::CONFLICT)?;
    let submitted_github_ref = submission_payload
        .get("submitted_github_ref")
        .and_then(parse_coder_github_ref)
        .ok_or(StatusCode::CONFLICT)?;
    if !matches!(submitted_github_ref.kind, CoderGithubRefKind::PullRequest) {
        return Err(StatusCode::CONFLICT);
    }
    let follow_on_workflow_mode = input.workflow_mode.clone();
    let create_input = CoderRunCreateInput {
        coder_run_id: input.coder_run_id,
        ..build_follow_on_run_create_input(
            &record,
            follow_on_workflow_mode.clone(),
            submitted_github_ref,
            normalize_source_client(input.source_client.as_deref())
                .or_else(|| record.source_client.clone()),
            normalize_source_client(input.model_provider.as_deref())
                .or_else(|| record.model_provider.clone()),
            normalize_source_client(input.model_id.as_deref()).or_else(|| record.model_id.clone()),
            input
                .mcp_servers
                .or_else(|| Some(vec!["github".to_string()])),
            Some(record.coder_run_id.clone()),
            Some("issue_fix_pr_submit_manual_follow_on".to_string()),
            Some("coder_pr_submission".to_string()),
            Some(json!({
                "source": "issue_fix_pr_submit",
                "spawn_mode": "manual",
                "merge_auto_spawn_opted_in": submission_payload
                    .get("allow_auto_merge_recommendation")
                    .cloned()
                    .unwrap_or_else(|| json!(false)),
                "requested_follow_on_runs": submission_payload
                    .get("requested_spawn_follow_on_runs")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
                "effective_auto_spawn_runs": submission_payload
                    .get("spawned_follow_on_runs")
                    .and_then(Value::as_array)
                    .map(|rows| {
                        rows.iter()
                            .filter_map(|row| row.get("coder_run"))
                            .filter_map(|row| row.get("workflow_mode"))
                            .cloned()
                            .collect::<Vec<_>>()
                    })
                    .map(Value::from)
                    .unwrap_or_else(|| json!([])),
                "skipped_follow_on_runs": submission_payload
                    .get("skipped_follow_on_runs")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
                "required_completed_workflow_modes": if matches!(
                    follow_on_workflow_mode,
                    CoderWorkflowMode::MergeRecommendation
                ) {
                    json!(["pr_review"])
                } else {
                    json!([])
                },
            })),
        )
    };
    coder_run_create(State(state), Json(create_input)).await
}
