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
pub(super) mod memory;
pub(super) mod mission_builder;
pub(super) mod missions;
pub(super) mod observability_metrics;
pub(super) mod optimizations;
pub(super) mod orchestration_goals;
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
