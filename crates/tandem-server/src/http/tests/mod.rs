// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

pub(super) mod acme_slack_demo_e2e;
pub(super) mod agent_teams;
pub(super) mod approval_gate_matrix;
pub(super) mod approvals_aggregator;
pub(super) mod audit;
pub(super) mod automation_webhook_management;
pub(super) mod automation_webhooks;
pub(super) mod automation_webhooks_linear;
pub(super) mod capabilities;
pub(super) mod channel_automation_drafts;
pub(super) mod channel_interactions;
pub(super) mod channels;
pub(super) mod coder;
pub(super) mod context_packs;
pub(super) mod context_run_ledger;
pub(super) mod context_run_mutation_checkpoints;
pub(super) mod context_runs;
pub(super) mod global;
pub(super) mod global_tool_execute;
pub(super) mod governance;
pub(super) mod governance_adversarial;
pub(super) mod governance_policy_decisions;
pub(super) mod incident_monitor;
pub(super) mod intra_tenant_authority;
pub(super) mod marketplace;
pub(super) mod mcp;
pub(super) mod mcp_admin_hardening;
pub(super) mod memory;
pub(super) mod mission_builder;
pub(super) mod missions;
pub(super) mod observability_metrics;
pub(super) mod operator_tools;
pub(super) mod optimizations;
pub(super) mod orchestration_goal_plan_execute_verify_proof;
pub(super) mod orchestration_goals;
pub(super) mod orchestration_tools;
pub(super) mod pack_builder;
pub(super) mod packs;
pub(super) mod permissions;
pub(super) mod presets;
pub(super) mod providers;
pub(super) mod resources;
pub(super) mod routines;
pub(super) mod sessions;
pub(super) mod setup_understanding;
pub(super) mod slack_events;
pub(super) mod slack_events_governance;
pub(super) mod stateful_runtime_hardening;
pub(super) mod stateful_runtime_observability_contracts;
pub(super) mod task_intake;
pub(super) mod workflow_learning;
pub(super) mod workflow_planner;
pub(super) mod workflows;

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::Request;
use std::time::Duration;
use tandem_core::{
    AgentRegistry, CancellationRegistry, ConfigStore, EngineLoop, EventBus, PermissionManager,
    PluginRegistry, Storage, ToolPolicyContext, ToolPolicyHook,
};
use tandem_providers::ProviderRegistry;
use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
use tandem_tools::ToolRegistry;
use tokio::sync::broadcast;
use tower::ServiceExt;
use uuid::Uuid;

use crate::http::global::sanitize_relative_subpath;

pub(super) use crate::test_support::{next_event_of_type, test_state};

struct FixedCompletionProvider {
    response: String,
}

#[async_trait::async_trait]
impl tandem_providers::Provider for FixedCompletionProvider {
    fn info(&self) -> tandem_types::ProviderInfo {
        tandem_types::ProviderInfo {
            id: "fixed-completion-test".to_string(),
            name: "Fixed Completion Test".to_string(),
            models: vec![tandem_types::ModelInfo {
                id: "fixed-completion-test-1".to_string(),
                provider_id: "fixed-completion-test".to_string(),
                display_name: "Fixed Completion Test 1".to_string(),
                context_window: 8_192,
            }],
        }
    }

    async fn complete(
        &self,
        _prompt: &str,
        _model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        Ok(self.response.clone())
    }

    async fn complete_with_auth_override(
        &self,
        _prompt: &str,
        _model_override: Option<&str>,
        _auth_override: tandem_providers::ProviderAuthOverride,
    ) -> anyhow::Result<String> {
        Ok(self.response.clone())
    }
}

pub(super) async fn install_fixed_completion_provider(state: &AppState, response: &str) {
    state
        .providers
        .replace_for_test(
            vec![Arc::new(FixedCompletionProvider {
                response: response.to_string(),
            })],
            Some("fixed-completion-test".to_string()),
        )
        .await;
}

pub(super) fn write_pack_zip(path: &std::path::Path, manifest: &str) {
    write_pack_zip_with_entries(path, manifest, &[("README.md", "# pack")]);
}

pub(super) fn write_pack_zip_with_entries(
    path: &std::path::Path,
    manifest: &str,
    extra_entries: &[(&str, &str)],
) {
    let file = std::fs::File::create(path).expect("create zip");
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("tandempack.yaml", opts)
        .expect("start marker");
    std::io::Write::write_all(&mut zip, manifest.as_bytes()).expect("write marker");
    for (name, body) in extra_entries {
        zip.start_file(*name, opts).expect("start extra entry");
        std::io::Write::write_all(&mut zip, body.as_bytes()).expect("write extra entry");
    }
    zip.finish().expect("finish zip");
}

pub(super) struct TrustedPackKeyGuard {
    previous: Option<String>,
}

impl Drop for TrustedPackKeyGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.as_deref() {
            std::env::set_var("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", previous);
        } else {
            std::env::remove_var("TANDEM_PACK_TRUSTED_PUBLIC_KEYS");
        }
    }
}

pub(super) fn write_signed_pack_zip(path: &std::path::Path, manifest: &str) -> TrustedPackKeyGuard {
    write_signed_pack_zip_with_entries(path, manifest, &[("README.md", "# pack")])
}

pub(super) fn write_signed_pack_zip_with_entries(
    path: &std::path::Path,
    manifest: &str,
    extra_entries: &[(&str, &str)],
) -> TrustedPackKeyGuard {
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};

    let mut entries = vec![("tandempack.yaml", manifest)];
    entries.extend_from_slice(extra_entries);
    let mut ordered = entries
        .iter()
        .map(|(name, body)| ((*name).to_string(), body.as_bytes().to_vec()))
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (name, body) in &ordered {
        hasher.update((name.len() as u64).to_be_bytes());
        hasher.update(name.as_bytes());
        hasher.update((body.len() as u64).to_be_bytes());
        hasher.update(body);
    }
    let digest: [u8; 32] = hasher.finalize().into();
    let signing_key = SigningKey::from_bytes(&[11u8; 32]);
    let signature = signing_key.sign(&digest);
    let envelope = json!({
        "key_id": "http-test-publisher",
        "signature": base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
    })
    .to_string();

    let file = std::fs::File::create(path).expect("create signed zip");
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in entries {
        zip.start_file(name, opts).expect("start signed entry");
        std::io::Write::write_all(&mut zip, body.as_bytes()).expect("write signed entry");
    }
    zip.start_file("tandempack.sig", opts)
        .expect("start pack signature");
    std::io::Write::write_all(&mut zip, envelope.as_bytes()).expect("write pack signature");
    zip.finish().expect("finish signed zip");

    let previous = std::env::var("TANDEM_PACK_TRUSTED_PUBLIC_KEYS").ok();
    std::env::set_var(
        "TANDEM_PACK_TRUSTED_PUBLIC_KEYS",
        format!(
            "http-test-publisher={}",
            base64::engine::general_purpose::STANDARD
                .encode(signing_key.verifying_key().to_bytes())
        ),
    );
    TrustedPackKeyGuard { previous }
}

pub(super) fn write_plain_zip_without_marker(path: &std::path::Path) {
    let file = std::fs::File::create(path).expect("create zip");
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("README.md", opts).expect("start readme");
    std::io::Write::write_all(&mut zip, b"# not a pack").expect("write readme");
    zip.start_file("agents/a.txt", opts)
        .expect("start agents file");
    std::io::Write::write_all(&mut zip, b"agent body").expect("write agents file");
    zip.finish().expect("finish zip");
}
