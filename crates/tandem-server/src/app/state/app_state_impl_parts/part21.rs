// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use crate::http::workflow_planner::{
    WorkflowPlanDraftAccessBinding, WorkflowPlanDraftAuthority, WorkflowPlannerSessionRecord,
};

fn workflow_planner_session_draft_binding(
    session: &WorkflowPlannerSessionRecord,
) -> Option<WorkflowPlanDraftAccessBinding> {
    if session.tenant_context.is_local_implicit() {
        return None;
    }
    if let Some(source) = session.source_workflow.as_ref() {
        return Some(WorkflowPlanDraftAccessBinding::Workflow(source.clone()));
    }
    if session.source_kind.trim_start_matches("forked_") == "workflow_learning_revision"
        || session
            .tenant_context
            .actor_id
            .as_deref()
            .is_none_or(str::is_empty)
    {
        return None;
    }
    Some(WorkflowPlanDraftAccessBinding::Actor(
        session.tenant_context.clone(),
    ))
}

fn hosted_workflow_planner_session_plan_id(
    session: &WorkflowPlannerSessionRecord,
) -> anyhow::Result<Option<&str>> {
    if session.tenant_context.is_local_implicit() {
        return Ok(session
            .draft
            .as_ref()
            .map(|draft| draft.current_plan.plan_id.as_str()));
    }
    match (session.current_plan_id.as_deref(), session.draft.as_ref()) {
        (None, None) => Ok(None),
        (Some(plan_id), Some(draft))
            if plan_id == draft.current_plan.plan_id
                && plan_id == draft.initial_plan.plan_id
                && plan_id == draft.conversation.plan_id =>
        {
            Ok(Some(plan_id))
        }
        _ => anyhow::bail!("hosted planner session plan IDs must agree"),
    }
}

#[derive(Debug)]
pub(crate) struct WorkflowPlannerSessionWriteDenied;

impl std::fmt::Display for WorkflowPlannerSessionWriteDenied {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("workflow planner session write authority changed")
    }
}

impl std::error::Error for WorkflowPlannerSessionWriteDenied {}

impl AppState {
    pub async fn put_workflow_plan(&self, plan: WorkflowPlan) {
        self.workflow_plans
            .write()
            .await
            .insert(plan.plan_id.clone(), plan);
    }

    pub async fn get_workflow_plan(&self, plan_id: &str) -> Option<WorkflowPlan> {
        self.workflow_plans.read().await.get(plan_id).cloned()
    }

    pub async fn put_workflow_plan_draft(&self, draft: WorkflowPlanDraftRecord) {
        self.workflow_plan_drafts
            .write()
            .await
            .insert(draft.current_plan.plan_id.clone(), draft.clone());
        self.put_workflow_plan(draft.current_plan).await;
    }

    pub async fn get_workflow_plan_draft(&self, plan_id: &str) -> Option<WorkflowPlanDraftRecord> {
        self.workflow_plan_drafts.read().await.get(plan_id).cloned()
    }

    pub(crate) async fn workflow_plan_draft_authority(
        &self,
        plan_id: &str,
    ) -> Option<WorkflowPlanDraftAuthority> {
        self.workflow_plan_draft_authority
            .read()
            .await
            .get(plan_id)
            .cloned()
    }

    pub(crate) async fn get_workflow_plan_draft_scoped(
        &self,
        plan_id: &str,
        tenant: &tandem_types::TenantContext,
        verified: Option<&tandem_types::VerifiedTenantContext>,
    ) -> Option<WorkflowPlanDraftRecord> {
        if tenant.is_local_implicit() {
            return self.get_workflow_plan_draft(plan_id).await;
        }
        let expected = self.workflow_plan_draft_authority(plan_id).await?;
        let WorkflowPlanDraftAuthority::Bound { binding, .. } = &expected else {
            return None;
        };
        if !crate::http::workflow_planner::workflow_plan_access_binding_allowed(
            self, tenant, verified, binding, false,
        )
        .await
        {
            return None;
        }
        let authority = self.workflow_plan_draft_authority.read().await;
        if authority.get(plan_id) != Some(&expected) {
            return None;
        }
        self.workflow_plan_drafts.read().await.get(plan_id).cloned()
    }

    pub(crate) async fn put_workflow_plan_draft_scoped(
        &self,
        draft: WorkflowPlanDraftRecord,
        tenant: &tandem_types::TenantContext,
        verified: Option<&tandem_types::VerifiedTenantContext>,
        requested_binding: Option<&WorkflowPlanDraftAccessBinding>,
        new_draft_only: bool,
    ) -> anyhow::Result<()> {
        if tenant.is_local_implicit() {
            self.put_workflow_plan_draft(draft).await;
            return Ok(());
        }
        let plan_id = draft.current_plan.plan_id.clone();
        if plan_id.trim().is_empty()
            || draft.initial_plan.plan_id != plan_id
            || draft.conversation.plan_id != plan_id
        {
            anyhow::bail!("workflow plan draft IDs must agree");
        }
        let mut authority = self.workflow_plan_draft_authority.write().await;
        let previous = authority.get(&plan_id).cloned();
        if new_draft_only && previous.is_some() {
            anyhow::bail!("workflow plan ID is already in use");
        }
        let binding = match previous {
            Some(WorkflowPlanDraftAuthority::Bound { binding, .. }) => {
                if requested_binding.is_some_and(|requested| requested != &binding) {
                    anyhow::bail!("workflow plan source binding changed");
                }
                binding
            }
            Some(WorkflowPlanDraftAuthority::Denied) => {
                anyhow::bail!("workflow plan ID is ambiguous");
            }
            None => {
                if self
                    .workflow_plan_drafts
                    .read()
                    .await
                    .contains_key(&plan_id)
                {
                    anyhow::bail!("workflow plan lacks hosted provenance");
                }
                requested_binding
                    .cloned()
                    .unwrap_or_else(|| WorkflowPlanDraftAccessBinding::Actor(tenant.clone()))
            }
        };
        if !crate::http::workflow_planner::workflow_plan_access_binding_allowed(
            self, tenant, verified, &binding, true,
        )
        .await
        {
            anyhow::bail!("workflow plan authoring is not authorized");
        }
        authority
            .entry(plan_id)
            .or_insert_with(|| WorkflowPlanDraftAuthority::Bound {
                binding,
                session_id: None,
            });
        self.put_workflow_plan_draft(draft).await;
        Ok(())
    }

    pub async fn load_workflow_planner_sessions(&self) -> anyhow::Result<()> {
        let Some(raw) = read_state_file_with_legacy(
            &self.workflow_planner_sessions_path,
            "workflow_planner_sessions.json",
        )
        .await?
        else {
            return Ok(());
        };
        let parsed = serde_json::from_str::<
            std::collections::HashMap<
                String,
                crate::http::workflow_planner::WorkflowPlannerSessionRecord,
            >,
        >(&raw)
        .unwrap_or_default();
        self.replace_workflow_planner_sessions(parsed).await?;
        Ok(())
    }

    pub async fn persist_workflow_planner_sessions(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.workflow_planner_sessions_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.workflow_planner_sessions.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.workflow_planner_sessions_path, payload).await?;
        Ok(())
    }

    async fn replace_workflow_planner_sessions(
        &self,
        sessions: std::collections::HashMap<
            String,
            crate::http::workflow_planner::WorkflowPlannerSessionRecord,
        >,
    ) -> anyhow::Result<()> {
        let mut authority = self.workflow_plan_draft_authority.write().await;
        authority.clear();
        for session in sessions.values() {
            if session.tenant_context.is_local_implicit() {
                continue;
            }
            let Ok(Some(plan_id)) = hosted_workflow_planner_session_plan_id(session) else {
                continue;
            };
            let Some(binding) = workflow_planner_session_draft_binding(session) else {
                continue;
            };
            let next = WorkflowPlanDraftAuthority::Bound {
                binding,
                session_id: Some(session.session_id.clone()),
            };
            if authority.insert(plan_id.to_string(), next).is_some() {
                authority.insert(plan_id.to_string(), WorkflowPlanDraftAuthority::Denied);
            }
        }
        *self.workflow_planner_sessions.write().await = sessions.clone();
        let mut plans = self.workflow_plans.write().await;
        let mut drafts = self.workflow_plan_drafts.write().await;
        plans.clear();
        drafts.clear();
        for session in sessions.values() {
            let Some(draft) = session.draft.as_ref() else {
                continue;
            };
            let plan_id = draft.current_plan.plan_id.as_str();
            let accessible = if session.tenant_context.is_local_implicit() {
                !authority.contains_key(plan_id)
            } else {
                matches!(
                    authority.get(plan_id),
                    Some(WorkflowPlanDraftAuthority::Bound { session_id: Some(owner), .. })
                        if owner == &session.session_id
                )
            };
            if accessible {
                plans.insert(plan_id.to_string(), draft.current_plan.clone());
                drafts.insert(plan_id.to_string(), draft.clone());
            }
        }
        Ok(())
    }

    async fn sync_workflow_planner_session_cache(
        &self,
        session: &crate::http::workflow_planner::WorkflowPlannerSessionRecord,
    ) {
        if let Some(draft) = session.draft.as_ref() {
            self.workflow_plans.write().await.insert(
                draft.current_plan.plan_id.clone(),
                draft.current_plan.clone(),
            );
            self.workflow_plan_drafts
                .write()
                .await
                .insert(draft.current_plan.plan_id.clone(), draft.clone());
        }
    }

    pub async fn put_workflow_planner_session(
        &self,
        session: crate::http::workflow_planner::WorkflowPlannerSessionRecord,
    ) -> anyhow::Result<crate::http::workflow_planner::WorkflowPlannerSessionRecord> {
        self.put_workflow_planner_session_inner(session, None).await
    }

    pub(crate) async fn put_workflow_planner_session_checked(
        &self,
        session: crate::http::workflow_planner::WorkflowPlannerSessionRecord,
        tenant: &tandem_types::TenantContext,
        verified: Option<&tandem_types::VerifiedTenantContext>,
    ) -> anyhow::Result<crate::http::workflow_planner::WorkflowPlannerSessionRecord> {
        self.put_workflow_planner_session_inner(session, Some((tenant, verified)))
            .await
    }

    async fn put_workflow_planner_session_inner(
        &self,
        mut session: crate::http::workflow_planner::WorkflowPlannerSessionRecord,
        caller: Option<(
            &tandem_types::TenantContext,
            Option<&tandem_types::VerifiedTenantContext>,
        )>,
    ) -> anyhow::Result<crate::http::workflow_planner::WorkflowPlannerSessionRecord> {
        if session.session_id.trim().is_empty() {
            anyhow::bail!("session_id is required");
        }
        if session.project_slug.trim().is_empty() {
            anyhow::bail!("project_slug is required");
        }
        let plan_id = hosted_workflow_planner_session_plan_id(&session)?.map(str::to_string);
        let binding = if session.tenant_context.is_local_implicit() {
            None
        } else {
            Some(
                workflow_planner_session_draft_binding(&session).ok_or_else(|| {
                    anyhow::anyhow!("hosted planner session lacks source provenance")
                })?,
            )
        };
        let now = now_ms();
        if session.created_at_ms == 0 {
            session.created_at_ms = now;
        }
        session.updated_at_ms = now;
        let mut authority = self.workflow_plan_draft_authority.write().await;
        let mut sessions = self.workflow_planner_sessions.write().await;
        let current = if caller.is_some() {
            Some(
                sessions
                    .get(&session.session_id)
                    .cloned()
                    .ok_or_else(|| anyhow::Error::new(WorkflowPlannerSessionWriteDenied))?,
            )
        } else {
            None
        };
        let previous_plan_id = if let Some(previous) = sessions.get(&session.session_id) {
            if !previous.tenant_context.is_local_implicit()
                && (previous.tenant_context != session.tenant_context
                    || previous.source_workflow != session.source_workflow)
            {
                anyhow::bail!("hosted planner session provenance cannot change");
            }
            previous
                .draft
                .as_ref()
                .map(|draft| draft.current_plan.plan_id.clone())
        } else {
            None
        };
        if let (Some(plan_id), Some(binding)) = (plan_id.as_deref(), binding.as_ref()) {
            match authority.get(plan_id) {
                Some(WorkflowPlanDraftAuthority::Bound {
                    binding: existing_binding,
                    session_id,
                }) if existing_binding == binding
                    && session_id
                        .as_deref()
                        .is_none_or(|owner| owner == session.session_id.as_str()) => {}
                None if !self.workflow_plan_drafts.read().await.contains_key(plan_id) => {}
                _ => anyhow::bail!("workflow plan ID is already bound to another source"),
            }
        } else if let Some(plan_id) = plan_id.as_deref() {
            if authority.contains_key(plan_id) {
                anyhow::bail!("workflow plan ID is already bound to a hosted source");
            }
        }
        let previous_plan_to_remove = previous_plan_id.filter(|previous_plan_id| {
            Some(previous_plan_id.as_str()) != plan_id.as_deref()
                && matches!(
                    authority.get(previous_plan_id),
                    Some(WorkflowPlanDraftAuthority::Bound { session_id: Some(owner), .. })
                        if owner == &session.session_id
                )
        });
        let checked_binding = if let Some((tenant, _verified)) = caller {
            let current = current.as_ref().expect("checked write has current session");
            if current.tenant_context.is_local_implicit() {
                if !tenant.is_local_implicit()
                    || tenant.org_id != current.tenant_context.org_id
                    || tenant.workspace_id != current.tenant_context.workspace_id
                    || tenant.deployment_id != current.tenant_context.deployment_id
                {
                    return Err(anyhow::Error::new(WorkflowPlannerSessionWriteDenied));
                }
                None
            } else {
                let current_binding = workflow_planner_session_draft_binding(current)
                    .ok_or_else(|| anyhow::Error::new(WorkflowPlannerSessionWriteDenied))?;
                let current_plan_id = hosted_workflow_planner_session_plan_id(current)
                    .map_err(|_| anyhow::Error::new(WorkflowPlannerSessionWriteDenied))?;
                if current_plan_id.is_some_and(|plan_id| {
                    authority.get(plan_id)
                        != Some(&WorkflowPlanDraftAuthority::Bound {
                            binding: current_binding.clone(),
                            session_id: Some(current.session_id.clone()),
                        })
                }) {
                    return Err(anyhow::Error::new(WorkflowPlannerSessionWriteDenied));
                }
                Some(current_binding)
            }
        } else {
            None
        };
        let mut commit = || {
            if let (Some(plan_id), Some(binding)) = (plan_id.as_deref(), binding.as_ref()) {
                authority.insert(
                    plan_id.to_string(),
                    WorkflowPlanDraftAuthority::Bound {
                        binding: binding.clone(),
                        session_id: Some(session.session_id.clone()),
                    },
                );
            }
            if let Some(previous_plan_id) = previous_plan_to_remove.as_deref() {
                authority.remove(previous_plan_id);
            }
            sessions.insert(session.session_id.clone(), session.clone());
        };
        if let Some((tenant, verified)) = caller {
            if let Some(current_binding) = checked_binding.as_ref() {
                self.enterprise
                    .hosted_policy
                    .with_current_policy(|policy| {
                        crate::http::workflow_planner::with_current_planner_session_write_authority(
                            self,
                            current_binding,
                            tenant,
                            verified,
                            policy,
                            &mut commit,
                        )
                    })
                    .map_err(|_| anyhow::Error::new(WorkflowPlannerSessionWriteDenied))?
                    .map_err(|_| anyhow::Error::new(WorkflowPlannerSessionWriteDenied))?;
            } else {
                commit();
            }
        } else {
            commit();
        }
        drop(commit);
        if let Some(previous_plan_id) = previous_plan_to_remove.as_deref() {
            self.workflow_plan_drafts
                .write()
                .await
                .remove(previous_plan_id);
            self.workflow_plans.write().await.remove(previous_plan_id);
        }
        drop(sessions);
        self.sync_workflow_planner_session_cache(&session).await;
        drop(authority);
        self.persist_workflow_planner_sessions().await?;
        Ok(session)
    }

    pub async fn get_workflow_planner_session(
        &self,
        session_id: &str,
    ) -> Option<crate::http::workflow_planner::WorkflowPlannerSessionRecord> {
        self.workflow_planner_sessions
            .read()
            .await
            .get(session_id)
            .cloned()
    }

    pub async fn list_workflow_planner_sessions(
        &self,
        project_slug: Option<&str>,
    ) -> Vec<crate::http::workflow_planner::WorkflowPlannerSessionRecord> {
        let mut rows = self
            .workflow_planner_sessions
            .read()
            .await
            .values()
            .filter(|session| {
                project_slug
                    .map(|slug| session.project_slug == slug)
                    .unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows
    }

    pub async fn delete_workflow_planner_session(
        &self,
        session_id: &str,
    ) -> Option<crate::http::workflow_planner::WorkflowPlannerSessionRecord> {
        let mut authority = self.workflow_plan_draft_authority.write().await;
        let removed = self
            .workflow_planner_sessions
            .write()
            .await
            .remove(session_id);
        if let Some(session) = removed.as_ref() {
            if let Some(draft) = session.draft.as_ref() {
                let plan_id = draft.current_plan.plan_id.as_str();
                let owns_cached_plan = if session.tenant_context.is_local_implicit() {
                    !authority.contains_key(plan_id)
                } else {
                    matches!(
                        authority.get(plan_id),
                        Some(WorkflowPlanDraftAuthority::Bound { session_id: Some(owner), .. })
                            if owner == session_id
                    )
                };
                if owns_cached_plan {
                    authority.remove(plan_id);
                    self.workflow_plan_drafts.write().await.remove(plan_id);
                    self.workflow_plans.write().await.remove(plan_id);
                }
            }
        }
        drop(authority);
        let _ = self.persist_workflow_planner_sessions().await;
        removed
    }
}
