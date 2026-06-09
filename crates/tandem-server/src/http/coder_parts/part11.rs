fn coder_memory_authority_job_context(
    tenant_context: &tandem_types::TenantContext,
    capability: &tandem_memory::MemoryCapabilityToken,
    record: &CoderRunRecord,
    candidate_id: &str,
    partition: &tandem_memory::MemoryPartition,
    artifact_refs: &[String],
    operation: tandem_memory::MemoryAuthorityOperation,
    source_memory_ids: Vec<String>,
    approval_id: Option<&String>,
) -> tandem_memory::MemoryAuthorityJobContext {
    tandem_memory::MemoryAuthorityJobContext {
        org_id: tenant_context.org_id.clone(),
        workspace_id: tenant_context.workspace_id.clone(),
        deployment_id: tenant_context.deployment_id.clone(),
        project_id: partition.project_id.clone(),
        actor_id: capability.subject.clone(),
        run_id: record.linked_context_run_id.clone(),
        node_id: record
            .worker_run_id
            .clone()
            .or_else(|| record.worker_session_id.clone()),
        task_id: Some(candidate_id.to_string()),
        purpose: "promote approved coder memory candidate".to_string(),
        source_binding_id: Some(format!("repo:{}", record.repo_binding.repo_slug)),
        data_class: Some(tandem_types::DataClass::SourceCode),
        classification: tandem_memory::MemoryClassification::Internal,
        operation,
        source_memory_ids,
        artifact_refs: artifact_refs.to_vec(),
        policy_decision_id: approval_id.cloned(),
        grant_decision_id: approval_id.cloned(),
    }
}
