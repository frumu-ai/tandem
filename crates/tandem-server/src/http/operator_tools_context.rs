use super::*;

fn tenant_matches(left: &TenantContext, right: &TenantContext) -> bool {
    left.org_id == right.org_id
        && left.workspace_id == right.workspace_id
        && left.deployment_id == right.deployment_id
}

fn planner_url(session_id: &str) -> String {
    format!("/#/planner?session_id={session_id}")
}

pub(crate) async fn product_capabilities(
    state: &AppState,
    tenant: &TenantContext,
    chat_session: &Session,
) -> anyhow::Result<Value> {
    let providers = state.providers.list().await;
    let mcp_servers = state
        .mcp
        .list_public()
        .await
        .into_values()
        .map(|server| {
            json!({
                "name": server.name,
                "enabled": server.enabled,
                "purpose": server.purpose,
                "grounding_required": server.grounding_required,
            })
        })
        .collect::<Vec<_>>();
    let workspace = state.workspace_index.snapshot().await;
    Ok(json!({
        "ok": true,
        "action": "inspect_capabilities",
        "tenant": {
            "org_id": tenant.org_id,
            "workspace_id": tenant.workspace_id,
            "deployment_id": tenant.deployment_id,
        },
        "workspace": {
            "root": chat_session.workspace_root.as_deref().unwrap_or(&workspace.root),
            "project_id": chat_session.project_id,
        },
        "providers": providers,
        "mcp_servers": mcp_servers,
        "channels": ["control_panel", "slack", "discord", "telegram"],
        "memory": {
            "session_context": true,
            "project_memory": true,
            "global_memory": true,
            "embedding_search": true,
        },
        "authoring": {
            "workflow_planner": true,
            "automation_drafts": true,
            "orchestrations": true,
            "materialization_default": "disabled_draft",
        },
        "secrets_included": false,
        "blockers": [],
    }))
}

pub(crate) async fn operator_artifact_context(
    state: &AppState,
    tenant: &TenantContext,
    chat_session_id: &str,
) -> Value {
    let mut sessions = state
        .list_workflow_planner_sessions(None)
        .await
        .into_iter()
        .filter(|session| {
            tenant_matches(&session.tenant_context, tenant)
                && session.linked_chat_session_id.as_deref() == Some(chat_session_id)
        })
        .collect::<Vec<_>>();
    sessions.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    let latest_reference = sessions
        .iter()
        .filter_map(|session| session.last_referenced_at_ms)
        .max();
    let referenced = latest_reference.and_then(|latest| {
        let mut matches = sessions
            .iter()
            .filter(|session| session.last_referenced_at_ms == Some(latest));
        let selected = matches.next();
        (matches.next().is_none()).then_some(selected).flatten()
    });
    let active = referenced.or_else(|| (sessions.len() == 1).then(|| &sessions[0]));
    json!({
        "selection": if active.is_some() { "single_active" } else if sessions.is_empty() { "none" } else { "ambiguous" },
        "active": active.map(|session| json!({
            "planner_session_id": session.session_id,
            "plan_id": session.current_plan_id,
            "revision": session.draft.as_ref().map(|draft| draft.plan_revision),
            "url": planner_url(&session.session_id),
        })),
        "recent": sessions.into_iter().take(5).map(|session| json!({
            "planner_session_id": session.session_id,
            "title": session.title,
            "plan_id": session.current_plan_id,
            "revision": session.draft.as_ref().map(|draft| draft.plan_revision),
            "operation": session.operation,
            "artifact_links": session.artifact_links,
            "updated_at_ms": session.updated_at_ms,
            "url": planner_url(&session.session_id),
        })).collect::<Vec<_>>(),
    })
}
