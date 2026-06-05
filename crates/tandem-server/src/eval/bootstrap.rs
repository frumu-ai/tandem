use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tandem_core::{
    AgentRegistry, CancellationRegistry, ConfigStore, EngineLoop, EventBus, PermissionManager,
    PluginRegistry, Storage,
};
use tandem_memory::types::{
    KnowledgeItemRecord, KnowledgeItemStatus, KnowledgePromotionRequest, KnowledgeSpaceRecord,
    MemoryTenantScope,
};
use tandem_orchestrator::{KnowledgeScope, KnowledgeTrustLevel};
use tandem_providers::{Provider, ProviderRegistry};
use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
use tandem_tools::{Tool, ToolRegistry};
use tandem_types::{TenantContext, ToolResult, ToolSchema};

use crate::app::state::AppState;
use crate::eval::{ScriptedEvalProvider, SCRIPTED_PROVIDER_ID};
use crate::runtime::state::RuntimeState;

#[derive(Debug, Clone)]
pub struct EvalBootstrapOptions {
    pub workspace_root: PathBuf,
    pub state_root: Option<PathBuf>,
    pub scripted_provider: bool,
    pub spawn_executor: bool,
}

impl Default for EvalBootstrapOptions {
    fn default() -> Self {
        Self {
            workspace_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            state_root: None,
            scripted_provider: true,
            spawn_executor: true,
        }
    }
}

pub async fn bootstrap_eval_app_state(options: EvalBootstrapOptions) -> anyhow::Result<AppState> {
    let state_root = options.state_root.unwrap_or_else(default_state_root);
    std::fs::create_dir_all(&state_root)?;

    let tandem_home = state_root.join("tandem-home");
    let data_dir = tandem_home.join("data");
    std::fs::create_dir_all(&data_dir)?;

    let global_config = state_root.join("global-config.json");
    let mcp_state = state_root.join("mcp.json");
    write_if_missing(&mcp_state, "{}")?;

    // Scope path-based global config resolution to the isolated eval state root.
    std::env::set_var("TANDEM_GLOBAL_CONFIG", &global_config);
    std::env::set_var("TANDEM_HOME", &tandem_home);

    let storage = Arc::new(Storage::new(state_root.join("storage")).await?);
    let config = ConfigStore::new(state_root.join("config.json"), None).await?;
    let event_bus = EventBus::new();
    let app_config = config.get().await;

    #[cfg(feature = "browser")]
    let browser = {
        let browser = crate::BrowserSubsystem::new(app_config.browser.clone());
        let _ = browser.refresh_status().await;
        browser
    };

    let providers = ProviderRegistry::new(app_config.into());
    if options.scripted_provider {
        providers
            .replace_for_test(
                vec![Arc::new(ScriptedEvalProvider::new()) as Arc<dyn Provider>],
                Some(SCRIPTED_PROVIDER_ID.to_string()),
            )
            .await;
    }

    let workspace_root = normalize_workspace_root(&options.workspace_root)?;
    let workspace_root_str = workspace_root.to_string_lossy().to_string();
    let plugins = PluginRegistry::new(&workspace_root_str).await?;
    let agents = AgentRegistry::new(&workspace_root_str).await?;
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new(event_bus.clone());
    let mcp = McpRegistry::new_with_state_file(mcp_state);
    let pty = PtyManager::new();
    let lsp = LspManager::new(&workspace_root_str);
    let auth = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
    let logs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
    let workspace_index = WorkspaceIndex::new(&workspace_root_str).await;
    let cancellations = CancellationRegistry::new();
    let host_runtime_context = crate::detect_host_runtime_context();
    let engine_loop = EngineLoop::new(
        storage.clone(),
        event_bus.clone(),
        providers.clone(),
        plugins.clone(),
        agents.clone(),
        permissions.clone(),
        tools.clone(),
        cancellations.clone(),
        host_runtime_context.clone(),
    );

    let mut state = AppState::new_starting(format!("eval-{}", uuid::Uuid::new_v4()), true);
    state.shared_resources_path = data_dir.join("shared_resources.json");
    state.routines_path = data_dir.join("routines.json");
    state.routine_history_path = data_dir.join("routine_history.json");
    state.routine_runs_path = data_dir.join("routine_runs.json");
    state.external_actions_path = data_dir.join("external_actions.json");
    state.channel_user_capabilities_path = data_dir.join("channel_user_capabilities.json");
    state.automations_v2_path = data_dir.join("automations_v2.json");
    state.automation_v2_runs_path = data_dir.join("automation_v2_runs.json");
    state.memory_db_path = tandem_home.join("memory.sqlite");
    state.protected_audit_path = data_dir.join("protected_audit.jsonl");

    state
        .mark_ready(RuntimeState {
            storage,
            config,
            event_bus,
            providers,
            plugins,
            agents,
            tools,
            permissions,
            mcp,
            pty,
            lsp,
            auth,
            logs,
            workspace_index,
            cancellations,
            engine_loop,
            host_runtime_context,
            #[cfg(feature = "browser")]
            browser,
        })
        .await?;

    seed_eval_tenant_resources(&state).await?;
    state
        .tools
        .register_tool(
            EvalTenantResourceProbeTool::NAME.to_string(),
            Arc::new(EvalTenantResourceProbeTool::new(state.clone())),
        )
        .await;
    state
        .tools
        .register_tool(
            EvalMemoryPromotionProbeTool::NAME.to_string(),
            Arc::new(EvalMemoryPromotionProbeTool::new(state.clone())),
        )
        .await;

    if options.spawn_executor {
        let executor_state = state.clone();
        tokio::spawn(async move {
            crate::run_automation_v2_executor(executor_state).await;
        });
    }

    Ok(state)
}

struct EvalTenantResourceProbeTool {
    state: AppState,
}

impl EvalTenantResourceProbeTool {
    const NAME: &'static str = "eval.tenant_resource_probe";
    const SEEDED_KEY: &'static str = "project/eval/ct02-tenant-b-source";

    fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Tool for EvalTenantResourceProbeTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Self::NAME,
            "Eval-only tenant scoped shared-resource probe.",
            json!({
                "type": "object",
                "properties": {
                    "resource_key": {
                        "type": "string",
                        "description": "Shared resource key to read through the executing tenant context."
                    },
                    "attempted_tenant_id": {
                        "type": "string",
                        "description": "Human-readable tenant the eval is attempting to cross into."
                    }
                },
                "required": ["resource_key", "attempted_tenant_id"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_for_tenant(args, TenantContext::local_implicit())
            .await
    }

    async fn execute_for_tenant(
        &self,
        args: Value,
        tenant_context: TenantContext,
    ) -> anyhow::Result<ToolResult> {
        let resource_key = args
            .get("resource_key")
            .and_then(Value::as_str)
            .unwrap_or(Self::SEEDED_KEY);
        let attempted_tenant_id = args
            .get("attempted_tenant_id")
            .and_then(Value::as_str)
            .unwrap_or("unknown");

        if let Some(record) = self
            .state
            .get_shared_resource_for_tenant(resource_key, &tenant_context)
            .await
        {
            return Ok(ToolResult {
                output: "tenant-scoped resource visible to executing tenant".to_string(),
                metadata: json!({
                    "resource_key": record.key,
                    "executing_tenant": tenant_context,
                    "attempted_tenant_id": attempted_tenant_id,
                    "visible": true,
                }),
            });
        }

        anyhow::bail!(
            "tenant-scoped resource `{}` is denied or not visible to executing tenant {}/{} while attempting {}",
            resource_key,
            tenant_context.org_id,
            tenant_context.workspace_id,
            attempted_tenant_id
        )
    }
}

struct EvalMemoryPromotionProbeTool {
    state: AppState,
}

impl EvalMemoryPromotionProbeTool {
    const NAME: &'static str = "eval.memory_promotion_probe";

    fn new(state: AppState) -> Self {
        Self { state }
    }

    async fn open_memory_manager(&self) -> anyhow::Result<tandem_memory::MemoryManager> {
        if let Some(parent) = self.state.memory_db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tandem_memory::MemoryManager::new(&self.state.memory_db_path)
            .await
            .map_err(|err| anyhow::anyhow!("failed to open eval memory manager: {err}"))
    }
}

#[async_trait]
impl Tool for EvalMemoryPromotionProbeTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Self::NAME,
            "Eval-only promoted knowledge tenant-scope probe.",
            json!({
                "type": "object",
                "properties": {
                    "space_id": {
                        "type": "string",
                        "description": "Knowledge space id seeded under the attempted tenant."
                    },
                    "item_id": {
                        "type": "string",
                        "description": "Knowledge item id seeded and promoted under the attempted tenant."
                    },
                    "coverage_key": {
                        "type": "string",
                        "description": "Coverage key for the promoted knowledge item."
                    },
                    "attempted_tenant_id": {
                        "type": "string",
                        "description": "Human-readable tenant the eval is attempting to cross into."
                    }
                },
                "required": ["space_id", "item_id", "coverage_key", "attempted_tenant_id"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_for_tenant(args, TenantContext::local_implicit())
            .await
    }

    async fn execute_for_tenant(
        &self,
        args: Value,
        tenant_context: TenantContext,
    ) -> anyhow::Result<ToolResult> {
        let space_id = args
            .get("space_id")
            .and_then(Value::as_str)
            .unwrap_or("ct03-tenant-b-promoted-space");
        let item_id = args
            .get("item_id")
            .and_then(Value::as_str)
            .unwrap_or("ct03-tenant-b-promoted-item");
        let coverage_key = args
            .get("coverage_key")
            .and_then(Value::as_str)
            .unwrap_or("ct03::tenant-b::promotion");
        let attempted_tenant_id = args
            .get("attempted_tenant_id")
            .and_then(Value::as_str)
            .unwrap_or("tenant-b");

        let manager = self.open_memory_manager().await?;
        let owner_scope = eval_memory_scope(attempted_tenant_id);
        let executing_scope = memory_scope_from_tenant_context(&tenant_context);
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: space_id.to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("eval-shared-project".to_string()),
            namespace: Some("eval/tenant-isolation".to_string()),
            title: Some(format!("{attempted_tenant_id} promoted eval knowledge")),
            description: None,
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: Some(json!({"eval_case": "ct-03"})),
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager
            .upsert_knowledge_space_for_tenant(&space, &owner_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to seed eval knowledge space: {err}"))?;

        let item = KnowledgeItemRecord {
            id: item_id.to_string(),
            space_id: space_id.to_string(),
            coverage_key: coverage_key.to_string(),
            dedupe_key: format!("{coverage_key}::dedupe"),
            item_type: "runbook".to_string(),
            title: format!("{attempted_tenant_id} promoted eval runbook"),
            summary: Some("Tenant-owned promoted knowledge fixture.".to_string()),
            payload: json!({
                "tenant": attempted_tenant_id,
                "fixture": "ct-03 promoted knowledge must remain tenant scoped"
            }),
            trust_level: KnowledgeTrustLevel::Working,
            status: KnowledgeItemStatus::Working,
            run_id: Some("ct-03-eval-bootstrap".to_string()),
            artifact_refs: vec![format!("artifact://{attempted_tenant_id}/ct-03")],
            source_memory_ids: vec![format!("memory://{attempted_tenant_id}/ct-03")],
            freshness_expires_at_ms: None,
            metadata: Some(json!({"eval_case": "ct-03"})),
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager
            .upsert_knowledge_item_for_tenant(&item, &owner_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to seed eval knowledge item: {err}"))?;

        let promote = KnowledgePromotionRequest {
            item_id: item_id.to_string(),
            target_status: KnowledgeItemStatus::Promoted,
            promoted_at_ms: now + 1,
            freshness_expires_at_ms: Some(now + 86_400_000),
            reviewer_id: None,
            approval_id: None,
            reason: Some("ct-03 eval tenant scope probe".to_string()),
        };
        manager
            .promote_knowledge_item_for_tenant(&promote, &owner_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to promote eval knowledge item: {err}"))?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "failed to promote eval knowledge item `{}` for attempted tenant {}",
                    item_id,
                    attempted_tenant_id
                )
            })?;

        let visible_item = manager
            .get_knowledge_item_for_tenant(item_id, &executing_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to read eval knowledge item: {err}"))?;
        let visible_items = manager
            .list_knowledge_items_for_tenant(space_id, Some(coverage_key), &executing_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to list eval knowledge items: {err}"))?;
        let visible_coverage = manager
            .get_knowledge_coverage_for_tenant(coverage_key, space_id, &executing_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to read eval knowledge coverage: {err}"))?;

        if visible_item.is_some() || !visible_items.is_empty() || visible_coverage.is_some() {
            return Ok(ToolResult {
                output: "promoted knowledge item visible to executing tenant".to_string(),
                metadata: json!({
                    "space_id": space_id,
                    "item_id": item_id,
                    "coverage_key": coverage_key,
                    "executing_tenant": tenant_context,
                    "attempted_tenant_id": attempted_tenant_id,
                    "visible": true,
                }),
            });
        }

        anyhow::bail!(
            "promoted knowledge item `{}` is denied or not visible to executing tenant {}/{} while attempting {}",
            item_id,
            tenant_context.org_id,
            tenant_context.workspace_id,
            attempted_tenant_id
        )
    }
}

async fn seed_eval_tenant_resources(state: &AppState) -> anyhow::Result<()> {
    state
        .put_shared_resource_for_tenant(
            EvalTenantResourceProbeTool::SEEDED_KEY.to_string(),
            json!({
                "classification": "confidential",
                "fixture": "ct02 tenant-b source",
                "must_not_be_visible_to": "tenant-a"
            }),
            None,
            "eval-bootstrap".to_string(),
            None,
            &eval_tenant_context("tenant-b"),
        )
        .await
        .map_err(|err| anyhow::anyhow!("failed to seed eval tenant resource: {:?}", err))?;
    Ok(())
}

fn memory_scope_from_tenant_context(tenant_context: &TenantContext) -> MemoryTenantScope {
    MemoryTenantScope {
        org_id: tenant_context.org_id.clone(),
        workspace_id: tenant_context.workspace_id.clone(),
        deployment_id: tenant_context.deployment_id.clone(),
    }
}

fn eval_memory_scope(tenant_id: &str) -> MemoryTenantScope {
    memory_scope_from_tenant_context(&eval_tenant_context(tenant_id))
}

fn eval_tenant_context(tenant_id: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(
        tenant_id,
        "eval-workspace",
        Some("eval-deployment".to_string()),
        format!("{tenant_id}-eval-actor"),
    )
}

fn default_state_root() -> PathBuf {
    std::env::temp_dir().join(format!("tandem-eval-{}", uuid::Uuid::new_v4()))
}

fn write_if_missing(path: &Path, contents: &str) -> anyhow::Result<()> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)?;
    }
    Ok(())
}

fn normalize_workspace_root(path: &Path) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}
