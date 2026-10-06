// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

type WorkflowLearningCandidateMap = std::collections::HashMap<String, WorkflowLearningCandidate>;

struct PreparedWorkflowLearningFile(std::path::PathBuf);

impl Drop for PreparedWorkflowLearningFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn merge_workflow_learning_candidate(
    rows: &mut WorkflowLearningCandidateMap,
    mut candidate: WorkflowLearningCandidate,
) -> WorkflowLearningCandidate {
    let now = now_ms();
    if candidate.candidate_id.trim().is_empty() {
        candidate.candidate_id = format!("wflearn-{}", uuid::Uuid::new_v4());
    }
    if candidate.created_at_ms == 0 {
        candidate.created_at_ms = now;
    }
    candidate.updated_at_ms = now;
    if let Some(existing) = rows.values_mut().find(|row| {
        row.workflow_id == candidate.workflow_id
            && row.kind == candidate.kind
            && row.fingerprint == candidate.fingerprint
            && row.source_binding == candidate.source_binding
            && matches!((crate::memory::derived_lineage::candidate_lineage(row),
                crate::memory::derived_lineage::candidate_lineage(&candidate)),
                (Ok(left), Ok(right)) if left == right)
    }) {
        existing.summary = candidate.summary.clone();
        existing.confidence = existing.confidence.max(candidate.confidence);
        existing.updated_at_ms = now;
        if existing.node_id.is_none() { existing.node_id = candidate.node_id.clone(); }
        if existing.node_kind.is_none() { existing.node_kind = candidate.node_kind.clone(); }
        if existing.validator_family.is_none() { existing.validator_family = candidate.validator_family.clone(); }
        if existing.proposed_memory_payload.is_none() { existing.proposed_memory_payload = candidate.proposed_memory_payload.clone(); }
        if existing.proposed_revision_prompt.is_none() { existing.proposed_revision_prompt = candidate.proposed_revision_prompt.clone(); }
        if existing.source_memory_id.is_none() { existing.source_memory_id = candidate.source_memory_id.clone(); }
        if existing.promoted_memory_id.is_none() { existing.promoted_memory_id = candidate.promoted_memory_id.clone(); }
        if existing.baseline_before.is_none() { existing.baseline_before = candidate.baseline_before.clone(); }
        if candidate.latest_observed_metrics.is_some() { existing.latest_observed_metrics = candidate.latest_observed_metrics.clone(); }
        if candidate.last_revision_session_id.is_some() { existing.last_revision_session_id = candidate.last_revision_session_id.clone(); }
        existing.needs_plan_bundle |= candidate.needs_plan_bundle;
        for artifact_ref in candidate.artifact_refs {
            if !existing.artifact_refs.contains(&artifact_ref) { existing.artifact_refs.push(artifact_ref); }
        }
        for run_id in candidate.run_ids {
            if !existing.run_ids.contains(&run_id) { existing.run_ids.push(run_id); }
        }
        for evidence_ref in candidate.evidence_refs {
            if !existing.evidence_refs.contains(&evidence_ref) { existing.evidence_refs.push(evidence_ref); }
        }
        existing.clone()
    } else {
        rows.insert(candidate.candidate_id.clone(), candidate.clone());
        candidate
    }
}

impl AppState {
    /// Native source reads precede this call. Candidate commits acquire the
    /// candidate writer before publication authority; no publication-first
    /// caller may enter this method. The owned task survives caller cancellation.
    pub(crate) async fn upsert_workflow_learning_candidate_with_current_policy(
        &self,
        candidate: WorkflowLearningCandidate,
        verified: Option<tandem_types::VerifiedTenantContext>,
    ) -> anyhow::Result<WorkflowLearningCandidate> {
        let state = self.clone();
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;

            let mut rows = state.workflow_learning_candidates.write().await;
            let _publication = state.enterprise.hosted_policy.lock_publication_owned().await;
            state.enterprise.hosted_policy.authorize(verified.as_ref())
                .map_err(anyhow::Error::msg)?;
            let mut next = rows.clone();
            let stored = merge_workflow_learning_candidate(&mut next, candidate);
            let payload = serde_json::to_vec_pretty(&next)?;
            if let Some(parent) = state.workflow_learning_candidates_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            let temporary_path = state.workflow_learning_candidates_path
                .with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&temporary_path).await?;
            let prepared = PreparedWorkflowLearningFile(temporary_path);
            file.write_all(&payload).await?;
            file.sync_all().await?;
            drop(file);
            // Preparation can await filesystem work. Check the ORIGINAL
            // assertion again at actual file publication, before changing cache.
            state.enterprise.hosted_policy.authorize(verified.as_ref())
                .map_err(anyhow::Error::msg)?;
            std::fs::rename(&prepared.0, &state.workflow_learning_candidates_path)?;
            *rows = next;
            Ok(stored)
        }).await?
    }
}
