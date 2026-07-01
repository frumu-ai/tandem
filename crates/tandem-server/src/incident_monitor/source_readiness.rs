use std::collections::BTreeMap;

use crate::{
    sha256_hex, IncidentMonitorConfig, IncidentMonitorLogSource,
    IncidentMonitorLogSourceRuntimeStatus, IncidentMonitorLogWatcherStatus,
    IncidentMonitorMonitoredProject, IncidentMonitorSourceBinding, IncidentMonitorSourceReadiness,
    IncidentMonitorSourceReadinessConfig, IncidentMonitorSourceReadinessFinding,
};

pub fn evaluate_source_readiness(
    config: &IncidentMonitorConfig,
    log_watcher: &IncidentMonitorLogWatcherStatus,
    now_ms: u64,
) -> Vec<IncidentMonitorSourceReadiness> {
    let runtime_by_source = log_watcher
        .sources
        .iter()
        .map(|row| ((row.project_id.clone(), row.source_id.clone()), row))
        .collect::<BTreeMap<_, _>>();
    let mut readiness = Vec::new();

    for project in &config.monitored_projects {
        readiness.push(evaluate_source_binding(
            project,
            None,
            &project.source_binding(None),
            None,
            now_ms,
        ));

        for source in &project.log_sources {
            let runtime =
                runtime_by_source.get(&(project.project_id.clone(), source.source_id.clone()));
            readiness.push(evaluate_source_binding(
                project,
                Some(source),
                &project.source_binding(Some(source)),
                runtime.copied(),
                now_ms,
            ));
        }
    }

    readiness
}

fn evaluate_source_binding(
    project: &IncidentMonitorMonitoredProject,
    source: Option<&IncidentMonitorLogSource>,
    binding: &IncidentMonitorSourceBinding,
    runtime: Option<&IncidentMonitorLogSourceRuntimeStatus>,
    now_ms: u64,
) -> IncidentMonitorSourceReadiness {
    let source_id = source.map(|row| row.source_id.clone());
    let source_ref = source_ref(&project.project_id, source_id.as_deref());
    let enabled = project.enabled
        && !project.paused
        && source.map(|row| row.enabled && !row.paused).unwrap_or(true);
    let mut findings = Vec::new();
    let mut missing = Vec::new();

    if !enabled {
        return IncidentMonitorSourceReadiness {
            project_id: project.project_id.clone(),
            source_id,
            source_kind: binding.source_kind.clone(),
            enabled,
            ready: false,
            warnings: vec!["Source is disabled or paused".to_string()],
            detail: Some(
                "Source readiness was not evaluated because the source is disabled or paused"
                    .to_string(),
            ),
            ..IncidentMonitorSourceReadiness::default()
        };
    }

    let governance_ready = evaluate_governance_fields(
        binding,
        &source_ref,
        source_id.as_deref(),
        &mut missing,
        &mut findings,
    );
    let lineage_ready = evaluate_lineage_fields(
        &binding.data_readiness,
        &source_ref,
        source_id.as_deref(),
        &mut missing,
        &mut findings,
    );
    let (freshness_ready, last_observed_at_ms, stale_after_ms) = evaluate_freshness_fields(
        &binding.data_readiness,
        runtime,
        &source_ref,
        source_id.as_deref(),
        now_ms,
        &mut missing,
        &mut findings,
    );
    let schema_ready = evaluate_schema_fields(
        binding,
        &source_ref,
        source_id.as_deref(),
        &mut missing,
        &mut findings,
    );
    let protection_ready = evaluate_protection_fields(
        binding,
        &source_ref,
        source_id.as_deref(),
        &mut missing,
        &mut findings,
    );
    evaluate_quality_fields(
        &binding.data_readiness,
        &source_ref,
        source_id.as_deref(),
        &mut missing,
        &mut findings,
    );

    if let Some(runtime) = runtime {
        if runtime.consecutive_errors > 0 || !runtime.healthy {
            push_finding(
                &mut findings,
                &source_ref,
                "source_runtime_unhealthy",
                "source_freshness",
                "medium",
                format!("Source `{source_ref}` has watcher health warnings"),
                "The source watcher has recent errors or is not reporting healthy status.".to_string(),
                vec![evidence_path(source_id.as_deref(), "runtime.health")],
                "Review the log watcher state and repair source access before relying on freshness evidence.",
            );
        }
    }

    let ready = findings.is_empty()
        && governance_ready
        && lineage_ready
        && freshness_ready
        && schema_ready
        && protection_ready;
    let warnings = findings
        .iter()
        .map(|finding| format!("{}: {}", finding.severity, finding.title))
        .collect::<Vec<_>>();
    let detail = if ready {
        Some("Source readiness passed".to_string())
    } else {
        Some(format!(
            "Source readiness has {} finding(s)",
            findings.len()
        ))
    };

    IncidentMonitorSourceReadiness {
        project_id: project.project_id.clone(),
        source_id,
        source_kind: binding.source_kind.clone(),
        enabled,
        ready,
        governance_ready,
        lineage_ready,
        freshness_ready,
        schema_ready,
        protection_ready,
        missing,
        warnings,
        findings,
        last_observed_at_ms,
        stale_after_ms,
        detail,
    }
}

fn evaluate_governance_fields(
    binding: &IncidentMonitorSourceBinding,
    source_ref: &str,
    source_id: Option<&str>,
    missing: &mut Vec<String>,
    findings: &mut Vec<IncidentMonitorSourceReadinessFinding>,
) -> bool {
    let mut ready = true;
    let readiness = &binding.data_readiness;
    for (field, label, severity) in [
        ("source_owner", "source owner", "medium"),
        ("system_of_record", "system of record", "medium"),
        ("data_classification", "data classification", "high"),
        ("allowed_use", "allowed use", "high"),
    ] {
        let present = match field {
            "source_owner" => has_text(readiness.source_owner.as_deref()),
            "system_of_record" => has_text(readiness.system_of_record.as_deref()),
            "data_classification" => has_text(readiness.data_classification.as_deref()),
            "allowed_use" => has_text(readiness.allowed_use.as_deref()),
            _ => true,
        };
        if !present {
            ready = false;
            missing.push(field.to_string());
            push_finding(
                findings,
                source_ref,
                "source_governance_metadata_missing",
                "source_governance",
                severity,
                format!("Source `{source_ref}` is missing {label} metadata"),
                format!("The monitored source does not declare its {label}."),
                vec![evidence_path(source_id, &format!("data_readiness.{field}"))],
                "Add the missing data readiness metadata on the monitored project or log source.",
            );
        }
    }

    if !has_text(binding.tenant_id.as_deref()) || !has_text(binding.workspace_id.as_deref()) {
        ready = false;
        if !has_text(binding.tenant_id.as_deref()) {
            missing.push("tenant_id".to_string());
        }
        if !has_text(binding.workspace_id.as_deref()) {
            missing.push("workspace_id".to_string());
        }
        push_finding(
            findings,
            source_ref,
            "source_boundary_context_missing",
            "source_boundary",
            "high",
            format!("Source `{source_ref}` is missing tenant or workspace context"),
            "Production source readiness requires explicit tenant and workspace boundaries before routing can be trusted.".to_string(),
            vec![
                evidence_path(source_id, "tenant_id"),
                evidence_path(source_id, "workspace_id"),
            ],
            "Set tenant_id and workspace_id on the project or log source binding.",
        );
    }

    if !has_text(readiness.legal_basis.as_deref())
        && !has_text(readiness.authorization_marker.as_deref())
    {
        ready = false;
        missing.push("legal_basis_or_authorization_marker".to_string());
        push_finding(
            findings,
            source_ref,
            "source_authorization_missing",
            "source_governance",
            "high",
            format!("Source `{source_ref}` is missing authorization evidence"),
            "The monitored source does not declare a legal basis or deployer-provided authorization marker.".to_string(),
            vec![
                evidence_path(source_id, "data_readiness.legal_basis"),
                evidence_path(source_id, "data_readiness.authorization_marker"),
            ],
            "Record the legal basis or an authorization marker without embedding raw credentials.",
        );
    }

    ready
}

fn evaluate_lineage_fields(
    readiness: &IncidentMonitorSourceReadinessConfig,
    source_ref: &str,
    source_id: Option<&str>,
    missing: &mut Vec<String>,
    findings: &mut Vec<IncidentMonitorSourceReadinessFinding>,
) -> bool {
    if has_text(readiness.source_of_truth.as_deref()) || has_text(readiness.lineage_ref.as_deref())
    {
        return true;
    }
    missing.push("source_of_truth_or_lineage_ref".to_string());
    push_finding(
        findings,
        source_ref,
        "source_lineage_missing",
        "source_lineage",
        "high",
        format!("Source `{source_ref}` is missing lineage metadata"),
        "The monitored source does not identify a source of truth or lineage reference for audit review.".to_string(),
        vec![
            evidence_path(source_id, "data_readiness.source_of_truth"),
            evidence_path(source_id, "data_readiness.lineage_ref"),
        ],
        "Add source_of_truth or lineage_ref metadata that points auditors to the system of record.",
    );
    false
}

fn evaluate_freshness_fields(
    readiness: &IncidentMonitorSourceReadinessConfig,
    runtime: Option<&IncidentMonitorLogSourceRuntimeStatus>,
    source_ref: &str,
    source_id: Option<&str>,
    now_ms: u64,
    missing: &mut Vec<String>,
    findings: &mut Vec<IncidentMonitorSourceReadinessFinding>,
) -> (bool, Option<u64>, Option<u64>) {
    let Some(freshness_sla_ms) = readiness.freshness_sla_ms.filter(|value| *value > 0) else {
        missing.push("freshness_sla_ms".to_string());
        push_finding(
            findings,
            source_ref,
            "source_freshness_sla_missing",
            "source_freshness",
            "medium",
            format!("Source `{source_ref}` is missing a freshness SLA"),
            "The monitored source does not declare how stale its data may be before production routing should warn.".to_string(),
            vec![evidence_path(source_id, "data_readiness.freshness_sla_ms")],
            "Set freshness_sla_ms on the source readiness metadata.",
        );
        return (false, readiness.last_observed_at_ms, None);
    };

    let runtime_last_seen = runtime.and_then(runtime_last_observed_at_ms);
    let last_observed_at_ms = match (readiness.last_observed_at_ms, runtime_last_seen) {
        (Some(configured), Some(runtime)) => Some(configured.max(runtime)),
        (Some(configured), None) => Some(configured),
        (None, Some(runtime)) => Some(runtime),
        (None, None) => None,
    };
    let Some(last_observed_at_ms) = last_observed_at_ms else {
        missing.push("last_observed_at_ms".to_string());
        push_finding(
            findings,
            source_ref,
            "source_freshness_observation_missing",
            "source_freshness",
            "medium",
            format!("Source `{source_ref}` has no freshness observation"),
            "The monitored source has a freshness SLA but no last observed timestamp from config or the log watcher.".to_string(),
            vec![
                evidence_path(source_id, "data_readiness.last_observed_at_ms"),
                evidence_path(source_id, "runtime.last_observed_at_ms"),
            ],
            "Record a deployer-provided observation timestamp or let the log watcher poll the source.",
        );
        return (false, None, None);
    };

    let stale_after_ms = last_observed_at_ms.saturating_add(freshness_sla_ms);
    if now_ms > stale_after_ms {
        push_finding(
            findings,
            source_ref,
            "source_stale",
            "source_freshness",
            "high",
            format!("Source `{source_ref}` is stale"),
            "The latest source observation is older than its configured freshness SLA.".to_string(),
            vec![
                evidence_path(source_id, "data_readiness.freshness_sla_ms"),
                evidence_path(source_id, "data_readiness.last_observed_at_ms"),
            ],
            "Refresh or quarantine the source before using it for production routing.",
        );
        return (false, Some(last_observed_at_ms), Some(stale_after_ms));
    }

    (true, Some(last_observed_at_ms), Some(stale_after_ms))
}

fn evaluate_schema_fields(
    binding: &IncidentMonitorSourceBinding,
    source_ref: &str,
    source_id: Option<&str>,
    missing: &mut Vec<String>,
    findings: &mut Vec<IncidentMonitorSourceReadinessFinding>,
) -> bool {
    let mut ready = true;
    let readiness = &binding.data_readiness;
    if !has_text(binding.event_schema_version.as_deref()) {
        ready = false;
        missing.push("event_schema_version".to_string());
        push_finding(
            findings,
            source_ref,
            "source_schema_version_missing",
            "source_schema",
            "high",
            format!("Source `{source_ref}` is missing an event schema version"),
            "The source cannot be compared against route or parser expectations without an event schema version.".to_string(),
            vec![evidence_path(source_id, "event_schema_version")],
            "Set event_schema_version on the project or log source binding.",
        );
    }

    if let (Some(expected), Some(actual)) = (
        readiness
            .expected_schema_version
            .as_deref()
            .filter(|value| has_text(Some(value))),
        binding
            .event_schema_version
            .as_deref()
            .filter(|value| has_text(Some(value))),
    ) {
        if !expected.trim().eq_ignore_ascii_case(actual.trim()) {
            ready = false;
            push_finding(
                findings,
                source_ref,
                "source_schema_version_mismatch",
                "source_schema",
                "high",
                format!("Source `{source_ref}` schema version does not match readiness metadata"),
                "The configured event schema version differs from the expected schema version recorded in data readiness metadata.".to_string(),
                vec![
                    evidence_path(source_id, "event_schema_version"),
                    evidence_path(source_id, "data_readiness.expected_schema_version"),
                ],
                "Update the source schema version or complete schema migration review before production routing.",
            );
        }
    }

    match schema_drift_status(readiness.schema_drift_status.as_deref()) {
        SchemaDriftStatus::Ready => {}
        SchemaDriftStatus::Missing => {
            ready = false;
            missing.push("schema_drift_status".to_string());
            push_finding(
                findings,
                source_ref,
                "source_schema_drift_status_missing",
                "source_schema",
                "medium",
                format!("Source `{source_ref}` is missing schema drift status"),
                "The monitored source does not declare whether schema drift has been reviewed.".to_string(),
                vec![evidence_path(source_id, "data_readiness.schema_drift_status")],
                "Set schema_drift_status to none, stable, accepted, or drift_detected after review.",
            );
        }
        SchemaDriftStatus::Unknown => {
            ready = false;
            push_finding(
                findings,
                source_ref,
                "source_schema_drift_unknown",
                "source_schema",
                "medium",
                format!("Source `{source_ref}` schema drift status is unknown"),
                "The monitored source schema drift status is present but does not indicate a reviewed stable state.".to_string(),
                vec![evidence_path(source_id, "data_readiness.schema_drift_status")],
                "Review source schema drift and set an explicit stable or accepted status.",
            );
        }
        SchemaDriftStatus::Drifted => {
            ready = false;
            push_finding(
                findings,
                source_ref,
                "source_schema_drift_detected",
                "source_schema",
                "high",
                format!("Source `{source_ref}` has schema drift"),
                "The monitored source declares schema drift that has not been accepted for production routing.".to_string(),
                vec![evidence_path(source_id, "data_readiness.schema_drift_status")],
                "Quarantine the source or complete schema migration review before production routing.",
            );
        }
    }

    ready
}

fn evaluate_protection_fields(
    binding: &IncidentMonitorSourceBinding,
    source_ref: &str,
    source_id: Option<&str>,
    missing: &mut Vec<String>,
    findings: &mut Vec<IncidentMonitorSourceReadinessFinding>,
) -> bool {
    let mut ready = true;
    if !has_text(binding.redaction_profile.as_deref()) {
        ready = false;
        missing.push("redaction_profile".to_string());
        push_finding(
            findings,
            source_ref,
            "source_redaction_profile_missing",
            "source_protection",
            "high",
            format!("Source `{source_ref}` is missing redaction profile coverage"),
            "Production source readiness requires a redaction profile before source evidence is summarized or routed.".to_string(),
            vec![evidence_path(source_id, "redaction_profile")],
            "Attach a redaction_profile to the source binding.",
        );
    }
    if !has_text(binding.retention_profile.as_deref()) {
        ready = false;
        missing.push("retention_profile".to_string());
        push_finding(
            findings,
            source_ref,
            "source_retention_profile_missing",
            "source_protection",
            "medium",
            format!("Source `{source_ref}` is missing retention profile coverage"),
            "Production source readiness requires an explicit retention profile for evidence and reports.".to_string(),
            vec![evidence_path(source_id, "retention_profile")],
            "Attach a retention_profile to the source binding.",
        );
    }
    ready
}

fn evaluate_quality_fields(
    readiness: &IncidentMonitorSourceReadinessConfig,
    source_ref: &str,
    source_id: Option<&str>,
    missing: &mut Vec<String>,
    findings: &mut Vec<IncidentMonitorSourceReadinessFinding>,
) {
    if has_text(readiness.quality_notes.as_deref()) {
        return;
    }
    missing.push("quality_notes".to_string());
    push_finding(
        findings,
        source_ref,
        "source_quality_notes_missing",
        "source_quality",
        "low",
        format!("Source `{source_ref}` is missing quality notes"),
        "The monitored source does not summarize completeness, known gaps, or quality limitations for reviewers.".to_string(),
        vec![evidence_path(source_id, "data_readiness.quality_notes")],
        "Add quality_notes that summarize known completeness and quality caveats.",
    );
}

fn runtime_last_observed_at_ms(runtime: &IncidentMonitorLogSourceRuntimeStatus) -> Option<u64> {
    [
        runtime.last_submitted_at_ms,
        runtime.last_candidate_at_ms,
        runtime.last_poll_at_ms,
    ]
    .into_iter()
    .flatten()
    .max()
}

fn has_text(value: Option<&str>) -> bool {
    value
        .map(str::trim)
        .is_some_and(|value| !value.is_empty() && value != "local" && value != "null")
}

enum SchemaDriftStatus {
    Ready,
    Missing,
    Unknown,
    Drifted,
}

fn schema_drift_status(value: Option<&str>) -> SchemaDriftStatus {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return SchemaDriftStatus::Missing;
    };
    match value.to_ascii_lowercase().as_str() {
        "none" | "stable" | "accepted" | "up_to_date" | "no_drift" => SchemaDriftStatus::Ready,
        "drifted" | "drift_detected" | "detected" | "outdated" => SchemaDriftStatus::Drifted,
        _ => SchemaDriftStatus::Unknown,
    }
}

fn push_finding(
    findings: &mut Vec<IncidentMonitorSourceReadinessFinding>,
    source_ref: &str,
    rule_id: &str,
    category: &str,
    severity: &str,
    title: String,
    detail: String,
    evidence_refs: Vec<String>,
    recommendation: &str,
) {
    let evidence_key = evidence_refs.join("|");
    let hash = sha256_hex(&[source_ref, rule_id, category, evidence_key.as_str()]);
    findings.push(IncidentMonitorSourceReadinessFinding {
        finding_id: format!("srf_{}", hash.get(..16).unwrap_or(hash.as_str())),
        rule_id: rule_id.to_string(),
        category: category.to_string(),
        severity: severity.to_string(),
        title,
        detail,
        evidence_refs,
        recommendation: recommendation.to_string(),
    });
}

fn source_ref(project_id: &str, source_id: Option<&str>) -> String {
    match source_id {
        Some(source_id) => format!("{project_id}/{source_id}"),
        None => project_id.to_string(),
    }
}

fn evidence_path(source_id: Option<&str>, field: &str) -> String {
    match source_id {
        Some(source_id) => {
            format!("incident_monitor.config.monitored_projects[].log_sources[{source_id}].{field}")
        }
        None => format!("incident_monitor.config.monitored_projects[].{field}"),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{IncidentMonitorLogSource, IncidentMonitorSourceKind};

    const NOW_MS: u64 = 10_000;

    #[test]
    fn ready_source_passes_all_readiness_gates() {
        let project = ready_project();
        let rows = evaluate_source_readiness(
            &IncidentMonitorConfig {
                monitored_projects: vec![project],
                ..IncidentMonitorConfig::default()
            },
            &IncidentMonitorLogWatcherStatus::default(),
            NOW_MS,
        );

        assert!(rows.iter().all(|row| row.ready));
        assert!(rows.iter().all(|row| row.findings.is_empty()));
    }

    #[test]
    fn missing_lineage_produces_structured_finding() {
        let mut project = ready_project();
        project.data_readiness.source_of_truth = None;
        project.data_readiness.lineage_ref = None;

        let row = project_readiness(project);

        assert!(!row.ready);
        assert!(row
            .missing
            .contains(&"source_of_truth_or_lineage_ref".to_string()));
        assert_finding(&row, "source_lineage_missing", "high");
    }

    #[test]
    fn stale_source_produces_freshness_finding() {
        let mut project = ready_project();
        project.data_readiness.last_observed_at_ms = Some(1_000);
        project.data_readiness.freshness_sla_ms = Some(1_000);

        let row = project_readiness(project);

        assert!(!row.ready);
        assert_eq!(row.stale_after_ms, Some(2_000));
        assert_finding(&row, "source_stale", "high");
    }

    #[test]
    fn missing_tenant_workspace_context_is_high_severity() {
        let mut project = ready_project();
        project.tenant_id = None;
        project.workspace_id = None;

        let row = project_readiness(project);

        assert!(!row.ready);
        assert!(row.missing.contains(&"tenant_id".to_string()));
        assert!(row.missing.contains(&"workspace_id".to_string()));
        assert_finding(&row, "source_boundary_context_missing", "high");
    }

    #[test]
    fn schema_drift_blocks_readiness() {
        let mut project = ready_project();
        project.event_schema_version = Some("payments.v1".to_string());
        project.data_readiness.expected_schema_version = Some("payments.v2".to_string());
        project.data_readiness.schema_drift_status = Some("drift_detected".to_string());

        let row = project_readiness(project);

        assert!(!row.ready);
        assert_finding(&row, "source_schema_version_mismatch", "high");
        assert_finding(&row, "source_schema_drift_detected", "high");
    }

    #[test]
    fn missing_redaction_and_retention_profiles_are_reported() {
        let mut project = ready_project();
        project.redaction_profile = None;
        project.retention_profile = None;

        let row = project_readiness(project);

        assert!(!row.ready);
        assert_finding(&row, "source_redaction_profile_missing", "high");
        assert_finding(&row, "source_retention_profile_missing", "medium");
    }

    #[test]
    fn readiness_output_omits_raw_paths_and_authorization_markers() {
        let mut project = ready_project();
        project.workspace_root = "/tmp/raw/customer-data".to_string();
        project.data_readiness.authorization_marker =
            Some("secret-authorization-token".to_string());
        project.log_sources = vec![IncidentMonitorLogSource {
            source_id: "worker".to_string(),
            path: "/tmp/raw/customer-data/service.log".to_string(),
            ..IncidentMonitorLogSource::default()
        }];

        let rows = evaluate_source_readiness(
            &IncidentMonitorConfig {
                monitored_projects: vec![project],
                ..IncidentMonitorConfig::default()
            },
            &IncidentMonitorLogWatcherStatus::default(),
            NOW_MS,
        );
        let serialized = serde_json::to_string(&json!(rows)).expect("serialize readiness");

        assert!(!serialized.contains("/tmp/raw/customer-data"));
        assert!(!serialized.contains("secret-authorization-token"));
    }

    fn project_readiness(
        project: IncidentMonitorMonitoredProject,
    ) -> IncidentMonitorSourceReadiness {
        evaluate_source_readiness(
            &IncidentMonitorConfig {
                monitored_projects: vec![project],
                ..IncidentMonitorConfig::default()
            },
            &IncidentMonitorLogWatcherStatus::default(),
            NOW_MS,
        )
        .into_iter()
        .next()
        .expect("project readiness")
    }

    fn assert_finding(row: &IncidentMonitorSourceReadiness, rule_id: &str, severity: &str) {
        assert!(
            row.findings
                .iter()
                .any(|finding| finding.rule_id == rule_id && finding.severity == severity),
            "missing finding {rule_id}/{severity}: {:?}",
            row.findings
        );
    }

    fn ready_project() -> IncidentMonitorMonitoredProject {
        IncidentMonitorMonitoredProject {
            project_id: "payments".to_string(),
            name: "Payments".to_string(),
            enabled: true,
            paused: false,
            repo: "frumu/payments".to_string(),
            workspace_root: "/tmp/payments".to_string(),
            source_kind: IncidentMonitorSourceKind::ExternalApp,
            tenant_id: Some("tenant-a".to_string()),
            workspace_id: Some("workspace-a".to_string()),
            event_schema_version: Some("payments.v1".to_string()),
            redaction_profile: Some("standard".to_string()),
            retention_profile: Some("90d".to_string()),
            data_readiness: IncidentMonitorSourceReadinessConfig {
                source_owner: Some("payments-platform".to_string()),
                system_of_record: Some("payments-ledger".to_string()),
                data_classification: Some("confidential".to_string()),
                allowed_use: Some("incident triage and governance review".to_string()),
                source_of_truth: Some("payments-ledger".to_string()),
                lineage_ref: Some("lineage://payments".to_string()),
                freshness_sla_ms: Some(60_000),
                last_observed_at_ms: Some(NOW_MS),
                expected_schema_version: Some("payments.v1".to_string()),
                schema_drift_status: Some("stable".to_string()),
                quality_notes: Some("complete for production incidents".to_string()),
                legal_basis: Some("contract".to_string()),
                authorization_marker: Some("deployment-approved".to_string()),
            },
            ..IncidentMonitorMonitoredProject::default()
        }
    }
}
