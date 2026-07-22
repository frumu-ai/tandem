// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::{sync::Arc, time::Duration};

use tandem_core::{
    evaluate_dispatch_boundary, DataBoundaryDispatchContext, DataBoundaryDispatchOutcome,
    PermissionManager,
};
use tandem_data_boundary::{
    DataBoundaryTenantRef, ProviderEgressAuditEvent, ProviderEgressAuthority, ProviderEgressPermit,
    SensitiveDataClass,
};
use tandem_memory::{MemoryProviderEgressApprovalHandler, MemoryProviderEgressContext};
use tandem_providers::ChatMessage;
use tandem_types::{EngineEvent, TenantContext, VerifiedTenantContext};
use tokio_util::sync::CancellationToken;

use crate::AppState;

const PROVIDER_EGRESS_APPROVAL_TIMEOUT: Duration = Duration::from_secs(30);

async fn persist_and_publish_dispatch_event(
    state: &AppState,
    mut event: EngineEvent,
) -> Result<(), String> {
    let recorded = crate::data_boundary_bridge::record_data_boundary_protected_audit(state, &event)
        .await
        .map_err(|error| {
            format!(
                "DATA_BOUNDARY_AUDIT_FAILED: provider dispatch denied because protected audit persistence failed: {error:#}"
            )
        })?;
    if recorded {
        crate::data_boundary_bridge::mark_data_boundary_protected_audit_recorded(&mut event);
    }
    state.event_bus.publish(event);
    Ok(())
}

async fn wait_for_provider_egress_reply(
    permissions: &PermissionManager,
    request_id: &str,
) -> (Option<String>, bool) {
    permissions
        .wait_for_reply_with_timeout(
            request_id,
            CancellationToken::new(),
            Some(PROVIDER_EGRESS_APPROVAL_TIMEOUT),
        )
        .await
}

pub(crate) fn authority_for_dispatch(
    tenant_context: Option<&TenantContext>,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    run_id: Option<&str>,
    session_id: Option<&str>,
) -> ProviderEgressAuthority {
    let tenant = tenant_context
        .filter(|tenant| !tenant.is_local_implicit())
        .map(|tenant| DataBoundaryTenantRef {
            organization_id: Some(tenant.org_id.clone()),
            workspace_id: Some(tenant.workspace_id.clone()),
            deployment_id: tenant.deployment_id.clone(),
        })
        .unwrap_or_default();
    ProviderEgressAuthority {
        tenant,
        run_id: run_id.map(str::to_string),
        session_id: session_id.map(str::to_string),
        authority_ref: verified_tenant_context.map(|verified| verified.assertion_id.clone()),
    }
}

pub(crate) fn memory_egress_context(
    state: &AppState,
    tenant_context: Option<&TenantContext>,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    run_id: Option<&str>,
    session_id: Option<&str>,
) -> MemoryProviderEgressContext {
    let authority =
        authority_for_dispatch(tenant_context, verified_tenant_context, run_id, session_id);
    let audit_state = state.clone();
    let context = MemoryProviderEgressContext::new(authority).with_audit_sink(Arc::new(
        move |event: ProviderEgressAuditEvent| {
            let state = audit_state.clone();
            Box::pin(async move {
                let event_name = event.boundary.event_name.clone();
                let properties = serde_json::to_value(event).map_err(|error| {
                    format!("DATA_BOUNDARY_AUDIT_FAILED: serialize memory decision: {error}")
                })?;
                persist_and_publish_dispatch_event(&state, EngineEvent::new(event_name, properties))
                    .await
            })
        },
    ));
    let permissions = state
        .runtime
        .get()
        .map(|runtime| runtime.permissions.clone());
    let approval_session_id = session_id.map(str::to_string);
    let approval_tenant_context = tenant_context
        .cloned()
        .unwrap_or_else(TenantContext::local_implicit);
    let approval_handler: MemoryProviderEgressApprovalHandler = Arc::new(move |event| {
        let permissions = permissions.clone();
        let session_id = approval_session_id.clone();
        let tenant_context = approval_tenant_context.clone();
        Box::pin(async move {
            let permissions = permissions.ok_or_else(|| {
                "DATA_BOUNDARY_APPROVAL_UNAVAILABLE: memory runtime is not initialized".to_string()
            })?;
            let pending = permissions
                .ask_for_session_for_tenant(
                    &tenant_context,
                    session_id.as_deref(),
                    "data_boundary_egress",
                    serde_json::json!({
                        "kind": "memory_provider_egress",
                        "decisionID": event.boundary.decision_id,
                        "payloadHash": event.boundary.payload_hash,
                        "findingSummary": event.boundary.finding_summary,
                        "semanticDataClasses": event.semantic_data_classes,
                        "reasonCodes": event.boundary.reason_codes,
                    }),
                )
                .await;
            let (reply, timed_out) =
                wait_for_provider_egress_reply(&permissions, &pending.id).await;
            if matches!(
                reply.as_deref(),
                Some("once") | Some("always") | Some("allow")
            ) {
                Ok(pending.id)
            } else {
                Err(format!(
                    "DATA_BOUNDARY_APPROVAL_REQUIRED: memory provider dispatch approval {}",
                    if timed_out { "timed out" } else { "was denied" }
                ))
            }
        })
    });
    context.with_approval_handler(approval_handler)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServerProviderEgressKind {
    MissionBuilder,
    WorkflowPlanner,
    KnowledgeBase,
}

impl ServerProviderEgressKind {
    fn data_classes(self) -> &'static [SensitiveDataClass] {
        match self {
            Self::MissionBuilder | Self::WorkflowPlanner => &[
                SensitiveDataClass::CustomerData,
                SensitiveDataClass::SourceCode,
                SensitiveDataClass::ProprietaryBusinessData,
            ],
            Self::KnowledgeBase => &[
                SensitiveDataClass::CustomerData,
                SensitiveDataClass::Legal,
                SensitiveDataClass::ProprietaryBusinessData,
            ],
        }
    }
}

pub(crate) struct PreparedChatDispatch {
    pub(crate) messages: Vec<ChatMessage>,
    pub(crate) permit: ProviderEgressPermit,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare_chat_messages(
    state: &AppState,
    tenant_context: Option<&TenantContext>,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    run_id: Option<&str>,
    session_id: &str,
    operation_id: &str,
    source_ref: &str,
    kind: ServerProviderEgressKind,
    provider_id: &str,
    model_id: Option<&str>,
    messages: &[ChatMessage],
) -> Result<PreparedChatDispatch, String> {
    let approval_tenant_context = tenant_context
        .cloned()
        .unwrap_or_else(TenantContext::local_implicit);
    let authority = authority_for_dispatch(
        tenant_context,
        verified_tenant_context,
        run_id,
        Some(session_id),
    );
    match evaluate_dispatch_boundary(
        &DataBoundaryDispatchContext {
            session_id,
            run_id,
            message_id: operation_id,
            correlation_id: None,
            provider_id,
            model_id,
            tool_schema_payload: None,
            source_ref,
            data_classes: kind.data_classes(),
            authority_ref: authority.authority_ref.as_deref(),
            org_id: authority.tenant.organization_id.as_deref(),
            workspace_id: authority.tenant.workspace_id.as_deref(),
            deployment_id: authority.tenant.deployment_id.as_deref(),
        },
        messages,
    ) {
        DataBoundaryDispatchOutcome::Off { permit } => Ok(PreparedChatDispatch {
            messages: messages.to_vec(),
            permit,
        }),
        DataBoundaryDispatchOutcome::Proceed { event, permit } => {
            persist_and_publish_dispatch_event(state, event).await?;
            Ok(PreparedChatDispatch {
                messages: messages.to_vec(),
                permit,
            })
        }
        DataBoundaryDispatchOutcome::ProceedTransformed {
            event,
            messages,
            permit,
        } => {
            persist_and_publish_dispatch_event(state, event).await?;
            Ok(PreparedChatDispatch { messages, permit })
        }
        DataBoundaryDispatchOutcome::RequireApproval {
            event,
            evidence,
            denial_reason,
            approval,
        } => {
            persist_and_publish_dispatch_event(state, event).await?;
            let permissions = state
                .runtime
                .get()
                .map(|runtime| runtime.permissions.clone())
                .ok_or_else(|| {
                    format!(
                        "{denial_reason} (direct provider runtime has no approval continuation)"
                    )
                })?;
            let pending = permissions
                .ask_for_session_for_tenant(
                    &approval_tenant_context,
                    Some(session_id),
                    "data_boundary_egress",
                    evidence,
                )
                .await;
            let (reply, timed_out) =
                wait_for_provider_egress_reply(&permissions, &pending.id).await;
            if !matches!(
                reply.as_deref(),
                Some("once") | Some("always") | Some("allow")
            ) {
                return Err(format!(
                    "{denial_reason} ({})",
                    if timed_out {
                        "approval timed out"
                    } else {
                        "approval denied"
                    }
                ));
            }
            Ok(PreparedChatDispatch {
                messages: messages.to_vec(),
                permit: approval.approve(pending.id)?,
            })
        }
        DataBoundaryDispatchOutcome::Blocked { event, reason } => {
            persist_and_publish_dispatch_event(state, event).await?;
            Err(reason)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    const APPROVAL_RESPONDER_TIMEOUT: Duration = Duration::from_secs(5);

    struct EnvRestore(Vec<(&'static str, Option<OsString>)>);

    impl EnvRestore {
        fn set(values: &[(&'static str, &str)]) -> Self {
            let restore = Self(
                values
                    .iter()
                    .map(|(name, _)| (*name, std::env::var_os(name)))
                    .collect(),
            );
            for (name, value) in values {
                std::env::set_var(name, value);
            }
            restore
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (name, value) in self.0.drain(..) {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    #[test]
    fn direct_provider_surfaces_have_trusted_semantic_classes() {
        for kind in [
            ServerProviderEgressKind::MissionBuilder,
            ServerProviderEgressKind::WorkflowPlanner,
        ] {
            assert!(kind
                .data_classes()
                .contains(&SensitiveDataClass::SourceCode));
            assert!(kind
                .data_classes()
                .contains(&SensitiveDataClass::CustomerData));
            assert!(kind
                .data_classes()
                .contains(&SensitiveDataClass::ProprietaryBusinessData));
        }
        let kb = ServerProviderEgressKind::KnowledgeBase.data_classes();
        assert!(kb.contains(&SensitiveDataClass::Legal));
        assert!(kb.contains(&SensitiveDataClass::CustomerData));
        assert!(kb.contains(&SensitiveDataClass::ProprietaryBusinessData));
    }

    #[tokio::test]
    #[serial_test::serial(data_boundary_env)]
    async fn direct_provider_approval_continuation_activates_route_bound_permit() {
        let _env = EnvRestore::set(&[
            ("TANDEM_DATA_BOUNDARY_MODE", "enforce"),
            ("TANDEM_DATA_BOUNDARY_STRICT", "1"),
            (
                "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
                "test-provider=approved_external",
            ),
            ("TANDEM_DATA_BOUNDARY_APPROVAL_CLASSES", "customer_data"),
        ]);

        let state = crate::test_support::test_state().await;
        let mut published_events = state.event_bus.subscribe();
        let mut events = state.event_bus.subscribe();
        let permissions = state.runtime.wait().permissions.clone();
        let reply_task = tokio::spawn(async move {
            tokio::time::timeout(APPROVAL_RESPONDER_TIMEOUT, async move {
                loop {
                    let event = events.recv().await.expect("permission event");
                    if event.event_type != "permission.asked" {
                        continue;
                    }
                    let request_id = event.properties["requestID"]
                        .as_str()
                        .expect("permission request id");
                    assert!(permissions.reply(request_id, "allow").await);
                    break;
                }
            })
            .await
            .expect("approval responder timed out");
        });
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let messages = [ChatMessage {
            role: "user".to_string(),
            content: "ordinary mission content".to_string(),
            attachments: Vec::new(),
        }];
        let prepared = tokio::time::timeout(
            APPROVAL_RESPONDER_TIMEOUT,
            prepare_chat_messages(
                &state,
                Some(&tenant),
                None,
                Some("run-a"),
                "session-a",
                "operation-a",
                "server.mission_builder",
                ServerProviderEgressKind::MissionBuilder,
                "test-provider",
                Some("test-model"),
                &messages,
            ),
        )
        .await
        .expect("approval preparation timed out")
        .expect("approved direct dispatch");
        reply_task.await.expect("approval reply task");
        assert_eq!(prepared.messages.len(), 1);
        assert_eq!(prepared.messages[0].role, messages[0].role);
        assert_eq!(prepared.messages[0].content, messages[0].content);
        assert!(prepared.permit.approval_ref().is_some());
        let fields = [
            tandem_data_boundary::ProviderEgressField::untransformable(
                "message.0.role",
                prepared.messages[0].role.as_str(),
            ),
            tandem_data_boundary::ProviderEgressField::transformable(
                "message.0.content",
                prepared.messages[0].content.as_str(),
            ),
        ];
        assert!(prepared
            .permit
            .ensure_request(
                Some("test-provider"),
                Some("test-model"),
                &tandem_data_boundary::provider_egress_payload_hash(&fields),
            )
            .is_ok());

        let boundary_event = tokio::time::timeout(APPROVAL_RESPONDER_TIMEOUT, async {
            loop {
                let event = published_events.recv().await.expect("published event");
                if event.event_type.starts_with("data_boundary.") {
                    break event;
                }
            }
        })
        .await
        .expect("boundary event timed out");
        let ledger_before_bridge = tokio::fs::read_to_string(&state.protected_audit_path)
            .await
            .expect("protected audit ledger");
        assert_eq!(ledger_before_bridge.lines().count(), 1);
        assert!(
            !crate::data_boundary_bridge::record_data_boundary_protected_audit(
                &state,
                &boundary_event,
            )
            .await
            .expect("bridge replay"),
            "the bridge must skip synchronously persisted provider evidence"
        );
        let ledger_after_bridge = tokio::fs::read_to_string(&state.protected_audit_path)
            .await
            .expect("protected audit ledger after bridge replay");
        assert_eq!(ledger_after_bridge.lines().count(), 1);
    }

    #[tokio::test]
    #[serial_test::serial(data_boundary_env)]
    async fn protected_audit_failure_blocks_direct_provider_permit() {
        let _env = EnvRestore::set(&[
            ("TANDEM_DATA_BOUNDARY_MODE", "audit"),
            (
                "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
                "test-provider=prohibited",
            ),
        ]);

        let mut state = crate::test_support::test_state().await;
        let failed_ledger_path = state
            .protected_audit_path
            .with_file_name("protected-audit-path-is-a-directory");
        tokio::fs::create_dir_all(&failed_ledger_path)
            .await
            .expect("create invalid ledger path");
        state.protected_audit_path = failed_ledger_path;
        let mut events = state.event_bus.subscribe();
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let messages = [ChatMessage {
            role: "user".to_string(),
            content: "ordinary mission content".to_string(),
            attachments: Vec::new(),
        }];

        let result = prepare_chat_messages(
            &state,
            Some(&tenant),
            None,
            Some("run-audit-failure"),
            "session-audit-failure",
            "operation-audit-failure",
            "server.mission_builder",
            ServerProviderEgressKind::MissionBuilder,
            "test-provider",
            Some("test-model"),
            &messages,
        )
        .await;
        let error = match result {
            Ok(_) => panic!("audit failure must prevent a dispatch permit"),
            Err(error) => error,
        };

        assert!(error.contains("DATA_BOUNDARY_AUDIT_FAILED"), "{error}");
        assert!(
            events.try_recv().is_err(),
            "failed audit must not publish an authorization-like boundary event"
        );
    }
}
