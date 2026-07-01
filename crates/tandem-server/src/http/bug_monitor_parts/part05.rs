async fn persist_bug_monitor_failure_pattern_from_approved_draft(
    state: &AppState,
    draft: &BugMonitorDraftRecord,
) -> Result<Value, StatusCode> {
    let summary_text = draft
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            draft
                .detail
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("Bug Monitor approved a failure draft without triage details.")
        .to_string();
    let detail = draft.detail.as_deref().unwrap_or_default();
    let canonical_markers = detail
        .lines()
        .filter_map(normalize_issue_draft_line)
        .take(5)
        .collect::<Vec<_>>();
    let duplicate_matches = super::coder::find_failure_pattern_duplicates(
        state,
        &draft.repo,
        None,
        &["bug_monitor".to_string(), "default".to_string()],
        &format!("{summary_text} {detail}"),
        Some(&draft.fingerprint),
        3,
    )
    .await?;
    let recurrence_count =
        bug_monitor_failure_recurrence_count(state, &draft.repo, &draft.fingerprint).await;
    let linked_issue_numbers = bug_monitor_linked_issue_numbers(draft);
    if duplicate_matches.iter().any(|row| {
        row.get("source").and_then(Value::as_str) == Some("governed_memory")
            && row.get("match_reason").and_then(Value::as_str) == Some("exact_fingerprint")
    }) {
        let duplicate_summary = build_bug_monitor_duplicate_summary(&duplicate_matches);
        return Ok(json!({
            "stored": false,
            "reason": "governed_failure_pattern_exists",
            "fingerprint": draft.fingerprint,
            "duplicate_summary": duplicate_summary,
            "duplicate_matches": duplicate_matches,
        }));
    }

    let run_id = format!("bug-monitor-approval-{}", draft.draft_id);
    ensure_context_run_dir(state, &run_id).await?;
    let approval_artifact_path =
        context_run_dir(state, &run_id).join("artifacts/bug_monitor.approval_failure_pattern.json");
    if let Some(parent) = approval_artifact_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let approval_artifact_payload = json!({
        "draft_id": draft.draft_id,
        "repo": draft.repo,
        "fingerprint": draft.fingerprint,
        "summary": summary_text.clone(),
        "detail": detail,
        "canonical_markers": canonical_markers.clone(),
        "created_at_ms": crate::now_ms(),
        "source": "bug_monitor_approval",
    });
    let approval_artifact_raw = serde_json::to_string_pretty(&approval_artifact_payload)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(&approval_artifact_path, approval_artifact_raw)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let approval_artifact_path = approval_artifact_path.to_string_lossy().to_string();
    let partition = MemoryPartition {
        org_id: draft.repo.clone(),
        workspace_id: draft.repo.clone(),
        project_id: draft.repo.clone(),
        tier: GovernedMemoryTier::Session,
    };
    let capability = Some(super::skills_memory::issue_run_memory_capability(
        &run_id,
        Some("bug_monitor"),
        &partition,
        super::skills_memory::RunMemoryCapabilityPolicy::CoderWorkflow,
    ));
    let metadata = json!({
        "kind": "failure_pattern",
        "repo_slug": draft.repo,
        "failure_pattern_fingerprint": draft.fingerprint,
        "linked_issue_numbers": linked_issue_numbers,
        "recurrence_count": recurrence_count,
        "affected_components": [draft
            .repo
            .rsplit('/')
            .next()
            .unwrap_or(draft.repo.as_str())],
        "artifact_refs": [approval_artifact_path],
        "canonical_markers": canonical_markers,
        "symptoms": [summary_text],
        "draft_id": draft.draft_id,
        "source": "bug_monitor_approval",
    });
    let tenant_context = tandem_types::TenantContext::local_implicit();
    let put_response = super::skills_memory::memory_put_impl(
        state,
        &tenant_context,
        MemoryPutRequest {
            run_id,
            partition: partition.clone(),
            kind: MemoryContentKind::Fact,
            content: summary_text.clone(),
            artifact_refs: vec![approval_artifact_path],
            classification: MemoryClassification::Internal,
            authority_job_context: None,
            metadata: Some(metadata.clone()),
        },
        capability,
    )
    .await?;
    Ok(json!({
        "stored": true,
        "memory_id": put_response.id,
        "fingerprint": draft.fingerprint,
        "content": summary_text,
        "duplicate_summary": build_bug_monitor_duplicate_summary(&duplicate_matches),
        "metadata": metadata,
        "partition": {
            "org_id": partition.org_id,
            "workspace_id": partition.workspace_id,
            "project_id": partition.project_id,
            "tier": partition.tier,
        },
        "duplicate_matches": duplicate_matches,
    }))
}

async fn latest_bug_monitor_incident_for_draft(
    state: &AppState,
    draft_id: &str,
) -> Option<crate::BugMonitorIncidentRecord> {
    state
        .bug_monitor_incidents
        .read()
        .await
        .values()
        .filter(|row| row.draft_id.as_deref() == Some(draft_id))
        .max_by_key(|row| row.updated_at_ms)
        .cloned()
}

fn latest_bug_monitor_artifact(
    state: &AppState,
    triage_run_id: &str,
    artifact_type: &str,
) -> Option<ContextBlackboardArtifact> {
    let blackboard = super::context_runs::load_context_blackboard(state, triage_run_id);
    blackboard
        .artifacts
        .iter()
        .filter(|row| row.artifact_type == artifact_type)
        .max_by_key(|row| row.ts_ms)
        .cloned()
}

async fn load_bug_monitor_artifact_payload(
    state: &AppState,
    triage_run_id: &str,
    artifact_type: &str,
) -> Option<(ContextBlackboardArtifact, Value)> {
    let artifact = latest_bug_monitor_artifact(state, triage_run_id, artifact_type)?;
    let raw = tokio::fs::read_to_string(&artifact.path).await.ok()?;
    let payload = serde_json::from_str::<Value>(&raw).ok()?;
    Some((artifact, payload))
}

pub(crate) async fn load_bug_monitor_triage_summary_artifact(
    state: &AppState,
    triage_run_id: &str,
) -> Option<Value> {
    load_bug_monitor_artifact_payload(state, triage_run_id, "bug_monitor_triage_summary")
        .await
        .map(|(_, payload)| payload)
}

pub(crate) async fn bug_monitor_failure_pattern_matches(
    state: &AppState,
    repo_slug: &str,
    fingerprint: &str,
    title: Option<&str>,
    detail: Option<&str>,
    excerpt: &[String],
    limit: usize,
) -> Vec<Value> {
    let mut rows = super::coder::query_failure_pattern_matches(
        state,
        repo_slug,
        fingerprint,
        title,
        detail,
        excerpt,
        limit,
    )
    .await
    .unwrap_or_default();
    for row in rows.iter_mut() {
        let source_missing = row.get("source").and_then(Value::as_str).is_none();
        let is_memory_candidate = row
            .get("candidate_id")
            .and_then(Value::as_str)
            .is_some_and(|value| value.starts_with("memcand-"));
        if source_missing && is_memory_candidate {
            if let Some(object) = row.as_object_mut() {
                object.insert(
                    "source".to_string(),
                    Value::String("coder_candidate".to_string()),
                );
            }
        }
    }
    rows
}

pub(crate) fn build_bug_monitor_duplicate_summary(matches: &[Value]) -> Value {
    let normalized_matches = matches
        .iter()
        .map(|row| {
            let candidate_id = row.get("candidate_id").cloned().unwrap_or(Value::Null);
            let source = row.get("source").cloned().or_else(|| {
                candidate_id
                    .as_str()
                    .filter(|value| value.starts_with("memcand-"))
                    .map(|_| Value::String("coder_candidate".to_string()))
            });
            json!({
                "source": source.unwrap_or(Value::Null),
                "fingerprint": row.get("fingerprint").cloned().unwrap_or(Value::Null),
                "summary": row.get("summary").cloned().unwrap_or(Value::Null),
                "match_reason": row
                    .get("match_reason")
                    .cloned()
                    .or_else(|| {
                        row.get("match_reasons")
                            .and_then(Value::as_array)
                            .and_then(|reasons| reasons.first().cloned())
                    })
                    .unwrap_or(Value::Null),
                "score": row.get("score").cloned().unwrap_or(Value::Null),
                "recurrence_count": row.get("recurrence_count").cloned().unwrap_or_else(|| Value::from(1_u64)),
                "linked_issue_numbers": row.get("linked_issue_numbers").cloned().unwrap_or_else(|| json!([])),
                "run_id": row.get("run_id").cloned().unwrap_or(Value::Null),
                "memory_id": row.get("memory_id").cloned().unwrap_or(Value::Null),
                "artifact_refs": row.get("artifact_refs").cloned().unwrap_or_else(|| json!([])),
                "artifact_path": row.get("artifact_path").cloned().unwrap_or(Value::Null),
                "candidate_id": candidate_id,
                "linked_context_run_id": row
                    .get("linked_context_run_id")
                    .cloned()
                    .unwrap_or(Value::Null),
                "source_coder_run_id": row
                    .get("source_coder_run_id")
                    .cloned()
                    .unwrap_or_else(|| row.get("coder_run_id").cloned().unwrap_or(Value::Null)),
            })
        })
        .collect::<Vec<_>>();
    let best_match = normalized_matches.first().cloned().unwrap_or(Value::Null);
    let max_recurrence_count = normalized_matches
        .iter()
        .filter_map(|row| row.get("recurrence_count").and_then(Value::as_u64))
        .max()
        .unwrap_or(1);
    let mut linked_issue_numbers = normalized_matches
        .iter()
        .filter_map(|row| row.get("linked_issue_numbers").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_u64)
        .collect::<Vec<_>>();
    linked_issue_numbers.sort_unstable();
    linked_issue_numbers.dedup();
    json!({
        "reason": "duplicate_failure_pattern",
        "match_count": normalized_matches.len(),
        "max_recurrence_count": max_recurrence_count,
        "linked_issue_numbers": linked_issue_numbers,
        "best_match": best_match,
        "matches": normalized_matches,
    })
}

pub(crate) async fn load_incident_monitor_issue_draft_artifact(
    state: &AppState,
    triage_run_id: &str,
) -> Option<Value> {
    load_bug_monitor_artifact_payload(state, triage_run_id, "incident_monitor_issue_draft")
        .await
        .map(|(_, payload)| payload)
}

pub(crate) async fn load_bug_monitor_proposal_quality_gate_artifact(
    state: &AppState,
    triage_run_id: &str,
) -> Option<Value> {
    load_bug_monitor_artifact_payload(state, triage_run_id, "bug_monitor_proposal_quality_gate")
        .await
        .map(|(_, payload)| payload)
}

fn bug_monitor_triage_artifacts(
    state: &AppState,
    triage_run_id: Option<&str>,
) -> (
    Option<ContextBlackboardArtifact>,
    Option<ContextBlackboardArtifact>,
    Option<ContextBlackboardArtifact>,
) {
    let triage_summary_artifact = triage_run_id.and_then(|triage_run_id| {
        latest_bug_monitor_artifact(state, triage_run_id, "bug_monitor_triage_summary")
    });
    let issue_draft_artifact = triage_run_id.and_then(|triage_run_id| {
        latest_bug_monitor_artifact(state, triage_run_id, "incident_monitor_issue_draft")
    });
    let duplicate_matches_artifact = triage_run_id.and_then(|triage_run_id| {
        latest_bug_monitor_artifact(state, triage_run_id, "failure_duplicate_matches")
    });
    (
        triage_summary_artifact,
        issue_draft_artifact,
        duplicate_matches_artifact,
    )
}

async fn bug_monitor_duplicate_match_context(
    state: &AppState,
    triage_run_id: Option<&str>,
) -> (Option<Value>, Option<Value>) {
    let Some(triage_run_id) = triage_run_id else {
        return (None, None);
    };
    let duplicate_matches =
        load_bug_monitor_artifact_payload(state, triage_run_id, "failure_duplicate_matches")
            .await
            .and_then(|(_, payload)| {
                payload
                    .get("matches")
                    .and_then(Value::as_array)
                    .cloned()
                    .map(Value::Array)
            });
    let duplicate_rows = duplicate_matches
        .as_ref()
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let duplicate_summary = Some(build_bug_monitor_duplicate_summary(&duplicate_rows));
    (duplicate_summary, Some(Value::Array(duplicate_rows)))
}

async fn refresh_bug_monitor_duplicate_matches_artifact(
    state: &AppState,
    draft: &BugMonitorDraftRecord,
    triage_run_id: &str,
) -> Option<Vec<Value>> {
    if latest_bug_monitor_artifact(state, triage_run_id, "failure_duplicate_matches").is_some() {
        return None;
    }
    let duplicate_matches = bug_monitor_failure_pattern_matches(
        state,
        &draft.repo,
        &draft.fingerprint,
        draft.title.as_deref(),
        draft.detail.as_deref(),
        &[],
        3,
    )
    .await;
    if duplicate_matches.is_empty() {
        return None;
    }
    write_bug_monitor_artifact(
        state,
        triage_run_id,
        "failure-duplicate-matches",
        "failure_duplicate_matches",
        "artifacts/failure_duplicate_matches.json",
        &json!({
            "draft_id": draft.draft_id,
            "repo": draft.repo,
            "fingerprint": draft.fingerprint,
            "matches": duplicate_matches,
            "created_at_ms": crate::now_ms(),
        }),
    )
    .await
    .ok()?;
    Some(duplicate_matches)
}

pub(crate) async fn ensure_incident_monitor_issue_draft(
    state: AppState,
    draft_id: &str,
    force: bool,
) -> anyhow::Result<Value> {
    let config = state.bug_monitor_config().await;
    let mut draft = state
        .get_bug_monitor_draft(draft_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Bug Monitor draft not found"))?;
    let triage_run_id = draft.triage_run_id.clone().ok_or_else(|| {
        anyhow::anyhow!("Bug Monitor draft needs a triage run before issue drafting")
    })?;
    let triage_summary = load_bug_monitor_triage_summary_artifact(&state, &triage_run_id).await;
    let (proposal_gate_passed, proposal_quality_gate) =
        bug_monitor_proposal_quality_gate(&state, &triage_run_id, triage_summary.as_ref());
    if !proposal_gate_passed {
        let _ = write_bug_monitor_artifact(
            &state,
            &triage_run_id,
            "bug-monitor-proposal-quality-gate",
            "bug_monitor_proposal_quality_gate",
            "artifacts/bug_monitor.proposal_quality_gate.json",
            &proposal_quality_gate,
        )
        .await;
        draft.github_status = Some("proposal_blocked".to_string());
        draft.last_post_error = Some(
            "Bug Monitor draft-to-proposal quality gate blocked issue draft generation".to_string(),
        );
        let _ = state.put_bug_monitor_draft(draft).await;
        anyhow::bail!(
            "Bug Monitor draft-to-proposal quality gate blocked issue draft generation: {}",
            proposal_quality_gate
        );
    }
    if !force {
        let existing_issue_draft = load_bug_monitor_artifact_payload(
            &state,
            &triage_run_id,
            "incident_monitor_issue_draft",
        )
        .await;
        let triage_summary =
            load_bug_monitor_artifact_payload(&state, &triage_run_id, "bug_monitor_triage_summary")
                .await;
        if let Some((issue_artifact, issue_payload)) = existing_issue_draft {
            let triage_newer = triage_summary
                .as_ref()
                .map(|(summary_artifact, _)| summary_artifact.ts_ms > issue_artifact.ts_ms)
                .unwrap_or(false);
            if !triage_newer {
                return Ok(issue_payload);
            }
        }
    }

    let incident = latest_bug_monitor_incident_for_draft(&state, draft_id).await;
    let (template, template_source) = load_bug_monitor_issue_template(&config).await;
    let what_happened = triage_summary
        .as_ref()
        .and_then(|row| row.get("what_happened"))
        .and_then(Value::as_str)
        .and_then(normalize_issue_draft_line)
        .or_else(|| {
            draft
                .detail
                .as_deref()
                .and_then(normalize_issue_draft_line)
                .or_else(|| {
                    incident
                        .as_ref()
                        .and_then(|row| normalize_issue_draft_line(&row.title))
                })
                .or_else(|| draft.title.as_deref().and_then(normalize_issue_draft_line))
        })
        .unwrap_or_else(|| "Bug Monitor detected a failure that needs triage.".to_string());
    let expected_behavior = triage_summary
        .as_ref()
        .and_then(|row| row.get("expected_behavior"))
        .and_then(Value::as_str)
        .and_then(normalize_issue_draft_line)
        .unwrap_or_else(|| derive_expected_behavior(&draft, incident.as_ref()));
    let steps_to_reproduce = triage_summary
        .as_ref()
        .and_then(|row| row.get("steps_to_reproduce"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .filter_map(normalize_issue_draft_line)
                .collect::<Vec<_>>()
        })
        .filter(|rows| !rows.is_empty())
        .unwrap_or_else(|| derive_steps_to_reproduce(&draft, incident.as_ref()));
    let environment_lines = triage_summary
        .as_ref()
        .and_then(|row| row.get("environment"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .filter_map(normalize_issue_draft_line)
                .collect::<Vec<_>>()
        })
        .filter(|rows| !rows.is_empty())
        .unwrap_or_else(|| derive_environment_lines(&draft, incident.as_ref()));
    let log_lines = triage_summary
        .as_ref()
        .and_then(|row| row.get("logs"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .filter_map(normalize_issue_draft_line)
                .collect::<Vec<_>>()
        })
        .filter(|rows| !rows.is_empty())
        .unwrap_or_else(|| derive_log_lines(&draft, incident.as_ref()));
    let string_array = |key: &str| -> Vec<String> {
        triage_summary
            .as_ref()
            .and_then(|row| row.get(key))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .filter_map(normalize_issue_draft_line)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let likely_files_to_edit = string_array("likely_files_to_edit");
    let affected_components = string_array("affected_components");
    let acceptance_criteria = string_array("acceptance_criteria");
    let verification_steps = string_array("verification_steps");
    let research_sources = triage_summary
        .as_ref()
        .and_then(|row| row.get("research_sources"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let file_references = triage_summary
        .as_ref()
        .and_then(|row| row.get("file_references"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let fix_points = triage_summary
        .as_ref()
        .and_then(|row| row.get("fix_points"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let related_existing_issues = triage_summary
        .as_ref()
        .and_then(|row| row.get("related_existing_issues"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let duplicate_failure_patterns =
        load_bug_monitor_artifact_payload(&state, &triage_run_id, "failure_duplicate_matches")
            .await
            .and_then(|(_, payload)| payload.get("matches").and_then(Value::as_array).cloned())
            .unwrap_or_default();
    let related_failure_patterns = triage_summary
        .as_ref()
        .and_then(|row| row.get("related_failure_patterns"))
        .and_then(Value::as_array)
        .cloned()
        .filter(|rows| !rows.is_empty())
        .or_else(|| {
            if duplicate_failure_patterns.is_empty() {
                None
            } else {
                Some(duplicate_failure_patterns)
            }
        })
        .unwrap_or_default();
    let why_it_likely_happened = triage_summary
        .as_ref()
        .and_then(|row| row.get("why_it_likely_happened"))
        .and_then(Value::as_str)
        .and_then(normalize_issue_draft_line)
        .unwrap_or_default();
    let recommended_fix = triage_summary
        .as_ref()
        .and_then(|row| row.get("recommended_fix"))
        .and_then(Value::as_str)
        .and_then(normalize_issue_draft_line)
        .unwrap_or_default();
    let failure_type = triage_summary
        .as_ref()
        .and_then(|row| row.get("failure_type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let root_cause_confidence = triage_summary
        .as_ref()
        .and_then(|row| row.get("root_cause_confidence"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("low")
        .to_string();
    let risk_level = triage_summary
        .as_ref()
        .and_then(|row| row.get("risk_level"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("medium")
        .to_string();
    let required_tool_scopes = string_array("required_tool_scopes");
    let missing_tool_scopes = string_array("missing_tool_scopes");
    let permissions_available = triage_summary
        .as_ref()
        .and_then(|row| row.get("permissions_available"))
        .and_then(Value::as_bool);
    let requested_coder_ready = triage_summary
        .as_ref()
        .and_then(|row| row.get("coder_ready"))
        .and_then(Value::as_bool);
    let duplicate_known = triage_summary
        .as_ref()
        .and_then(|row| row.get("duplicate"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let (coder_ready, coder_ready_gate) = bug_monitor_coder_ready_gate(
        requested_coder_ready,
        &root_cause_confidence,
        &likely_files_to_edit,
        &affected_components,
        &acceptance_criteria,
        &verification_steps,
        &risk_level,
        duplicate_known,
        &required_tool_scopes,
        &missing_tool_scopes,
        permissions_available,
    );
    let list_section = |items: &[String]| -> String {
        items
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let json_list_section = |items: &[Value]| -> String {
        items
            .iter()
            .map(|item| {
                let text = item
                    .as_str()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| item.to_string());
                format!("- {text}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let extra_sections = vec![
        (
            "Suspected root cause".to_string(),
            why_it_likely_happened.clone(),
        ),
        ("Recommended fix".to_string(), recommended_fix.clone()),
        (
            "Files likely involved".to_string(),
            list_section(&likely_files_to_edit),
        ),
        (
            "Affected components".to_string(),
            list_section(&affected_components),
        ),
        (
            "Acceptance criteria".to_string(),
            list_section(&acceptance_criteria),
        ),
        (
            "Verification steps".to_string(),
            list_section(&verification_steps),
        ),
        (
            "Related issues and failure patterns".to_string(),
            [
                json_list_section(&related_existing_issues),
                json_list_section(&related_failure_patterns),
            ]
            .into_iter()
            .filter(|row| !row.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        ),
        (
            "Research sources".to_string(),
            json_list_section(&research_sources),
        ),
        (
            "File references".to_string(),
            json_list_section(&file_references),
        ),
        (
            "Potential fix points".to_string(),
            json_list_section(&fix_points),
        ),
    ];
    let handoff = json!({
        "handoff_type": "tandem_autonomous_coder_issue",
        "source": "bug_monitor",
        "repo": draft.repo.clone(),
        "triage_run_id": triage_run_id.clone(),
        "workflow_run_id": incident.as_ref().and_then(|row| row.run_id.clone()),
        "incident_id": incident.as_ref().map(|row| row.incident_id.clone()),
        "draft_id": draft.draft_id.clone(),
        "failure_type": failure_type.clone(),
        "likely_files_to_edit": likely_files_to_edit.clone(),
        "acceptance_criteria": acceptance_criteria.clone(),
        "verification_steps": verification_steps.clone(),
        "risk_level": risk_level.clone(),
        "coder_ready": coder_ready,
        "coder_ready_gate": coder_ready_gate.clone(),
        "required_tool_scopes": required_tool_scopes.clone(),
        "missing_tool_scopes": missing_tool_scopes.clone(),
        "permissions_available": permissions_available,
    });
    let mut hidden_markers = vec![
        format!("<!-- tandem:fingerprint:v1:{} -->", draft.fingerprint),
        format!("<!-- tandem:triage_run_id:v1:{} -->", triage_run_id),
    ];
    if coder_ready {
        hidden_markers.push(format!(
            "<!-- tandem:coder_handoff:v1\n{}\n-->",
            serde_json::to_string_pretty(&handoff).unwrap_or_else(|_| "{}".to_string())
        ));
    }
    let rendered_body = render_bug_monitor_template(
        &template,
        &what_happened,
        &expected_behavior,
        &steps_to_reproduce,
        &environment_lines,
        &log_lines,
        &extra_sections,
        &hidden_markers,
    );
    let payload = json!({
        "draft_id": draft.draft_id,
        "repo": draft.repo,
        "triage_run_id": triage_run_id,
        "template_source": template_source,
        "suggested_title": triage_summary
            .as_ref()
            .and_then(|row| row.get("suggested_title"))
            .and_then(Value::as_str)
            .and_then(normalize_issue_draft_line)
            .or_else(|| draft.title.clone())
            .unwrap_or_else(|| "Bug Monitor issue".to_string()),
        "what_happened": what_happened,
        "expected_behavior": expected_behavior,
        "steps_to_reproduce": steps_to_reproduce,
        "environment": environment_lines,
        "logs": log_lines,
        "why_it_likely_happened": why_it_likely_happened,
        "root_cause_confidence": root_cause_confidence,
        "failure_type": failure_type,
        "affected_components": affected_components,
        "likely_files_to_edit": likely_files_to_edit,
        "related_existing_issues": related_existing_issues,
        "related_failure_patterns": related_failure_patterns,
        "research_sources": research_sources,
        "file_references": file_references,
        "fix_points": fix_points,
        "recommended_fix": recommended_fix,
        "acceptance_criteria": acceptance_criteria,
        "verification_steps": verification_steps,
        "coder_ready": coder_ready,
        "coder_ready_gate": coder_ready_gate,
        "proposal_quality_gate": proposal_quality_gate,
        "risk_level": risk_level,
        "required_tool_scopes": required_tool_scopes,
        "missing_tool_scopes": missing_tool_scopes,
        "permissions_available": permissions_available,
        "coder_handoff": handoff,
        "triage_summary": triage_summary,
        "rendered_body": rendered_body,
        "created_at_ms": crate::now_ms(),
    });
    let artifact_id = format!("bug-monitor-issue-draft-{}", Uuid::new_v4().simple());
    write_bug_monitor_artifact(
        &state,
        &triage_run_id,
        &artifact_id,
        "incident_monitor_issue_draft",
        "artifacts/bug_monitor.issue_draft.json",
        &payload,
    )
    .await
    .map_err(|status| anyhow::anyhow!("Failed to write issue draft artifact: HTTP {status}"))?;

    draft.github_status = Some("issue_draft_ready".to_string());
    if draft.status.eq_ignore_ascii_case("triage_queued") {
        draft.status = "draft_ready".to_string();
    }
    let _ = state.put_bug_monitor_draft(draft).await?;
    Ok(payload)
}
