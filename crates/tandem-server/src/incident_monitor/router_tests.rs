// Router unit tests, split out of router.rs to stay under the file-size
// gate (2000-line hard cap). Included via `#[path] mod tests;`.

use super::*;
use crate::{
    IncidentMonitorLogSource, IncidentMonitorMonitoredProject, IncidentMonitorSafetyDefaults,
    IncidentMonitorSourceKind,
};

fn source_bound_config() -> IncidentMonitorConfig {
    IncidentMonitorConfig {
        monitored_projects: vec![IncidentMonitorMonitoredProject {
            project_id: "payments".to_string(),
            name: "Payments".to_string(),
            repo: "acme/payments".to_string(),
            workspace_root: "/tmp/payments".to_string(),
            source_kind: IncidentMonitorSourceKind::ExternalApp,
            allowed_destination_ids: vec!["triage".to_string(), "pager".to_string()],
            default_destination_ids: vec!["triage".to_string()],
            default_route_tags: vec!["payments".to_string()],
            tenant_id: Some("tenant-payments".to_string()),
            workspace_id: Some("workspace-project".to_string()),
            event_schema_version: Some("project-v1".to_string()),
            log_sources: vec![IncidentMonitorLogSource {
                source_id: "ci".to_string(),
                path: "logs/ci.jsonl".to_string(),
                source_kind: Some(IncidentMonitorSourceKind::Ci),
                allowed_destination_ids: vec!["pager".to_string()],
                default_destination_ids: vec!["pager".to_string()],
                default_route_tags: vec!["ci".to_string()],
                tenant_id: Some("tenant-ci".to_string()),
                workspace_id: Some("workspace-ci".to_string()),
                event_schema_version: Some("ci-v1".to_string()),
                approval_policy: IncidentMonitorApprovalPolicy::Never,
                ..IncidentMonitorLogSource::default()
            }],
            ..IncidentMonitorMonitoredProject::default()
        }],
        ..IncidentMonitorConfig::default()
    }
}

#[test]
fn untrusted_report_source_ids_do_not_inherit_source_binding_routes() {
    let report = IncidentMonitorSubmission {
        project_id: Some("payments".to_string()),
        log_source_id: Some("ci".to_string()),
        source_kind: Some(IncidentMonitorSourceKind::ExternalApp),
        route_tags: vec!["forged".to_string()],
        allowed_destination_ids: vec!["pager".to_string()],
        default_destination_ids: vec!["pager".to_string()],
        tenant_id: Some("tenant-forged".to_string()),
        workspace_id: Some("workspace-forged".to_string()),
        event_schema_version: Some("forged-v1".to_string()),
        source_approval_policy: None,
        ..IncidentMonitorSubmission::default()
    };

    let context = build_route_context(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
        Some(&report),
        None,
        None,
    );
    let enriched = enrich_route_context_from_sources(&source_bound_config(), &context);

    assert_eq!(enriched.project_id, None);
    assert_eq!(enriched.log_source_id, None);
    assert_eq!(enriched.source_kind, None);
    assert_eq!(enriched.tenant_id, None);
    assert_eq!(enriched.workspace_id, None);
    assert_eq!(enriched.event_schema_version, None);
    assert!(enriched.route_tags.is_empty());
    assert!(enriched.allowed_destination_ids.is_empty());
    assert!(enriched.default_destination_ids.is_empty());
    assert!(!enriched.source_approval_policy_trusted);
}

#[test]
fn legacy_persisted_draft_source_ids_inherit_fail_closed_source_binding() {
    let mut config = source_bound_config();
    config.monitored_projects[0].log_sources[0].approval_policy =
        IncidentMonitorApprovalPolicy::Always;
    let draft = IncidentMonitorDraftRecord {
        project_id: Some("payments".to_string()),
        log_source_id: Some("ci".to_string()),
        source_approval_policy: None,
        ..IncidentMonitorDraftRecord::default()
    };

    let context = build_route_context(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
        None,
        Some(&draft),
        None,
    );
    let enriched = enrich_route_context_from_sources(&config, &context);

    assert_eq!(enriched.project_id.as_deref(), Some("payments"));
    assert_eq!(enriched.log_source_id.as_deref(), Some("ci"));
    assert_eq!(enriched.source_kind.as_deref(), Some("ci"));
    assert_eq!(enriched.default_destination_ids, vec!["pager"]);
    assert_eq!(
        enriched.source_approval_policy,
        Some(IncidentMonitorApprovalPolicy::Always)
    );
    assert!(enriched.source_approval_policy_trusted);
}

#[test]
fn trusted_report_source_binding_overrides_forged_source_fields() {
    let report = IncidentMonitorSubmission {
        project_id: Some("payments".to_string()),
        log_source_id: Some("ci".to_string()),
        source_kind: Some(IncidentMonitorSourceKind::ExternalApp),
        route_tags: vec!["candidate".to_string()],
        allowed_destination_ids: vec!["triage".to_string(), "pager".to_string()],
        default_destination_ids: vec!["triage".to_string()],
        tenant_id: Some("tenant-forged".to_string()),
        workspace_id: Some("workspace-forged".to_string()),
        event_schema_version: Some("forged-v1".to_string()),
        source_approval_policy: Some(IncidentMonitorApprovalPolicy::Never),
        ..IncidentMonitorSubmission::default()
    };

    let context = build_route_context(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
        Some(&report),
        None,
        None,
    );
    let enriched = enrich_route_context_from_sources(&source_bound_config(), &context);

    assert_eq!(enriched.project_id.as_deref(), Some("payments"));
    assert_eq!(enriched.log_source_id.as_deref(), Some("ci"));
    assert_eq!(enriched.source_kind.as_deref(), Some("ci"));
    assert_eq!(enriched.tenant_id.as_deref(), Some("tenant-ci"));
    assert_eq!(enriched.workspace_id.as_deref(), Some("workspace-ci"));
    assert_eq!(enriched.event_schema_version.as_deref(), Some("ci-v1"));
    assert_eq!(enriched.route_tags, vec!["candidate", "payments", "ci"]);
    assert_eq!(enriched.allowed_destination_ids, vec!["pager"]);
    assert_eq!(enriched.default_destination_ids, vec!["pager"]);
    assert_eq!(
        enriched.source_approval_policy,
        Some(IncidentMonitorApprovalPolicy::Never)
    );
    assert!(enriched.source_approval_policy_trusted);
}

#[test]
fn publish_audit_tenant_context_uses_source_bound_draft_scope() {
    let draft = IncidentMonitorDraftRecord {
        tenant_id: Some("tenant-draft".to_string()),
        workspace_id: Some("workspace-draft".to_string()),
        ..IncidentMonitorDraftRecord::default()
    };
    let context = IncidentMonitorRouteContext {
        tenant_id: Some("tenant-ci".to_string()),
        workspace_id: Some("workspace-ci".to_string()),
        ..IncidentMonitorRouteContext::default()
    };

    let tenant_context = publish_audit_tenant_context(&context, &draft);

    assert_eq!(tenant_context.org_id, "tenant-ci");
    assert_eq!(tenant_context.workspace_id, "workspace-ci");
    assert_eq!(tenant_context.actor_id, None);
    assert_eq!(tenant_context.source, tandem_types::TenantSource::Explicit);
}

#[test]
fn publish_audit_tenant_context_falls_back_to_local_without_complete_scope() {
    let draft = IncidentMonitorDraftRecord {
        tenant_id: Some("tenant-draft".to_string()),
        ..IncidentMonitorDraftRecord::default()
    };
    let context = IncidentMonitorRouteContext::default();

    let tenant_context = publish_audit_tenant_context(&context, &draft);

    assert!(tenant_context.is_local_implicit());
}

#[test]
fn route_preview_matches_risk_category() {
    let config = IncidentMonitorConfig {
        destinations: vec![IncidentMonitorDestinationConfig {
            destination_id: "security-linear".to_string(),
            name: "Security Linear".to_string(),
            kind: IncidentMonitorDestinationKind::LinearIssue,
            ..IncidentMonitorDestinationConfig::default()
        }],
        routes: vec![IncidentMonitorRouteConfig {
            route_id: "security-risk".to_string(),
            name: "Security Risk".to_string(),
            destination_ids: vec!["security-linear".to_string()],
            match_risk_categories: vec!["data_exfiltration".to_string()],
            ..IncidentMonitorRouteConfig::default()
        }],
        default_destination_ids: vec!["legacy-github".to_string()],
        ..IncidentMonitorConfig::default()
    };
    let context = build_route_context(
        None,
        None,
        None,
        None,
        Some("data_exfiltration"),
        None,
        None,
        None,
        None,
        &[],
        None,
        None,
        None,
    );
    let destinations = config.effective_destinations();
    let preview = build_route_preview(&config, &destinations, &[], &[], &context, &[]);

    assert_eq!(
        preview.effective_destination_ids,
        vec!["security-linear".to_string()]
    );
    assert_eq!(
        preview
            .matches
            .first()
            .and_then(|row| row.reason.as_deref()),
        Some("matched_risk_category")
    );
}

#[test]
fn route_preview_prefers_highest_priority_route_over_overlapping_catch_all() {
    // TAN-543: when more than one route matches, the highest-priority route
    // wins and its destination is the only effective one. Previously the
    // matcher unioned destinations across all matching routes, which then
    // hard-failed every publish for any event that matched an overlapping
    // catch-all route.
    let config = IncidentMonitorConfig {
        destinations: vec![
            IncidentMonitorDestinationConfig {
                destination_id: "security-linear".to_string(),
                name: "Security Linear".to_string(),
                kind: IncidentMonitorDestinationKind::LinearIssue,
                ..IncidentMonitorDestinationConfig::default()
            },
            IncidentMonitorDestinationConfig {
                destination_id: "legacy-github".to_string(),
                name: "Legacy GitHub".to_string(),
                kind: IncidentMonitorDestinationKind::GithubIssue,
                ..IncidentMonitorDestinationConfig::default()
            },
        ],
        routes: vec![
            IncidentMonitorRouteConfig {
                route_id: "catch-all".to_string(),
                name: "Catch all".to_string(),
                enabled: true,
                priority: 0,
                destination_ids: vec!["legacy-github".to_string()],
                ..IncidentMonitorRouteConfig::default()
            },
            IncidentMonitorRouteConfig {
                route_id: "security-risk".to_string(),
                name: "Security Risk".to_string(),
                enabled: true,
                priority: 100,
                destination_ids: vec!["security-linear".to_string()],
                match_risk_categories: vec!["data_exfiltration".to_string()],
                ..IncidentMonitorRouteConfig::default()
            },
        ],
        default_destination_ids: vec!["legacy-github".to_string()],
        ..IncidentMonitorConfig::default()
    };
    let context = build_route_context(
        None,
        None,
        None,
        None,
        Some("data_exfiltration"),
        None,
        None,
        None,
        None,
        &[],
        None,
        None,
        None,
    );
    let destinations = config.effective_destinations();
    let preview = build_route_preview(&config, &destinations, &[], &[], &context, &[]);

    // Both routes match...
    assert!(
        preview.matches.len() >= 2,
        "both routes should appear as matches: {:?}",
        preview.matches
    );
    // ...but only the higher-priority route's destination is effective.
    assert_eq!(
        preview.effective_destination_ids,
        vec!["security-linear".to_string()]
    );
    // A single effective destination satisfies the one-destination phase limit,
    // so validate_publish_plan no longer hard-fails on overlapping routes.
    assert_eq!(preview.destinations.len(), 1);
}

#[test]
fn preview_and_publish_approval_decisions_agree() {
    // TAN-557: the preview approval flag must reflect the publish gate exactly,
    // across risk levels and approval policies.
    let destinations = vec![IncidentMonitorDestinationConfig {
        destination_id: "gh".to_string(),
        name: "GitHub".to_string(),
        kind: IncidentMonitorDestinationKind::GithubIssue,
        require_approval: true,
        ..IncidentMonitorDestinationConfig::default()
    }];
    let config = IncidentMonitorConfig {
        destinations: destinations.clone(),
        ..IncidentMonitorConfig::default()
    };
    let ids = vec!["gh".to_string()];
    for policy in [
        IncidentMonitorApprovalPolicy::Inherit,
        IncidentMonitorApprovalPolicy::HighRisk,
        IncidentMonitorApprovalPolicy::Always,
        IncidentMonitorApprovalPolicy::Never,
    ] {
        let route = IncidentMonitorRouteConfig {
            route_id: "r".to_string(),
            approval_policy: policy.clone(),
            destination_ids: ids.clone(),
            ..IncidentMonitorRouteConfig::default()
        };
        for risk in [None, Some("low"), Some("high")] {
            let context = build_route_context(
                None,
                None,
                None,
                risk,
                None,
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                None,
            );
            assert_eq!(
                route_preview_approval_required(
                    Some(&route),
                    &context,
                    &config,
                    &destinations,
                    &ids
                ),
                route_publish_match_approval_required(
                    &config,
                    Some(&route),
                    &context,
                    &destinations,
                    &ids
                ),
                "preview must match publish for policy={policy:?} risk={risk:?}"
            );
        }
    }
}

#[test]
fn minimum_risk_level_floor_blocks_reporter_downgrade() {
    // TAN-548: a reporter labelling a high-risk incident "low" must not be
    // able to slip under the high-risk approval gate when the operator has
    // configured a server-side floor.
    let config = IncidentMonitorConfig {
        safety_defaults: IncidentMonitorSafetyDefaults {
            minimum_risk_level: Some("high".to_string()),
            ..IncidentMonitorSafetyDefaults::default()
        },
        ..IncidentMonitorConfig::default()
    };
    let context = IncidentMonitorRouteContext {
        risk_level: Some("low".to_string()),
        ..IncidentMonitorRouteContext::default()
    };
    let enriched = enrich_route_context_from_sources(&config, &context);
    assert_eq!(enriched.risk_level.as_deref(), Some("high"));
    assert!(
        is_high_risk(enriched.risk_level.as_deref()),
        "floored risk must trip the high-risk approval gate"
    );
}

#[test]
fn minimum_risk_level_floor_never_downgrades_a_higher_report() {
    // The floor only raises; a report already above the floor is untouched.
    let config = IncidentMonitorConfig {
        safety_defaults: IncidentMonitorSafetyDefaults {
            minimum_risk_level: Some("medium".to_string()),
            ..IncidentMonitorSafetyDefaults::default()
        },
        ..IncidentMonitorConfig::default()
    };
    let context = IncidentMonitorRouteContext {
        risk_level: Some("critical".to_string()),
        ..IncidentMonitorRouteContext::default()
    };
    let enriched = enrich_route_context_from_sources(&config, &context);
    assert_eq!(enriched.risk_level.as_deref(), Some("critical"));
}

#[test]
fn absent_minimum_risk_level_leaves_reporter_value_untouched() {
    let config = IncidentMonitorConfig::default();
    let context = IncidentMonitorRouteContext {
        risk_level: Some("low".to_string()),
        ..IncidentMonitorRouteContext::default()
    };
    let enriched = enrich_route_context_from_sources(&config, &context);
    assert_eq!(enriched.risk_level.as_deref(), Some("low"));
}

fn source_gate_config(block_unready_sources: bool) -> IncidentMonitorConfig {
    IncidentMonitorConfig {
        destinations: vec![IncidentMonitorDestinationConfig {
            destination_id: "gh".to_string(),
            name: "GitHub".to_string(),
            kind: IncidentMonitorDestinationKind::GithubIssue,
            enabled: true,
            repo: Some("acme/app".to_string()),
            ..IncidentMonitorDestinationConfig::default()
        }],
        default_destination_ids: vec!["gh".to_string()],
        safety_defaults: IncidentMonitorSafetyDefaults {
            block_unready_sources,
            ..IncidentMonitorSafetyDefaults::default()
        },
        monitored_projects: vec![IncidentMonitorMonitoredProject {
            project_id: "payments".to_string(),
            name: "Payments".to_string(),
            repo: "acme/app".to_string(),
            workspace_root: "/tmp/payments".to_string(),
            log_sources: vec![IncidentMonitorLogSource {
                source_id: "ci".to_string(),
                path: "logs/ci.jsonl".to_string(),
                ..IncidentMonitorLogSource::default()
            }],
            ..IncidentMonitorMonitoredProject::default()
        }],
        ..IncidentMonitorConfig::default()
    }
}

#[test]
fn block_unready_sources_gates_publish_on_not_ready_source() {
    // TAN-544: with the gate enabled, a not-ready source blocks publish
    // instead of the readiness result being advisory only; with it off the
    // same source publishes (default behavior preserved).
    // A persisted-draft context is trusted, so enrichment keeps the
    // project/source binding the readiness gate matches on.
    let draft = IncidentMonitorDraftRecord {
        draft_id: "d1".to_string(),
        fingerprint: "fp".to_string(),
        repo: "acme/app".to_string(),
        project_id: Some("payments".to_string()),
        log_source_id: Some("ci".to_string()),
        ..IncidentMonitorDraftRecord::default()
    };
    let context = build_route_context(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
        None,
        Some(&draft),
        None,
    );
    let ready_destinations = source_gate_config(true).effective_destinations();
    let destination_readiness = ready_destinations
        .iter()
        .map(|destination| IncidentMonitorDestinationReadiness {
            destination_id: destination.destination_id.clone(),
            kind: destination.kind.clone(),
            enabled: true,
            publish_ready: true,
            ..IncidentMonitorDestinationReadiness::default()
        })
        .collect::<Vec<_>>();
    let source_readiness = vec![IncidentMonitorSourceReadiness {
        project_id: "payments".to_string(),
        source_id: Some("ci".to_string()),
        ready: false,
        findings: vec![crate::IncidentMonitorSourceReadinessFinding {
            rule_id: "source_stale".to_string(),
            ..Default::default()
        }],
        ..IncidentMonitorSourceReadiness::default()
    }];

    let gated = source_gate_config(true);
    let preview = build_route_preview(
        &gated,
        &ready_destinations,
        &destination_readiness,
        &source_readiness,
        &context,
        &[],
    );
    assert!(
        preview
            .blocked_reasons
            .iter()
            .any(|reason| reason.contains("not data-ready")),
        "expected a source-readiness block: {:?}",
        preview.blocked_reasons
    );
    assert!(
        validate_publish_plan(
            &gated,
            &preview,
            incident_monitor_github::PublishMode::ManualPublish
        )
        .is_err(),
        "the source-readiness gate must block a Manual publish"
    );

    let open = source_gate_config(false);
    let preview_open = build_route_preview(
        &open,
        &ready_destinations,
        &destination_readiness,
        &source_readiness,
        &context,
        &[],
    );
    assert!(
        !preview_open
            .blocked_reasons
            .iter()
            .any(|reason| reason.contains("not data-ready")),
        "advisory-only default must not add a source block: {:?}",
        preview_open.blocked_reasons
    );
    assert!(
        validate_publish_plan(
            &open,
            &preview_open,
            incident_monitor_github::PublishMode::ManualPublish
        )
        .is_ok(),
        "with the gate off the same not-ready source must still publish"
    );
}

#[test]
fn block_unready_sources_fails_closed_when_no_readiness_row_matches() {
    // TAN-544 review: with the gate on, a bound project/source that has no
    // matching readiness row (e.g. the source was renamed/removed after triage)
    // must fail closed instead of publishing with no readiness evidence.
    let draft = IncidentMonitorDraftRecord {
        draft_id: "d1".to_string(),
        fingerprint: "fp".to_string(),
        repo: "acme/app".to_string(),
        project_id: Some("payments".to_string()),
        log_source_id: Some("ci".to_string()),
        ..IncidentMonitorDraftRecord::default()
    };
    let context = build_route_context(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        &[],
        None,
        Some(&draft),
        None,
    );
    let gated = source_gate_config(true);
    let destinations = gated.effective_destinations();
    let destination_readiness = destinations
        .iter()
        .map(|destination| IncidentMonitorDestinationReadiness {
            destination_id: destination.destination_id.clone(),
            kind: destination.kind.clone(),
            enabled: true,
            publish_ready: true,
            ..IncidentMonitorDestinationReadiness::default()
        })
        .collect::<Vec<_>>();

    // No source readiness rows at all for the bound project/source.
    let preview = build_route_preview(
        &gated,
        &destinations,
        &destination_readiness,
        &[],
        &context,
        &[],
    );
    assert!(
        preview
            .blocked_reasons
            .iter()
            .any(|reason| reason.contains("no readiness evidence")),
        "missing readiness must fail closed: {:?}",
        preview.blocked_reasons
    );
    assert!(validate_publish_plan(
        &gated,
        &preview,
        incident_monitor_github::PublishMode::ManualPublish
    )
    .is_err());
}
