struct CoderMemoryAuthorityJobContextBase<'a> {
    tenant_context: &'a tandem_types::TenantContext,
    capability: &'a tandem_memory::MemoryCapabilityToken,
    record: &'a CoderRunRecord,
    candidate_id: &'a str,
    partition: &'a tandem_memory::MemoryPartition,
    artifact_refs: &'a [String],
    approval_id: Option<&'a str>,
}

fn coder_memory_authority_job_context(
    base: &CoderMemoryAuthorityJobContextBase<'_>,
    operation: tandem_memory::MemoryAuthorityOperation,
    source_memory_ids: Vec<String>,
) -> tandem_memory::MemoryAuthorityJobContext {
    tandem_memory::MemoryAuthorityJobContext {
        org_id: base.partition.org_id.clone(),
        workspace_id: base.partition.workspace_id.clone(),
        deployment_id: base.tenant_context.deployment_id.clone(),
        project_id: base.partition.project_id.clone(),
        actor_id: base.capability.subject.clone(),
        run_id: base.record.linked_context_run_id.clone(),
        node_id: base
            .record
            .worker_run_id
            .clone()
            .or_else(|| base.record.worker_session_id.clone()),
        task_id: Some(base.candidate_id.to_string()),
        purpose: "promote approved coder memory candidate".to_string(),
        source_binding_id: Some(format!("repo:{}", base.record.repo_binding.repo_slug)),
        data_class: Some(tandem_types::DataClass::SourceCode),
        classification: tandem_memory::MemoryClassification::Internal,
        operation,
        source_memory_ids,
        artifact_refs: base.artifact_refs.to_vec(),
        policy_decision_id: base.approval_id.map(ToString::to_string),
        grant_decision_id: base.approval_id.map(ToString::to_string),
    }
}
