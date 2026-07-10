use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tandem_data_boundary::{
    evaluate_provider_egress, ProviderEgressAuditEvent, ProviderEgressAuthority,
    ProviderEgressDisposition, ProviderEgressField, ProviderEgressRequest, SensitiveDataClass,
};
use tandem_providers::ProviderRegistry;

use crate::types::{MemoryError, MemoryResult};

pub type MemoryProviderEgressAuditFuture =
    Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'static>>;
pub type MemoryProviderEgressAuditSink = Arc<
    dyn Fn(ProviderEgressAuditEvent) -> MemoryProviderEgressAuditFuture + Send + Sync + 'static,
>;
pub type MemoryProviderEgressApprovalFuture =
    Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'static>>;
pub type MemoryProviderEgressApprovalHandler =
    Arc<dyn Fn(ProviderEgressAuditEvent) -> MemoryProviderEgressApprovalFuture + Send + Sync>;

/// Trusted semantic origin of a memory provider payload. These labels are
/// supplied by the owning memory operation rather than inferred from raw
/// content, so policy still applies when detector regexes find no spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryProviderEgressKind {
    Distillation,
    Consolidation,
    ContextLayer,
    RecursiveRetrieval,
}

impl MemoryProviderEgressKind {
    fn data_classes(self) -> &'static [SensitiveDataClass] {
        match self {
            Self::Distillation | Self::Consolidation => &[
                SensitiveDataClass::CustomerData,
                SensitiveDataClass::ProprietaryBusinessData,
            ],
            Self::ContextLayer | Self::RecursiveRetrieval => &[
                SensitiveDataClass::CustomerData,
                SensitiveDataClass::SourceCode,
                SensitiveDataClass::Legal,
                SensitiveDataClass::ProprietaryBusinessData,
            ],
        }
    }
}

/// Execution authority and audit sink for memory-originated provider calls.
/// Legacy constructors omit this context; strict enforcement then fails closed
/// instead of silently attributing hosted memory to the local tenant.
#[derive(Clone, Default)]
pub struct MemoryProviderEgressContext {
    pub authority: ProviderEgressAuthority,
    pub audit_sink: Option<MemoryProviderEgressAuditSink>,
    pub approval_handler: Option<MemoryProviderEgressApprovalHandler>,
}

impl MemoryProviderEgressContext {
    pub fn new(authority: ProviderEgressAuthority) -> Self {
        Self {
            authority,
            audit_sink: None,
            approval_handler: None,
        }
    }

    pub fn with_audit_sink(mut self, audit_sink: MemoryProviderEgressAuditSink) -> Self {
        self.audit_sink = Some(audit_sink);
        self
    }

    pub fn with_approval_handler(
        mut self,
        approval_handler: MemoryProviderEgressApprovalHandler,
    ) -> Self {
        self.approval_handler = Some(approval_handler);
        self
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn complete_memory_prompt(
    providers: &ProviderRegistry,
    prompt: &str,
    provider_override: Option<&str>,
    model_override: Option<&str>,
    egress: Option<&MemoryProviderEgressContext>,
    kind: MemoryProviderEgressKind,
    operation_id: &str,
    source_ref: &str,
) -> MemoryResult<String> {
    let route = providers
        .resolve_cheapest_completion_route(provider_override, model_override)
        .await
        .map_err(|error| MemoryError::InvalidConfig(error.to_string()))?;
    let default_authority = ProviderEgressAuthority::default();
    let authority = egress
        .map(|context| &context.authority)
        .unwrap_or(&default_authority);
    let fields = [ProviderEgressField::transformable("prompt", prompt)];
    let request = ProviderEgressRequest {
        authority,
        operation_id,
        source_ref,
        provider_id: route.provider_id.as_str(),
        model_id: route.model_id.as_deref(),
        fields: &fields,
        data_classes: kind.data_classes(),
        action_tags: &[],
    };
    let mut evaluation = evaluate_provider_egress(&request);
    let approval_event = evaluation.event.clone();
    if let Some(event) = evaluation.event.take() {
        if let Some(sink) = egress.and_then(|context| context.audit_sink.as_ref()) {
            sink(event)
                .await
                .map_err(MemoryError::TenantScopeViolation)?;
        } else {
            return Err(MemoryError::TenantScopeViolation(format!(
                "DATA_BOUNDARY_AUDIT_UNAVAILABLE: memory provider dispatch cannot persist decision {}",
                event.boundary.decision_id
            )));
        }
    }
    let permit = match evaluation.disposition {
        ProviderEgressDisposition::Off | ProviderEgressDisposition::Proceed => evaluation
            .take_dispatch_permit()
            .map_err(MemoryError::TenantScopeViolation)?,
        ProviderEgressDisposition::RequireApproval => {
            let handler = egress
                .and_then(|context| context.approval_handler.as_ref())
                .ok_or_else(|| {
                    MemoryError::TenantScopeViolation(
                        "DATA_BOUNDARY_APPROVAL_UNAVAILABLE: memory provider dispatch has no approval continuation"
                            .to_string(),
                    )
                })?;
            let event = approval_event.ok_or_else(|| {
                MemoryError::TenantScopeViolation(
                    "DATA_BOUNDARY_APPROVAL_INVALID: approval event is unavailable".to_string(),
                )
            })?;
            let approval_ref = handler(event)
                .await
                .map_err(MemoryError::TenantScopeViolation)?;
            evaluation
                .take_approval()
                .map_err(MemoryError::TenantScopeViolation)?
                .approve(approval_ref)
                .map_err(MemoryError::TenantScopeViolation)?
        }
        ProviderEgressDisposition::Blocked => {
            return Err(MemoryError::TenantScopeViolation(
                evaluation
                    .blocked_reason
                    .unwrap_or_else(|| "DATA_BOUNDARY_BLOCKED".to_string()),
            ));
        }
    };
    let prepared_prompt = evaluation
        .transformed_fields
        .and_then(|mut fields| fields.drain(..).next())
        .unwrap_or_else(|| prompt.to_string());
    providers
        .complete_with_egress_permit(
            &permit,
            Some(route.provider_id.as_str()),
            &prepared_prompt,
            route.model_id.as_deref(),
        )
        .await
        .map_err(|error| MemoryError::InvalidConfig(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};
    use tandem_data_boundary::DataBoundaryTenantRef;
    use tandem_providers::{AppConfig, Provider};
    use tandem_types::ProviderInfo;

    struct CaptureProvider {
        prompt: Arc<Mutex<Option<String>>>,
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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
    fn memory_kinds_carry_trusted_semantic_classes() {
        assert_eq!(
            MemoryProviderEgressKind::Distillation.data_classes(),
            &[
                SensitiveDataClass::CustomerData,
                SensitiveDataClass::ProprietaryBusinessData,
            ]
        );
        assert_eq!(
            MemoryProviderEgressKind::Consolidation.data_classes(),
            MemoryProviderEgressKind::Distillation.data_classes()
        );
        for kind in [
            MemoryProviderEgressKind::ContextLayer,
            MemoryProviderEgressKind::RecursiveRetrieval,
        ] {
            assert!(kind
                .data_classes()
                .contains(&SensitiveDataClass::SourceCode));
            assert!(kind.data_classes().contains(&SensitiveDataClass::Legal));
            assert!(kind
                .data_classes()
                .contains(&SensitiveDataClass::CustomerData));
            assert!(kind
                .data_classes()
                .contains(&SensitiveDataClass::ProprietaryBusinessData));
        }
    }

    #[async_trait]
    impl Provider for CaptureProvider {
        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                id: "capture".to_string(),
                name: "Capture".to_string(),
                models: Vec::new(),
            }
        }

        async fn complete(
            &self,
            prompt: &str,
            _model_override: Option<&str>,
        ) -> anyhow::Result<String> {
            *self.prompt.lock().expect("capture lock") = Some(prompt.to_string());
            Ok("[]".to_string())
        }
    }

    #[tokio::test]
    async fn strict_redaction_never_sends_raw_memory_prompt() {
        let _guard = env_lock();
        let _env = EnvRestore::set(&[
            ("TANDEM_DATA_BOUNDARY_MODE", "enforce"),
            ("TANDEM_DATA_BOUNDARY_STRICT", "1"),
            (
                "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
                "capture=approved_external",
            ),
            ("TANDEM_DATA_BOUNDARY_REDACT_CLASSES", "pii,credential"),
        ]);

        let captured = Arc::new(Mutex::new(None));
        let providers = ProviderRegistry::new(AppConfig::default());
        providers
            .replace_for_test(
                vec![Arc::new(CaptureProvider {
                    prompt: captured.clone(),
                })],
                Some("capture".to_string()),
            )
            .await;
        let authority = ProviderEgressAuthority::new(DataBoundaryTenantRef {
            organization_id: Some("org-a".to_string()),
            workspace_id: Some("workspace-a".to_string()),
            deployment_id: None,
        })
        .with_run_id("run-a")
        .with_session_id("session-a");
        let context = MemoryProviderEgressContext::new(authority)
            .with_audit_sink(Arc::new(|_| Box::pin(async { Ok(()) })));
        complete_memory_prompt(
            &providers,
            "email alice@example.com api_key=sk-test-secret-123456",
            None,
            None,
            Some(&context),
            MemoryProviderEgressKind::Distillation,
            "memory-test",
            "memory.test",
        )
        .await
        .expect("guarded completion");

        let prompt = captured
            .lock()
            .expect("capture lock")
            .clone()
            .expect("captured prompt");
        assert!(!prompt.contains("alice@example.com"));
        assert!(!prompt.contains("sk-test-secret"));
        assert!(prompt.contains("[REDACTED:"));
    }

    #[tokio::test]
    async fn semantic_approval_uses_continuation_and_missing_continuation_fails_closed() {
        let _guard = env_lock();
        let _env = EnvRestore::set(&[
            ("TANDEM_DATA_BOUNDARY_MODE", "enforce"),
            ("TANDEM_DATA_BOUNDARY_STRICT", "1"),
            (
                "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
                "capture=approved_external",
            ),
            ("TANDEM_DATA_BOUNDARY_APPROVAL_CLASSES", "customer_data"),
        ]);

        let captured = Arc::new(Mutex::new(None));
        let providers = ProviderRegistry::new(AppConfig::default());
        providers
            .replace_for_test(
                vec![Arc::new(CaptureProvider {
                    prompt: captured.clone(),
                })],
                Some("capture".to_string()),
            )
            .await;
        let authority = ProviderEgressAuthority::new(DataBoundaryTenantRef {
            organization_id: Some("org-a".to_string()),
            workspace_id: Some("workspace-a".to_string()),
            deployment_id: None,
        })
        .with_run_id("run-a")
        .with_session_id("session-a");
        let context = MemoryProviderEgressContext::new(authority.clone())
            .with_audit_sink(Arc::new(|_| Box::pin(async { Ok(()) })))
            .with_approval_handler(Arc::new(|_| {
                Box::pin(async { Ok("approval-memory-1".to_string()) })
            }));
        complete_memory_prompt(
            &providers,
            "ordinary memory content",
            None,
            None,
            Some(&context),
            MemoryProviderEgressKind::Distillation,
            "memory-approval",
            "memory.test",
        )
        .await
        .expect("approval continuation authorizes dispatch");
        assert_eq!(
            captured.lock().expect("capture lock").as_deref(),
            Some("ordinary memory content")
        );

        let error = complete_memory_prompt(
            &providers,
            "ordinary memory content",
            None,
            None,
            Some(
                &MemoryProviderEgressContext::new(authority)
                    .with_audit_sink(Arc::new(|_| Box::pin(async { Ok(()) }))),
            ),
            MemoryProviderEgressKind::Distillation,
            "memory-no-approval",
            "memory.test",
        )
        .await
        .expect_err("missing continuation must fail closed");
        assert!(error.to_string().contains("no approval continuation"));
    }

    #[tokio::test]
    async fn required_audit_failure_blocks_memory_provider_dispatch() {
        let _guard = env_lock();
        let _env = EnvRestore::set(&[
            ("TANDEM_DATA_BOUNDARY_MODE", "enforce"),
            ("TANDEM_DATA_BOUNDARY_STRICT", "1"),
            (
                "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
                "capture=approved_external",
            ),
            ("TANDEM_DATA_BOUNDARY_REDACT_CLASSES", "pii"),
        ]);

        let captured = Arc::new(Mutex::new(None));
        let providers = ProviderRegistry::new(AppConfig::default());
        providers
            .replace_for_test(
                vec![Arc::new(CaptureProvider {
                    prompt: captured.clone(),
                })],
                Some("capture".to_string()),
            )
            .await;
        let context =
            MemoryProviderEgressContext::new(ProviderEgressAuthority::new(DataBoundaryTenantRef {
                organization_id: Some("org-a".to_string()),
                workspace_id: Some("workspace-a".to_string()),
                deployment_id: None,
            }))
            .with_audit_sink(Arc::new(|_| {
                Box::pin(async { Err("protected ledger unavailable".to_string()) })
            }));

        let error = complete_memory_prompt(
            &providers,
            "email alice@example.com",
            None,
            None,
            Some(&context),
            MemoryProviderEgressKind::Distillation,
            "memory-audit-failure",
            "memory.test",
        )
        .await
        .expect_err("audit failure must block provider dispatch");
        assert!(error.to_string().contains("protected ledger unavailable"));
        assert!(captured.lock().expect("capture lock").is_none());
    }
}
