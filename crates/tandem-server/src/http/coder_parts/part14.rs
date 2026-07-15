// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

async fn write_pr_review_evidence_artifact(
    state: &AppState,
    record: &CoderRunRecord,
    verdict: Option<&str>,
    summary: Option<&str>,
    risk_level: Option<&str>,
    changed_files: &[String],
    blockers: &[String],
    requested_changes: &[String],
    regression_signals: &[Value],
    memory_hits_used: &[String],
    notes: Option<&str>,
    summary_artifact_path: Option<&str>,
    phase: Option<&str>,
) -> Result<Option<ContextBlackboardArtifact>, StatusCode> {
    if changed_files.is_empty()
        && blockers.is_empty()
        && requested_changes.is_empty()
        && regression_signals.is_empty()
        && summary.map(str::trim).unwrap_or("").is_empty()
        && notes.map(str::trim).unwrap_or("").is_empty()
    {
        return Ok(None);
    }
    let evidence_id = format!("pr-review-evidence-{}", Uuid::new_v4().simple());
    let evidence_payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "verdict": verdict,
        "summary": summary,
        "risk_level": risk_level,
        "changed_files": changed_files,
        "blockers": blockers,
        "requested_changes": requested_changes,
        "regression_signals": regression_signals,
        "memory_hits_used": memory_hits_used,
        "notes": notes,
        "summary_artifact_path": summary_artifact_path,
        "created_at_ms": crate::now_ms(),
    });
    let evidence_artifact = write_coder_artifact(
        state,
        &record.linked_context_run_id,
        &evidence_id,
        "coder_review_evidence",
        "artifacts/pr_review.evidence.json",
        &evidence_payload,
    )
    .await?;
    publish_coder_artifact_added(state, record, &evidence_artifact, phase, {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("review_evidence"));
        if let Some(verdict) = verdict {
            extra.insert("verdict".to_string(), json!(verdict));
        }
        if let Some(risk_level) = risk_level {
            extra.insert("risk_level".to_string(), json!(risk_level));
        }
        extra
    });
    Ok(Some(evidence_artifact))
}

pub(super) async fn coder_pr_review_evidence_create(
    State(state): State<AppState>,
    axum::extract::Extension(tenant_context): axum::extract::Extension<tandem_types::TenantContext>,
    Path(id): Path<String>,
    Json(input): Json<CoderPrReviewEvidenceCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let (record, _run) =
        load_coder_run_with_context_for_tenant(&state, &id, &tenant_context).await?;
    let mut record = record;
    if !matches!(record.workflow_mode, CoderWorkflowMode::PrReview) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let artifact = write_pr_review_evidence_artifact(
        &state,
        &record,
        input.verdict.as_deref(),
        input.summary.as_deref(),
        input.risk_level.as_deref(),
        &input.changed_files,
        &input.blockers,
        &input.requested_changes,
        &input.regression_signals,
        &input.memory_hits_used,
        input.notes.as_deref(),
        None,
        Some("analysis"),
    )
    .await?;
    let Some(artifact) = artifact else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let final_run = advance_coder_workflow_run(
        &state,
        &record,
        &[
            "inspect_pull_request",
            "retrieve_memory",
            "review_pull_request",
        ],
        &["write_review_artifact"],
        "Write the PR review summary and verdict.",
    )
    .await?;
    record.updated_at_ms = final_run.updated_at_ms;
    save_coder_run_record(&state, &record).await?;
    let worker_payload =
        load_latest_coder_artifact_payload(&state, &record, "coder_pr_review_worker_session").await;
    Ok(Json(attach_worker_reference_fields(
        json!({
            "ok": true,
            "artifact": artifact,
            "coder_run": coder_run_payload(&record, &final_run),
            "run": final_run,
        }),
        worker_payload.as_ref(),
        None,
    )))
}
