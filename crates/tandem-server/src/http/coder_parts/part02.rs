
fn compare_coder_memory_hits(record: &CoderRunRecord, a: &Value, b: &Value) -> std::cmp::Ordering {
    let a_same_ref = a.get("same_ref").and_then(Value::as_bool).unwrap_or(false);
    let b_same_ref = b.get("same_ref").and_then(Value::as_bool).unwrap_or(false);
    let a_same_issue = a
        .get("same_issue")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let b_same_issue = b
        .get("same_issue")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let a_same_linked_issue = a
        .get("same_linked_issue")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let b_same_linked_issue = b
        .get("same_linked_issue")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let a_same_linked_pr = a
        .get("same_linked_pr")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let b_same_linked_pr = b
        .get("same_linked_pr")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let a_score = a.get("score").and_then(Value::as_f64).unwrap_or(0.0);
    let b_score = b.get("score").and_then(Value::as_f64).unwrap_or(0.0);
    let ref_order = b_same_ref
        .cmp(&a_same_ref)
        .then_with(|| b_same_issue.cmp(&a_same_issue))
        .then_with(|| b_same_linked_issue.cmp(&a_same_linked_issue))
        .then_with(|| b_same_linked_pr.cmp(&a_same_linked_pr));
    let kind_weight = |hit: &Value| match memory_hit_kind(hit).as_deref() {
        Some("failure_pattern")
            if matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage) =>
        {
            5_u8
        }
        Some("regression_signal")
            if matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage) =>
        {
            4_u8
        }
        Some("duplicate_linkage")
            if matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage) =>
        {
            3_u8
        }
        Some("triage_memory") if matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage) => {
            3_u8
        }
        Some("fix_pattern") if matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage) => {
            2_u8
        }
        Some("run_outcome")
            if matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage)
                && memory_hit_workflow_mode(hit).as_deref() == Some("issue_triage") =>
        {
            2_u8
        }
        Some("fix_pattern") if matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) => 4_u8,
        Some("validation_memory")
            if matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) =>
        {
            3_u8
        }
        Some("regression_signal")
            if matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) =>
        {
            3_u8
        }
        Some("run_outcome")
            if matches!(record.workflow_mode, CoderWorkflowMode::IssueFix)
                && memory_hit_workflow_mode(hit).as_deref() == Some("issue_fix") =>
        {
            2_u8
        }
        Some("triage_memory") if matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) => {
            1_u8
        }
        Some("duplicate_linkage")
            if matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) =>
        {
            3_u8
        }
        Some("merge_recommendation_memory")
            if matches!(record.workflow_mode, CoderWorkflowMode::MergeRecommendation) =>
        {
            4_u8
        }
        Some("review_memory")
            if matches!(record.workflow_mode, CoderWorkflowMode::MergeRecommendation) =>
        {
            3_u8
        }
        Some("run_outcome")
            if matches!(record.workflow_mode, CoderWorkflowMode::MergeRecommendation)
                && memory_hit_workflow_mode(hit).as_deref() == Some("merge_recommendation") =>
        {
            3_u8
        }
        Some("regression_signal")
            if matches!(record.workflow_mode, CoderWorkflowMode::MergeRecommendation) =>
        {
            2_u8
        }
        Some("review_memory") if matches!(record.workflow_mode, CoderWorkflowMode::PrReview) => {
            4_u8
        }
        Some("merge_recommendation_memory")
            if matches!(record.workflow_mode, CoderWorkflowMode::PrReview) =>
        {
            3_u8
        }
        Some("duplicate_linkage")
            if matches!(record.workflow_mode, CoderWorkflowMode::PrReview) =>
        {
            3_u8
        }
        Some("regression_signal")
            if matches!(record.workflow_mode, CoderWorkflowMode::PrReview) =>
        {
            3_u8
        }
        Some("duplicate_linkage")
            if matches!(record.workflow_mode, CoderWorkflowMode::MergeRecommendation) =>
        {
            2_u8
        }
        Some("run_outcome")
            if matches!(record.workflow_mode, CoderWorkflowMode::PrReview)
                && memory_hit_workflow_mode(hit).as_deref() == Some("pr_review") =>
        {
            2_u8
        }
        _ => 1_u8,
    };
    let structured_signal_weight = |hit: &Value| {
        let payload = hit
            .get("payload")
            .or_else(|| hit.get("metadata"))
            .cloned()
            .unwrap_or(Value::Null);
        let list_weight = |key: &str| {
            payload
                .get(key)
                .and_then(Value::as_array)
                .map(|rows| !rows.is_empty() as u8)
                .unwrap_or(0_u8)
        };
        match record.workflow_mode {
            CoderWorkflowMode::IssueTriage => {
                list_weight("regression_signals") + list_weight("observed_logs")
            }
            CoderWorkflowMode::IssueFix => {
                list_weight("validation_results") + list_weight("regression_signals")
            }
            CoderWorkflowMode::MergeRecommendation => {
                list_weight("blockers")
                    + list_weight("required_checks")
                    + list_weight("required_approvals")
            }
            CoderWorkflowMode::PrReview => {
                list_weight("blockers")
                    + list_weight("requested_changes")
                    + list_weight("regression_signals")
            }
        }
    };
    let governed_issue_fix_weight = |hit: &Value| {
        (matches!(record.workflow_mode, CoderWorkflowMode::IssueFix)
            && matches!(
                memory_hit_kind(hit).as_deref(),
                Some("fix_pattern") | Some("validation_memory") | Some("regression_signal")
            )
            && hit.get("source").and_then(Value::as_str) == Some("governed_memory")) as u8
    };
    let governed_issue_triage_weight = |hit: &Value| {
        (matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage)
            && matches!(
                memory_hit_kind(hit).as_deref(),
                Some("failure_pattern") | Some("regression_signal")
            )
            && hit.get("source").and_then(Value::as_str) == Some("governed_memory")) as u8
    };
    let governed_issue_triage_outcome_weight = |hit: &Value| {
        (matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage)
            && memory_hit_kind(hit).as_deref() == Some("run_outcome")
            && memory_hit_workflow_mode(hit).as_deref() == Some("issue_triage")
            && hit.get("source").and_then(Value::as_str) == Some("governed_memory")) as u8
    };
    let governed_pr_review_weight = |hit: &Value| {
        (matches!(record.workflow_mode, CoderWorkflowMode::PrReview)
            && memory_hit_kind(hit).as_deref() == Some("regression_signal")
            && hit.get("source").and_then(Value::as_str) == Some("governed_memory")) as u8
    };
    let governed_merge_weight = |hit: &Value| {
        (matches!(record.workflow_mode, CoderWorkflowMode::MergeRecommendation)
            && memory_hit_kind(hit).as_deref() == Some("run_outcome")
            && memory_hit_workflow_mode(hit).as_deref() == Some("merge_recommendation")
            && hit.get("source").and_then(Value::as_str) == Some("governed_memory")) as u8
    };
    let kind_order = kind_weight(b).cmp(&kind_weight(a));
    let structured_order = structured_signal_weight(b).cmp(&structured_signal_weight(a));
    let governed_issue_fix_order = governed_issue_fix_weight(b).cmp(&governed_issue_fix_weight(a));
    let governed_issue_triage_order =
        governed_issue_triage_weight(b).cmp(&governed_issue_triage_weight(a));
    let governed_issue_triage_outcome_order =
        governed_issue_triage_outcome_weight(b).cmp(&governed_issue_triage_outcome_weight(a));
    let governed_pr_review_order = governed_pr_review_weight(b).cmp(&governed_pr_review_weight(a));
    let governed_merge_order = governed_merge_weight(b).cmp(&governed_merge_weight(a));
    let score_order = || {
        b_score
            .partial_cmp(&a_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.get("created_at_ms")
                    .and_then(Value::as_u64)
                    .cmp(&a.get("created_at_ms").and_then(Value::as_u64))
            })
    };
    ref_order
        .then_with(|| governed_issue_triage_order)
        .then_with(|| governed_issue_triage_outcome_order)
        .then_with(|| governed_issue_fix_order)
        .then_with(|| governed_pr_review_order)
        .then_with(|| governed_merge_order)
        .then_with(|| kind_order)
        .then_with(|| structured_order)
        .then_with(score_order)
}

fn memory_hit_workflow_mode(hit: &Value) -> Option<String> {
    value_string(
        hit.get("payload")
            .and_then(|row| row.get("workflow_mode"))
            .or_else(|| hit.get("metadata").and_then(|row| row.get("workflow_mode"))),
    )
}

fn memory_hit_kind(hit: &Value) -> Option<String> {
    value_string(hit.get("kind"))
        .or_else(|| value_string(hit.get("metadata").and_then(|row| row.get("kind"))))
}

fn derive_failure_pattern_duplicate_matches(
    hits: &[Value],
    fingerprint: Option<&str>,
    limit: usize,
) -> Vec<Value> {
    let normalized_fingerprint = fingerprint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let mut duplicates = Vec::<Value>::new();
    let mut seen = HashSet::<String>::new();
    for hit in hits {
        let kind = memory_hit_kind(hit).unwrap_or_default();
        if kind != "failure_pattern" {
            continue;
        }
        let hit_fingerprint =
            value_string(hit.get("payload").and_then(|row| row.get("fingerprint"))).or_else(|| {
                value_string(
                    hit.get("metadata")
                        .and_then(|row| row.get("failure_pattern_fingerprint")),
                )
            });
        let exact_fingerprint =
            normalized_fingerprint.is_some() && normalized_fingerprint == hit_fingerprint;
        let score = hit.get("score").and_then(Value::as_f64).unwrap_or(0.0);
        if !exact_fingerprint && score <= 0.0 {
            continue;
        }
        let identity = value_string(hit.get("candidate_id"))
            .or_else(|| value_string(hit.get("memory_id")))
            .or_else(|| hit_fingerprint.clone())
            .unwrap_or_else(|| format!("failure-pattern-{}", duplicates.len()));
        if !seen.insert(identity) {
            continue;
        }
        duplicates.push(json!({
            "kind": "failure_pattern",
            "source": hit.get("source").cloned().unwrap_or(Value::Null),
            "match_reason": if exact_fingerprint { "exact_fingerprint" } else { "historical_failure_pattern" },
            "score": if exact_fingerprint { Value::from(1.0) } else { Value::from(score) },
            "fingerprint": hit_fingerprint,
            "summary": hit.get("summary").cloned().unwrap_or_else(|| hit.get("content").cloned().unwrap_or(Value::Null)),
            "linked_issue_numbers": hit
                .get("payload")
                .and_then(|row| row.get("linked_issue_numbers"))
                .cloned()
                .or_else(|| hit.get("metadata").and_then(|row| row.get("linked_issue_numbers")).cloned())
                .unwrap_or_else(|| Value::Array(Vec::new())),
            "recurrence_count": hit
                .get("payload")
                .and_then(|row| row.get("recurrence_count"))
                .cloned()
                .or_else(|| hit.get("metadata").and_then(|row| row.get("recurrence_count")).cloned())
                .unwrap_or_else(|| Value::from(1_u64)),
            "affected_components": hit
                .get("payload")
                .and_then(|row| row.get("affected_components"))
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
            "candidate_id": hit.get("candidate_id").cloned().unwrap_or(Value::Null),
            "memory_id": hit.get("memory_id").cloned().unwrap_or(Value::Null),
            "artifact_path": hit.get("path").cloned().unwrap_or(Value::Null),
            "run_id": hit.get("run_id").cloned().unwrap_or_else(|| hit.get("source_coder_run_id").cloned().unwrap_or(Value::Null)),
        }));
    }
    duplicates.sort_by(compare_failure_pattern_duplicate_matches);
    duplicates.truncate(limit.clamp(1, 8));
    duplicates
}

fn derive_duplicate_linkage_candidates_from_hits(hits: &[Value], limit: usize) -> Vec<Value> {
    let mut duplicates = Vec::<Value>::new();
    let mut seen_pr_numbers = HashSet::<u64>::new();
    for hit in hits {
        if memory_hit_kind(hit).as_deref() != Some("duplicate_linkage") {
            continue;
        }
        let linked_issue_numbers = candidate_linked_numbers(hit, "linked_issue_numbers");
        for number in candidate_linked_numbers(hit, "linked_pr_numbers") {
            if !seen_pr_numbers.insert(number) {
                continue;
            }
            duplicates.push(json!({
                "id": format!("duplicate-linkage-{number}"),
                "kind": "pull_request",
                "number": number,
                "summary": hit
                    .get("summary")
                    .cloned()
                    .or_else(|| hit.get("metadata").and_then(|row| row.get("summary")).cloned())
                    .unwrap_or_else(|| json!(format!("historical linked pull request #{number}"))),
                "linked_issue_numbers": linked_issue_numbers,
                "linked_pr_numbers": [number],
                "match_reason": "historical_duplicate_linkage",
                "source": hit.get("source").cloned().unwrap_or_else(|| json!("unknown")),
                "memory_id": hit.get("memory_id").cloned().unwrap_or(Value::Null),
                "candidate_id": hit.get("candidate_id").cloned().unwrap_or(Value::Null),
                "score": hit.get("score").cloned().unwrap_or(Value::Null),
                "same_ref": hit.get("same_ref").cloned().unwrap_or(Value::Null),
                "same_issue": hit.get("same_issue").cloned().unwrap_or(Value::Null),
                "same_linked_issue": hit.get("same_linked_issue").cloned().unwrap_or(Value::Null),
                "same_linked_pr": hit.get("same_linked_pr").cloned().unwrap_or(Value::Null),
            }));
        }
    }
    duplicates.sort_by(|a, b| {
        b.get("same_linked_issue")
            .and_then(Value::as_bool)
            .cmp(&a.get("same_linked_issue").and_then(Value::as_bool))
            .then_with(|| {
                b.get("score")
                    .and_then(Value::as_f64)
                    .partial_cmp(&a.get("score").and_then(Value::as_f64))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    duplicates.truncate(limit.clamp(1, 8));
    duplicates
}
fn default_coder_memory_query(record: &CoderRunRecord) -> String {
    match record.github_ref.as_ref() {
        Some(reference) if matches!(reference.kind, CoderGithubRefKind::PullRequest) => {
            match record.workflow_mode {
                CoderWorkflowMode::PrReview => format!(
                    "{} pull request #{} review regressions blockers requested changes",
                    record.repo_binding.repo_slug, reference.number
                ),
                CoderWorkflowMode::MergeRecommendation => format!(
                    "{} pull request #{} merge recommendation regressions blockers required checks approvals",
                    record.repo_binding.repo_slug, reference.number
                ),
                _ => format!(
                    "{} pull request #{}",
                    record.repo_binding.repo_slug, reference.number
                ),
            }
        }
        Some(reference) => format!(
            "{} issue #{}",
            record.repo_binding.repo_slug, reference.number
        ),
        None => record.repo_binding.repo_slug.clone(),
    }
}

fn value_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn triage_reproduction_outcome_failed(outcome: Option<&str>) -> bool {
    let Some(outcome) = outcome.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    matches!(
        outcome.to_ascii_lowercase().as_str(),
        "failed_to_reproduce" | "not_reproduced" | "inconclusive" | "error"
    )
}

fn merge_recommendation_promotion_allowed(candidate_payload: &Value) -> bool {
    let payload = candidate_payload
        .get("payload")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    ["blockers", "required_checks", "required_approvals"]
        .iter()
        .any(|field| {
            payload
                .get(*field)
                .and_then(Value::as_array)
                .is_some_and(|rows| !rows.is_empty())
        })
}

fn duplicate_linkage_promotion_allowed(candidate_payload: &Value) -> bool {
    let payload = candidate_payload.get("payload");
    payload
        .and_then(|row| row.get("linked_issue_numbers"))
        .and_then(Value::as_array)
        .is_some_and(|rows| !rows.is_empty())
        && payload
            .and_then(|row| row.get("linked_pr_numbers"))
            .and_then(Value::as_array)
            .is_some_and(|rows| !rows.is_empty())
}

fn regression_signal_promotion_allowed(candidate_payload: &Value) -> bool {
    let payload = candidate_payload.get("payload");
    payload
        .and_then(|row| row.get("regression_signals"))
        .and_then(Value::as_array)
        .is_some_and(|rows| !rows.is_empty())
        && [
            "summary_artifact_path",
            "review_evidence_artifact_path",
            "reproduction_artifact_path",
            "validation_artifact_path",
        ]
        .iter()
        .any(|key| {
            payload
                .and_then(|row| row.get(*key))
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        })
}

fn run_outcome_promotion_allowed(candidate_payload: &Value) -> bool {
    let payload = candidate_payload.get("payload");
    [
        "summary_artifact_path",
        "reproduction_artifact_path",
        "validation_artifact_path",
        "review_evidence_artifact_path",
        "readiness_artifact_path",
    ]
    .iter()
    .any(|field| {
        payload
            .and_then(|row| row.get(*field))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    })
}

fn coder_memory_candidate_promotion_allowed(
    kind: &CoderMemoryCandidateKind,
    candidate_payload: &Value,
) -> bool {
    match kind {
        CoderMemoryCandidateKind::MergeRecommendationMemory => {
            merge_recommendation_promotion_allowed(candidate_payload)
        }
        CoderMemoryCandidateKind::DuplicateLinkage => {
            duplicate_linkage_promotion_allowed(candidate_payload)
        }
        CoderMemoryCandidateKind::RegressionSignal => {
            regression_signal_promotion_allowed(candidate_payload)
        }
        CoderMemoryCandidateKind::RunOutcome => run_outcome_promotion_allowed(candidate_payload),
        _ => true,
    }
}

pub(crate) fn failure_pattern_fingerprint(
    repo_slug: &str,
    summary: &str,
    affected_files: &[String],
    canonical_markers: &[String],
) -> String {
    let mut parts = VecDeque::<String>::new();
    parts.push_back(repo_slug.to_string());
    parts.push_back(summary.trim().to_string());
    for marker in canonical_markers {
        parts.push_back(marker.trim().to_string());
    }
    for path in affected_files {
        parts.push_back(path.trim().to_string());
    }
    let joined = parts.into_iter().collect::<Vec<_>>().join("|");
    crate::sha256_hex(&[joined.as_str()])
}

pub(crate) async fn find_failure_pattern_duplicates(
    state: &AppState,
    repo_slug: &str,
    project_id: Option<&str>,
    subjects: &[String],
    query: &str,
    fingerprint: Option<&str>,
    limit: usize,
) -> Result<Vec<Value>, StatusCode> {
    let mut hits =
        list_repo_memory_candidates(state, repo_slug, None, limit.saturating_mul(3)).await?;
    if let Some(db) = super::skills_memory::open_global_memory_db().await {
        let mut seen_memory_ids = HashSet::<String>::new();
        for subject in subjects {
            let Ok(results) = db
                .search_global_memory(
                    subject,
                    query,
                    limit.clamp(1, 20) as i64,
                    project_id,
                    None,
                    None,
                )
                .await
            else {
                continue;
            };
            for hit in results {
                if !seen_memory_ids.insert(hit.record.id.clone()) {
                    continue;
                }
                hits.push(json!({
                    "source": "governed_memory",
                    "memory_id": hit.record.id,
                    "score": hit.score,
                    "content": hit.record.content,
                    "metadata": hit.record.metadata,
                    "memory_visibility": hit.record.visibility,
                    "source_type": hit.record.source_type,
                    "run_id": hit.record.run_id,
                    "project_tag": hit.record.project_tag,
                    "subject": subject,
                    "created_at_ms": hit.record.created_at_ms,
                }));
            }
        }
        if let Some(target_fingerprint) =
            fingerprint.map(str::trim).filter(|value| !value.is_empty())
        {
            for subject in subjects {
                let Ok(records) = db
                    .list_global_memory(subject, None, None, None, 200, 0)
                    .await
                else {
                    continue;
                };
                for record in records {
                    if !seen_memory_ids.insert(record.id.clone()) {
                        continue;
                    }
                    if record.project_tag.as_deref() != project_id.or(Some(repo_slug)) {
                        continue;
                    }
                    let Some(metadata) = record.metadata.as_ref() else {
                        continue;
                    };
                    let stored_fingerprint = metadata
                        .get("failure_pattern_fingerprint")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
                    if stored_fingerprint != Some(target_fingerprint) {
                        continue;
                    }
                    hits.push(json!({
                        "source": "governed_memory",
                        "memory_id": record.id,
                        "score": 1.0,
                        "content": record.content,
                        "metadata": record.metadata,
                        "memory_visibility": record.visibility,
                        "source_type": record.source_type,
                        "run_id": record.run_id,
                        "project_tag": record.project_tag,
                        "subject": subject,
                        "created_at_ms": record.created_at_ms,
                    }));
                }
            }
        }
    }
    Ok(derive_failure_pattern_duplicate_matches(
        &hits,
        fingerprint,
        limit,
    ))
}

async fn write_coder_artifact(
    state: &AppState,
    linked_context_run_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    relative_path: &str,
    payload: &Value,
) -> Result<ContextBlackboardArtifact, StatusCode> {
    let path =
        super::context_runs::context_run_dir(state, linked_context_run_id).join(relative_path);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let raw =
        serde_json::to_string_pretty(payload).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(&path, raw)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let artifact = ContextBlackboardArtifact {
        id: artifact_id.to_string(),
        ts_ms: crate::now_ms(),
        path: path.to_string_lossy().to_string(),
        artifact_type: artifact_type.to_string(),
        step_id: None,
        source_event_id: None,
    };
    context_run_engine()
        .commit_blackboard_patch(
            state,
            linked_context_run_id,
            ContextBlackboardPatchOp::AddArtifact,
            serde_json::to_value(&artifact).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        )
        .await?;
    Ok(artifact)
}

async fn write_coder_memory_candidate_artifact(
    state: &AppState,
    record: &CoderRunRecord,
    kind: CoderMemoryCandidateKind,
    summary: Option<String>,
    task_id: Option<String>,
    payload: Value,
) -> Result<(String, ContextBlackboardArtifact), StatusCode> {
    let candidate_id = format!("memcand-{}", Uuid::new_v4().simple());
    let stored_payload = json!({
        "candidate_id": candidate_id,
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "kind": kind,
        "task_id": task_id,
        "summary": summary,
        "payload": payload,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "created_at_ms": crate::now_ms(),
    });
    let artifact = write_coder_artifact(
        state,
        &record.linked_context_run_id,
        &candidate_id,
        "coder_memory_candidate",
        &format!("coder_memory/{candidate_id}.json"),
        &stored_payload,
    )
    .await?;
    publish_coder_artifact_added(state, record, &artifact, Some("artifact_write"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("memory_candidate"));
        extra.insert("candidate_id".to_string(), json!(candidate_id));
        extra.insert("candidate_kind".to_string(), json!(kind));
        extra
    });
    publish_coder_run_event(
        state,
        "coder.memory.candidate_added",
        record,
        Some("artifact_write"),
        {
            let mut extra = coder_artifact_event_fields(&artifact, Some("memory_candidate"));
            extra.insert("candidate_id".to_string(), json!(candidate_id));
            extra.insert("candidate_kind".to_string(), json!(kind));
            extra
        },
    );
    Ok((candidate_id, artifact))
}

fn build_governed_memory_content(candidate_payload: &Value) -> Option<String> {
    let base = candidate_payload
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            candidate_payload
                .get("payload")
                .and_then(|row| row.get("summary"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        });
    let payload = candidate_payload.get("payload");
    let mut segments = Vec::<String>::new();
    if let Some(summary) = base {
        segments.push(summary);
    }
    let push_optional = |segments: &mut Vec<String>, label: &str, value: Option<&Value>| {
        if let Some(text) = value
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            segments.push(format!("{label}: {text}"));
        }
    };
    let push_list = |segments: &mut Vec<String>, label: &str, value: Option<&Value>| {
        let values = value
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row.as_str().map(str::trim))
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !values.is_empty() {
            segments.push(format!("{label}: {}", values.join(", ")));
        }
    };
    let push_object_summaries = |segments: &mut Vec<String>, label: &str, value: Option<&Value>| {
        let values = value
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| {
                        row.get("summary")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(ToString::to_string)
                            .or_else(|| {
                                row.get("kind")
                                    .and_then(Value::as_str)
                                    .map(str::trim)
                                    .filter(|value| !value.is_empty())
                                    .map(ToString::to_string)
                            })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !values.is_empty() {
            segments.push(format!("{label}: {}", values.join(", ")));
        }
    };
    push_optional(
        &mut segments,
        "workflow",
        payload.and_then(|row| row.get("workflow_mode")),
    );
    push_optional(
        &mut segments,
        "result",
        payload.and_then(|row| row.get("result")),
    );
    push_optional(
        &mut segments,
        "verdict",
        payload.and_then(|row| row.get("verdict")),
    );
    push_optional(
        &mut segments,
        "recommendation",
        payload.and_then(|row| row.get("recommendation")),
    );
    push_optional(
        &mut segments,
        "fix_strategy",
        payload.and_then(|row| row.get("fix_strategy")),
    );
    push_optional(
        &mut segments,
        "root_cause",
        payload.and_then(|row| row.get("root_cause")),
    );
    push_optional(
        &mut segments,
        "risk_level",
        payload.and_then(|row| row.get("risk_level")),
    );
    push_list(
        &mut segments,
        "changed_files",
        payload.and_then(|row| row.get("changed_files")),
    );
    push_list(
        &mut segments,
        "blockers",
        payload.and_then(|row| row.get("blockers")),
    );
    push_list(
        &mut segments,
        "requested_changes",
        payload.and_then(|row| row.get("requested_changes")),
    );
    push_list(
        &mut segments,
        "required_checks",
        payload.and_then(|row| row.get("required_checks")),
    );
    push_list(
        &mut segments,
        "required_approvals",
        payload.and_then(|row| row.get("required_approvals")),
    );
    push_list(
        &mut segments,
        "validation_steps",
        payload.and_then(|row| row.get("validation_steps")),
    );
    push_object_summaries(
        &mut segments,
        "validation_results",
        payload.and_then(|row| row.get("validation_results")),
    );
    push_object_summaries(
        &mut segments,
        "regression_signals",
        payload.and_then(|row| row.get("regression_signals")),
    );
    if segments.is_empty() {
        None
    } else {
        Some(segments.join("\n"))
    }
}

fn coder_memory_partition(record: &CoderRunRecord, tier: GovernedMemoryTier) -> MemoryPartition {
    MemoryPartition {
        org_id: record.repo_binding.workspace_id.clone(),
        workspace_id: record.repo_binding.workspace_id.clone(),
        project_id: record.repo_binding.project_id.clone(),
        tier,
    }
}

fn project_coder_phase(run: &ContextRunState) -> &'static str {
    if matches!(
        run.status,
        ContextRunStatus::Queued | ContextRunStatus::Planning
    ) {
        return "bootstrapping";
    }
    if matches!(run.status, ContextRunStatus::AwaitingApproval) {
        return "approval";
    }
    if matches!(run.status, ContextRunStatus::Completed) {
        return "completed";
    }
    if matches!(run.status, ContextRunStatus::Cancelled) {
        return "cancelled";
    }
    if matches!(
        run.status,
        ContextRunStatus::Failed | ContextRunStatus::Blocked
    ) {
        return "failed";
    }
    for task in &run.tasks {
        if matches!(
            task.status,
            ContextBlackboardTaskStatus::Runnable | ContextBlackboardTaskStatus::InProgress
        ) {
            return match task.workflow_node_id.as_deref() {
                Some("ingest_reference") => "bootstrapping",
                Some("retrieve_memory") => "memory_retrieval",
                Some("inspect_repo") => "repo_inspection",
                Some("inspect_pull_request") => "repo_inspection",
                Some("attempt_reproduction") => "reproduction",
                Some("review_pull_request") => "analysis",
                Some("write_triage_artifact") => "artifact_write",
                Some("write_review_artifact") => "artifact_write",
                Some("write_fix_artifact") => "artifact_write",
                Some("write_merge_artifact") => "artifact_write",
                _ => "analysis",
            };
        }
    }
    "analysis"
}

async fn finalize_coder_workflow_run(
    state: &AppState,
    record: &CoderRunRecord,
    workflow_node_ids: &[&str],
    final_status: ContextRunStatus,
    completion_reason: &str,
) -> Result<ContextRunState, StatusCode> {
    let mut run = load_context_run_state(state, &record.linked_context_run_id).await?;
    let now = crate::now_ms();
    let workflow_nodes: HashSet<&str> = workflow_node_ids.iter().copied().collect();
    for task in &mut run.tasks {
        if task
            .workflow_node_id
            .as_deref()
            .is_some_and(|node_id| workflow_nodes.contains(node_id))
        {
            task.status = ContextBlackboardTaskStatus::Done;
            task.lease_owner = None;
            task.lease_token = None;
            task.lease_expires_at_ms = None;
            task.updated_ts = now;
            task.task_rev = task.task_rev.saturating_add(1);
        }
    }
    for workflow_node_id in workflow_node_ids {
        if run
            .tasks
            .iter()
            .any(|task| task.workflow_node_id.as_deref() == Some(*workflow_node_id))
        {
            continue;
        }
        let task_type = match *workflow_node_id {
            "retrieve_memory" => "research",
            "inspect_repo" | "inspect_pull_request" | "inspect_issue_context" => "inspection",
            "attempt_reproduction"
            | "review_pull_request"
            | "prepare_fix"
            | "assess_merge_readiness" => "analysis",
            _ => "implementation",
        };
        run.tasks.push(super::context_types::ContextBlackboardTask {
            id: format!("coder-autocomplete-{}", Uuid::new_v4().simple()),
            task_type: task_type.to_string(),
            payload: json!({
                "task_kind": task_type,
                "title": format!("Complete workflow step: {workflow_node_id}"),
                "source": "coder_summary_finalize",
            }),
            status: ContextBlackboardTaskStatus::Done,
            workflow_id: Some(run.run_type.clone()),
            workflow_node_id: Some((*workflow_node_id).to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            assigned_agent: None,
            priority: 0,
            attempt: 0,
            max_attempts: 1,
            last_error: None,
            next_retry_at_ms: None,
            lease_owner: None,
            lease_token: None,
            lease_expires_at_ms: None,
            task_rev: 1,
            created_ts: now,
            updated_ts: now,
        });
    }
    run.status = final_status;
    run.updated_at_ms = now;
    run.why_next_step = Some(completion_reason.to_string());
    ensure_context_run_dir(state, &record.linked_context_run_id).await?;
    save_context_run_state(state, &run).await?;
    let mut sync_record = record.clone();
    maybe_sync_github_project_status(state, &mut sync_record, &run).await?;
    publish_coder_run_event(
        state,
        "coder.run.phase_changed",
        &sync_record,
        Some(project_coder_phase(&run)),
        {
            let mut extra = serde_json::Map::new();
            extra.insert("status".to_string(), json!(run.status));
            extra.insert("event_type".to_string(), json!("workflow_summary_recorded"));
            extra
        },
    );
    Ok(run)
}

async fn advance_coder_workflow_run(
    state: &AppState,
    record: &CoderRunRecord,
    completed_workflow_node_ids: &[&str],
    runnable_workflow_node_ids: &[&str],
    next_reason: &str,
) -> Result<ContextRunState, StatusCode> {
    let mut run = load_context_run_state(state, &record.linked_context_run_id).await?;
    let now = crate::now_ms();
    let completed_nodes: HashSet<&str> = completed_workflow_node_ids.iter().copied().collect();
    let runnable_nodes: HashSet<&str> = runnable_workflow_node_ids.iter().copied().collect();
    for task in &mut run.tasks {
        if task
            .workflow_node_id
            .as_deref()
            .is_some_and(|node_id| completed_nodes.contains(node_id))
        {
            task.status = ContextBlackboardTaskStatus::Done;
            task.lease_owner = None;
            task.lease_token = None;
            task.lease_expires_at_ms = None;
            task.updated_ts = now;
            task.task_rev = task.task_rev.saturating_add(1);
            continue;
        }
        if task
            .workflow_node_id
            .as_deref()
            .is_some_and(|node_id| runnable_nodes.contains(node_id))
            && matches!(task.status, ContextBlackboardTaskStatus::Pending)
        {
            task.status = ContextBlackboardTaskStatus::Runnable;
            task.updated_ts = now;
            task.task_rev = task.task_rev.saturating_add(1);
        }
    }
    for workflow_node_id in completed_workflow_node_ids {
        if run
            .tasks
            .iter()
            .any(|task| task.workflow_node_id.as_deref() == Some(*workflow_node_id))
        {
            continue;
        }
        let task_type = match *workflow_node_id {
            "retrieve_memory" => "research",
            "inspect_repo" | "inspect_pull_request" | "inspect_issue_context" => "inspection",
            "attempt_reproduction"
            | "review_pull_request"
            | "prepare_fix"
            | "assess_merge_readiness" => "analysis",
            _ => "implementation",
        };
        run.tasks.push(super::context_types::ContextBlackboardTask {
            id: format!("coder-progress-complete-{}", Uuid::new_v4().simple()),
            task_type: task_type.to_string(),
            payload: json!({
                "task_kind": task_type,
                "title": format!("Complete workflow step: {workflow_node_id}"),
                "source": "coder_progress_advance",
            }),
            status: ContextBlackboardTaskStatus::Done,
            workflow_id: Some(run.run_type.clone()),
            workflow_node_id: Some((*workflow_node_id).to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            assigned_agent: None,
            priority: 0,
            attempt: 0,
            max_attempts: 1,
            last_error: None,
            next_retry_at_ms: None,
            lease_owner: None,
            lease_token: None,
            lease_expires_at_ms: None,
            task_rev: 1,
            created_ts: now,
            updated_ts: now,
        });
    }
    for workflow_node_id in runnable_workflow_node_ids {
        if run
            .tasks
            .iter()
            .any(|task| task.workflow_node_id.as_deref() == Some(*workflow_node_id))
        {
            continue;
        }
        let task_type = match *workflow_node_id {
            "retrieve_memory" => "research",
            "inspect_repo" | "inspect_pull_request" | "inspect_issue_context" => "inspection",
            "attempt_reproduction"
            | "review_pull_request"
            | "prepare_fix"
            | "assess_merge_readiness" => "analysis",
            _ => "implementation",
        };
        run.tasks.push(super::context_types::ContextBlackboardTask {
            id: format!("coder-progress-runnable-{}", Uuid::new_v4().simple()),
            task_type: task_type.to_string(),
            payload: json!({
                "task_kind": task_type,
                "title": format!("Continue workflow step: {workflow_node_id}"),
                "source": "coder_progress_advance",
            }),
            status: ContextBlackboardTaskStatus::Runnable,
            workflow_id: Some(run.run_type.clone()),
            workflow_node_id: Some((*workflow_node_id).to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            assigned_agent: None,
            priority: 0,
            attempt: 0,
            max_attempts: 1,
            last_error: None,
            next_retry_at_ms: None,
            lease_owner: None,
            lease_token: None,
            lease_expires_at_ms: None,
            task_rev: 1,
            created_ts: now,
            updated_ts: now,
        });
    }
    run.status = ContextRunStatus::Running;
    run.started_at_ms.get_or_insert(now);
    run.updated_at_ms = now;
    run.why_next_step = Some(next_reason.to_string());
    ensure_context_run_dir(state, &record.linked_context_run_id).await?;
    save_context_run_state(state, &run).await?;
    let mut sync_record = record.clone();
    maybe_sync_github_project_status(state, &mut sync_record, &run).await?;
    publish_coder_run_event(
        state,
        "coder.run.phase_changed",
        &sync_record,
        Some(project_coder_phase(&run)),
        {
            let mut extra = serde_json::Map::new();
            extra.insert("status".to_string(), json!(run.status));
            extra.insert("event_type".to_string(), json!("workflow_progressed"));
            extra
        },
    );
    Ok(run)
}

async fn bootstrap_coder_workflow_run(
    state: &AppState,
    record: &CoderRunRecord,
    completed_workflow_node_ids: &[&str],
    runnable_workflow_node_ids: &[&str],
    next_reason: &str,
) -> Result<ContextRunState, StatusCode> {
    advance_coder_workflow_run(
        state,
        record,
        completed_workflow_node_ids,
        runnable_workflow_node_ids,
        next_reason,
    )
    .await
}

fn default_coder_worker_agent_id(input: Option<&str>) -> String {
    input
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "coder_engine_worker".to_string())
}

fn summarize_workflow_memory_hits(
    record: &CoderRunRecord,
    run: &ContextRunState,
    workflow_node_id: &str,
) -> Vec<String> {
    run.tasks
        .iter()
        .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
        .and_then(|task| task.payload.get("memory_hits"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .take(3)
                .filter_map(|row| {
                    row.get("summary")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string)
                        .or_else(|| {
                            row.get("content")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(|value| value.chars().take(120).collect::<String>())
                        })
                })
                .collect::<Vec<_>>()
        })
        .filter(|rows| !rows.is_empty())
        .unwrap_or_else(|| {
            vec![format!(
                "No reusable workflow memory was available for {}.",
                record.repo_binding.repo_slug
            )]
        })
}

async fn complete_claimed_coder_task(
    state: &AppState,
    run_id: String,
    task: &super::context_types::ContextBlackboardTask,
    agent_id: &str,
) -> Result<(), StatusCode> {
    let lease_token = task
        .lease_token
        .clone()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = context_run_task_transition(
        State(state.clone()),
        Path((run_id, task.id.clone())),
        Json(ContextTaskTransitionInput {
            action: "complete".to_string(),
            command_id: Some(format!(
                "coder:{}:complete:{}",
                task.id,
                Uuid::new_v4().simple()
            )),
            expected_task_rev: Some(task.task_rev),
            lease_token: Some(lease_token),
            agent_id: Some(agent_id.to_string()),
            status: None,
            error: None,
            lease_ms: None,
        }),
    )
    .await?;
    let payload = response.0;
    if payload.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(StatusCode::CONFLICT);
    }
    Ok(())
}

async fn fail_claimed_coder_task(
    state: &AppState,
    run_id: String,
    task: &super::context_types::ContextBlackboardTask,
    agent_id: &str,
    error: &str,
) -> Result<(), StatusCode> {
    let lease_token = task
        .lease_token
        .clone()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = context_run_task_transition(
        State(state.clone()),
        Path((run_id, task.id.clone())),
        Json(ContextTaskTransitionInput {
            action: "fail".to_string(),
            command_id: Some(format!(
                "coder:{}:fail:{}",
                task.id,
                Uuid::new_v4().simple()
            )),
            expected_task_rev: Some(task.task_rev),
            lease_token: Some(lease_token),
            agent_id: Some(agent_id.to_string()),
            status: None,
            error: Some(crate::truncate_text(error, 500)),
            lease_ms: None,
        }),
    )
    .await?;
    let payload = response.0;
    if payload.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(StatusCode::CONFLICT);
    }
    Ok(())
}

async fn dispatch_issue_triage_task(
    state: AppState,
    record: &CoderRunRecord,
    task: &super::context_types::ContextBlackboardTask,
    agent_id: &str,
) -> Result<Value, StatusCode> {
    let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
    let issue_number = record
        .github_ref
        .as_ref()
        .map(|row| row.number)
        .unwrap_or_default();
    match task.workflow_node_id.as_deref() {
        Some("inspect_repo") => {
            let memory_hits_used = summarize_workflow_memory_hits(record, &run, "retrieve_memory");
            let (worker_artifact, worker_payload) =
                match run_issue_triage_worker(&state, record, &run, Some(task.id.as_str())).await {
                    Ok(result) => result,
                    Err(error) => {
                        let detail = format!(
                        "Issue-triage worker session failed during inspect_repo with status {}.",
                        error
                    );
                        let generated_candidate = write_worker_failure_run_outcome_candidate(
                            &state,
                            record,
                            "inspect_repo",
                            "coder_issue_triage_worker_session",
                            "issue_triage_inspection_failed",
                            &detail,
                        )
                        .await?;
                        fail_claimed_coder_task(
                            &state,
                            record.linked_context_run_id.clone(),
                            task,
                            agent_id,
                            &detail,
                        )
                        .await?;
                        let failed = coder_run_transition(
                            &state,
                            record,
                            "run_failed",
                            ContextRunStatus::Failed,
                            Some(detail.clone()),
                        )
                        .await?;
                        return Ok(json!({
                            "ok": false,
                            "error": detail,
                            "code": "CODER_WORKER_SESSION_FAILED",
                            "generated_candidates": generated_candidate
                                .map(|candidate| vec![candidate])
                                .unwrap_or_default(),
                            "run": failed.get("run").cloned().unwrap_or(Value::Null),
                            "coder_run": failed.get("coder_run").cloned().unwrap_or(Value::Null),
                        }));
                    }
                };
            let parsed_triage = parse_issue_triage_from_worker_payload(&worker_payload);
            let response = coder_triage_inspection_report_create(
                State(state),
                Path(record.coder_run_id.clone()),
                Json(CoderTriageInspectionReportCreateInput {
                    summary: parsed_triage
                        .get("summary")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some(format!(
                            "Engine worker inspected likely repo areas for {} issue #{}.",
                            record.repo_binding.repo_slug, issue_number
                        ))),
                    likely_areas: parsed_triage
                        .get("likely_areas")
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .filter(|rows| !rows.is_empty())
                        .unwrap_or_else(|| {
                            vec![
                                "repo workspace context".to_string(),
                                "prior triage memory".to_string(),
                            ]
                        }),
                    affected_files: parsed_triage
                        .get("affected_files")
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                    memory_hits_used,
                    notes: Some(format!(
                        "Auto-generated by coder engine worker dispatch. Worker run: {}. Worker artifact: {}.",
                        preferred_session_run_reference(&worker_payload)
                            .as_str()
                            .unwrap_or("unknown"),
                        worker_artifact.path
                    )),
                }),
            )
            .await?;
            Ok(attach_worker_dispatch_reference(
                response.0,
                Some(&worker_payload),
            ))
        }
        Some("attempt_reproduction") => {
            let memory_hits_used = summarize_workflow_memory_hits(record, &run, "retrieve_memory");
            let worker_payload = load_latest_coder_artifact_payload(
                &state,
                record,
                "coder_issue_triage_worker_session",
            )
            .await;
            let parsed_triage = worker_payload
                .as_ref()
                .map(parse_issue_triage_from_worker_payload);
            let response = coder_triage_reproduction_report_create(
                State(state),
                Path(record.coder_run_id.clone()),
                Json(CoderTriageReproductionReportCreateInput {
                    summary: parsed_triage
                        .as_ref()
                        .and_then(|payload| payload.get("summary"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| {
                            Some(format!(
                            "Engine worker attempted constrained reproduction for {} issue #{}.",
                            record.repo_binding.repo_slug, issue_number
                        ))
                        }),
                    outcome: parsed_triage
                        .as_ref()
                        .and_then(|payload| payload.get("reproduction_outcome"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some("needs_follow_up".to_string())),
                    steps: parsed_triage
                        .as_ref()
                        .and_then(|payload| payload.get("reproduction_steps"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .filter(|rows| !rows.is_empty())
                        .unwrap_or_else(|| {
                            vec![
                                "Review current issue context".to_string(),
                                "Use prior memory hits to constrain reproduction".to_string(),
                            ]
                        }),
                    observed_logs: parsed_triage
                        .as_ref()
                        .and_then(|payload| payload.get("observed_logs"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                    affected_files: parsed_triage
                        .as_ref()
                        .and_then(|payload| payload.get("affected_files"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                    memory_hits_used,
                    notes: Some(format!(
                        "Auto-generated by coder engine worker dispatch. Triage worker run: {}",
                        worker_payload
                            .as_ref()
                            .map(preferred_session_run_reference)
                            .as_ref()
                            .and_then(Value::as_str)
                            .unwrap_or("unavailable")
                    )),
                }),
            )
            .await?;
            Ok(attach_worker_dispatch_reference(
                response.0,
                worker_payload.as_ref(),
            ))
        }
        Some("write_triage_artifact") => {
            let memory_hits_used = summarize_workflow_memory_hits(record, &run, "retrieve_memory");
            let duplicate_candidates =
                summarize_workflow_duplicate_candidates(record, &run, "retrieve_memory");
            let prior_runs_considered =
                summarize_workflow_prior_runs_considered(record, &run, "retrieve_memory");
            let worker_payload = load_latest_coder_artifact_payload(
                &state,
                record,
                "coder_issue_triage_worker_session",
            )
            .await;
            let parsed_triage = worker_payload
                .as_ref()
                .map(parse_issue_triage_from_worker_payload);
            let response = coder_triage_summary_create(
                State(state),
                Path(record.coder_run_id.clone()),
                Json(CoderTriageSummaryCreateInput {
                    summary: parsed_triage
                        .as_ref()
                        .and_then(|payload| payload.get("summary"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some(format!(
                            "Engine worker completed initial triage for {} issue #{}.",
                            record.repo_binding.repo_slug, issue_number
                        ))),
                    confidence: parsed_triage
                        .as_ref()
                        .and_then(|payload| payload.get("confidence"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some("medium".to_string())),
                    affected_files: parsed_triage
                        .as_ref()
                        .and_then(|payload| payload.get("affected_files"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                    duplicate_candidates,
                    prior_runs_considered,
                    memory_hits_used,
                    reproduction: Some(json!({
                        "outcome": parsed_triage
                            .as_ref()
                            .and_then(|payload| payload.get("reproduction_outcome"))
                            .cloned()
                            .unwrap_or_else(|| json!("needs_follow_up")),
                        "steps": parsed_triage
                            .as_ref()
                            .and_then(|payload| payload.get("reproduction_steps"))
                            .cloned()
                            .unwrap_or_else(|| json!([])),
                        "observed_logs": parsed_triage
                            .as_ref()
                            .and_then(|payload| payload.get("observed_logs"))
                            .cloned()
                            .unwrap_or_else(|| json!([])),
                        "source": "coder_engine_worker"
                    })),
                    notes: Some(format!(
                        "Auto-generated by coder engine worker dispatch. Triage worker artifact available: {}",
                        worker_payload.is_some()
                    )),
                }),
            )
            .await?;
            Ok(attach_worker_dispatch_reference(
                response.0,
                worker_payload.as_ref(),
            ))
        }
        Some("ingest_reference") | Some("retrieve_memory") => {
            complete_claimed_coder_task(
                &state,
                record.linked_context_run_id.clone(),
                task,
                agent_id,
            )
            .await?;
            let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
            Ok(json!({
                "ok": true,
                "task": task,
                "run": run,
                "coder_run": coder_run_payload(record, &run),
                "dispatched": false,
                "reason": "bootstrap task completed through generic task transition"
            }))
        }
        _ => Err(StatusCode::CONFLICT),
    }
}
