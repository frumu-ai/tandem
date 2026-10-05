// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

const PROJECT: &str = "tan-829-derived-memory";
const PRIVATE_MARKER: &str = "TAN829_PRIVATE_CANONICAL_MARKER";
const SHARED_MARKER: &str = "TAN829_SHARED_CANONICAL_MARKER";
const PRIVATE_FACT: &str = "The private release checklist requires Alice's personal review.";
const SHARED_FACT: &str = "The engineering release checklist requires a shared team review.";
const SAME_FACT: &str = "The release checklist requires an explicit final review.";

fn long_text(marker: &str) -> String {
    format!(
        "{marker} The canonical release discussion covers build provenance, review ownership, \
         incident response, test outcomes, deployment timing, audit anchors, and recovery. \
         The team repeats these details to establish durable context for future sessions. \
         Every decision identifies the actual source and its audience before a fact is saved. \
         This text has enough distinct words for the real distillation token threshold."
    )
}

fn fact_response(content: &str) -> String {
    json!([{
        "category": "fact",
        "content": content,
        "importance": 0.95,
        "follow_up_needed": false
    }])
    .to_string()
}

#[derive(Clone)]
struct SourceAwareProvider {
    prompts: Arc<Mutex<Vec<String>>>,
    same_fact: bool,
    gate: Option<ProviderGate>,
}

#[derive(Clone)]
struct ProviderGate {
    captured: watch::Sender<bool>,
    released: watch::Receiver<bool>,
}

impl SourceAwareProvider {
    async fn response(&self, prompt: &str) -> anyhow::Result<String> {
        self.prompts
            .lock()
            .expect("prompt capture")
            .push(prompt.to_owned());
        if let Some(gate) = &self.gate {
            gate.captured
                .send(true)
                .expect("capture observer remains live");
            let mut released = gate.released.clone();
            while !*released.borrow() {
                released
                    .changed()
                    .await
                    .expect("release controller remains live");
            }
        }
        if self.same_fact {
            return Ok(fact_response(SAME_FACT));
        }
        let private = prompt.contains(PRIVATE_MARKER);
        let shared = prompt.contains(SHARED_MARKER);
        anyhow::ensure!(
            private ^ shared,
            "extraction must have one canonical disposition"
        );
        Ok(fact_response(if private {
            PRIVATE_FACT
        } else {
            SHARED_FACT
        }))
    }
}

#[async_trait::async_trait]
impl tandem_providers::Provider for SourceAwareProvider {
    fn info(&self) -> tandem_types::ProviderInfo {
        tandem_types::ProviderInfo {
            id: "tan-829-source-probe".to_owned(),
            name: "TAN-829 Source Probe".to_owned(),
            models: vec![tandem_types::ModelInfo {
                id: "tan-829-source-probe-1".to_owned(),
                provider_id: "tan-829-source-probe".to_owned(),
                display_name: "TAN-829 Source Probe 1".to_owned(),
                context_window: 8_192,
            }],
        }
    }

    async fn complete(&self, prompt: &str, _model: Option<&str>) -> anyhow::Result<String> {
        self.response(prompt).await
    }

    async fn complete_with_auth_override(
        &self,
        prompt: &str,
        _model: Option<&str>,
        _auth_override: tandem_providers::ProviderAuthOverride,
    ) -> anyhow::Result<String> {
        self.response(prompt).await
    }
}

struct LineageFixture {
    state: AppState,
    _policy: tempfile::TempDir,
    prompts: Arc<Mutex<Vec<String>>>,
}

impl LineageFixture {
    async fn new(same_fact: bool) -> Self {
        Self::new_with_gate(same_fact, None).await
    }

    async fn new_with_gate(same_fact: bool, gate: Option<ProviderGate>) -> Self {
        let state = test_state().await;
        let policy = tempfile::tempdir().expect("policy directory");
        let path = policy.path().join("policy.json");
        let now = crate::now_ms();
        let users = ["alice", "bob", "cara"]
            .into_iter()
            .map(|actor| {
                json!({
                    "id": actor,
                    "email": null,
                    "username": null,
                    "role": "member",
                    "capabilities": ["hosted.use", "automation.read", "automation.execute"],
                    "is_active": true,
                    "email_verified": true
                })
            })
            .collect::<Vec<_>>();
        let grants = ["alice", "bob", "cara"]
            .into_iter()
            .map(|actor| {
                json!({
                    "id": format!("write-{actor}"),
                    "deployment_id": "dep-learning",
                    "principal_kind": "member",
                    "principal_id": actor,
                    "resource_kind": "deployment",
                    "resource_id": "dep-learning",
                    "permissions": ["automation.write"]
                })
            })
            .collect::<Vec<_>>();
        let bundle = json!({
            "schema_version": 1,
            "policy_version": 1,
            "organization_id": "org-learning",
            "deployment_id": "dep-learning",
            "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
            "users": users,
            "org_units": [
                {"id":"eng", "slug":"eng", "display_name":"Engineering", "kind":"department", "state":"active"},
                {"id":"ops", "slug":"ops", "display_name":"Operations", "kind":"department", "state":"active"}
            ],
            "org_unit_memberships": [
                {"unit_id":"eng", "user_id":"alice"},
                {"unit_id":"eng", "user_id":"bob"},
                {"unit_id":"ops", "user_id":"cara"}
            ],
            "deployment_grants": grants
        });
        std::fs::write(&path, serde_json::to_vec(&bundle).unwrap()).expect("policy file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("private policy file");
        }
        state
            .enterprise
            .hosted_policy
            .configure_test_source("org-learning", "dep-learning", path);
        state.reload_hosted_policy().await.expect("hosted policy");
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = SourceAwareProvider {
            prompts: prompts.clone(),
            same_fact,
            gate,
        };
        state
            .providers
            .replace_for_test(
                vec![Arc::new(provider)],
                Some("tan-829-source-probe".to_owned()),
            )
            .await;
        Self {
            state,
            _policy: policy,
            prompts,
        }
    }

    fn unit(actor: &str) -> &'static str {
        match actor {
            "alice" | "bob" => "eng",
            "cara" => "ops",
            _ => panic!("unknown synthetic actor"),
        }
    }

    fn verified(&self, actor: &str) -> tandem_types::VerifiedTenantContext {
        use tandem_types::{AuthorityChain, HumanActor, TenantContextAssertionClaims};

        let now = crate::now_ms();
        let tenant = super::hosted_learning_tenant(actor);
        let mut claims = TenantContextAssertionClaims::new_v1(
            "tandem-web",
            "tandem-runtime",
            now,
            now + 300_000,
            format!("tan-829-{actor}"),
            tenant,
            HumanActor::tandem_user(actor),
            AuthorityChain::from_request(RequestPrincipal::authenticated_user(actor, "tandem-web")),
            vec!["hosted:role:member".into()],
        );
        claims.policy_version = Some(1);
        claims.capabilities = ["hosted.use", "automation.read", "automation.execute"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut verified = tandem_types::VerifiedTenantContext::from(claims);
        verified.org_units = vec![Self::unit(actor).to_owned()];
        self.state
            .enterprise
            .hosted_policy
            .project(&mut verified)
            .expect("project synthetic hosted identity")
            .expect("hosted policy installed");
        verified
    }

    fn router(&self, actor: &str) -> axum::Router {
        self.router_with_verified(actor, self.verified(actor))
    }

    fn router_with_verified(
        &self,
        actor: &str,
        verified: tandem_types::VerifiedTenantContext,
    ) -> axum::Router {
        let tenant = super::hosted_learning_tenant(actor);
        axum::Router::<AppState>::new()
            .route(
                "/memory/context/distill",
                axum::routing::post(skills_memory::context_distill),
            )
            .route(
                "/memory/list",
                axum::routing::get(skills_memory::memory_list),
            )
            .route(
                "/workflow-learning/candidates",
                axum::routing::get(skills_memory::workflow_learning_candidates_list),
            )
            .route(
                "/workflow-learning/candidates/{candidate_id}/review",
                axum::routing::post(skills_memory::workflow_learning_candidate_review),
            )
            .route(
                "/workflow-learning/candidates/{candidate_id}/promote",
                axum::routing::post(skills_memory::workflow_learning_candidate_promote),
            )
            .layer(axum::Extension(tenant))
            .layer(axum::Extension(verified))
            .with_state(self.state.clone())
    }

    /// The router receives server-projected identity. This test-only grant is
    /// attached after the hosted member projection, as an operator-managed
    /// source grant; it is never accepted from an HTTP request body.
    fn router_with_financial_grant(&self, actor: &str, binding_id: &str) -> axum::Router {
        use tandem_types::{
            AccessPermission, DataBoundary, DataClass, GrantSource, PrincipalRef, ResourceKind,
            ResourceRef, ScopedGrant,
        };

        let mut verified = self.verified(actor);
        let mut strict = verified
            .strict_projection
            .take()
            .expect("current hosted projection");
        strict.grants.push(
            ScopedGrant::new(
                format!("tan-829-financial-read-{actor}"),
                PrincipalRef::human_user(actor),
                ResourceRef::new(
                    "org-learning",
                    "dep-learning",
                    ResourceKind::DocumentCollection,
                    binding_id,
                ),
                GrantSource::Direct,
            )
            .with_permissions(vec![AccessPermission::Read])
            .with_data_classes(vec![DataClass::FinancialRecord]),
        );
        verified.strict_projection = Some(strict.with_data_boundary(DataBoundary::allow(vec![
            DataClass::Internal,
            DataClass::FinancialRecord,
        ])));
        self.router_with_verified(actor, verified)
    }

    async fn session(&self, actor: &str, messages: Vec<Message>) -> Session {
        let mut session = Session::new(Some("derived lineage".to_owned()), Some(".".to_owned()));
        session.project_id = Some(PROJECT.to_owned());
        session.tenant_context = super::hosted_learning_tenant(actor);
        session.verified_tenant_context = Some(self.verified(actor));
        session.messages = messages;
        self.state
            .storage
            .save_session(session.clone())
            .await
            .expect("canonical session");
        session
    }

    async fn distill(&self, actor: &str, body: Value) -> (StatusCode, Value) {
        super::hosted_learning_request(
            self.router(actor),
            "POST",
            "/memory/context/distill",
            Some(body),
        )
        .await
    }

    async fn memory_list(&self, actor: &str) -> Value {
        let (status, payload) = super::hosted_learning_request(
            self.router(actor),
            "GET",
            "/memory/list?project_id=tan-829-derived-memory",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "governed memory list: {payload}");
        payload
    }

    async fn candidate_list(&self, actor: &str) -> Value {
        let (status, payload) = super::hosted_learning_request(
            self.router(actor),
            "GET",
            "/workflow-learning/candidates",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "governed candidate list: {payload}");
        payload
    }

    async fn record_for(
        &self,
        id: &str,
        actor: &str,
    ) -> Option<tandem_memory::types::GlobalMemoryRecord> {
        let db = tandem_memory::db::MemoryDatabase::new(&self.state.memory_db_path)
            .await
            .expect("memory db");
        db.get_global_memory_for_tenant_scoped(
            id,
            "org-learning",
            "dep-learning",
            Some("dep-learning"),
            Some(Self::unit(actor)),
            Some(actor),
        )
        .await
        .expect("scoped canonical record")
    }

    async fn record_in_foreign_tenant(
        &self,
        id: &str,
    ) -> Option<tandem_memory::types::GlobalMemoryRecord> {
        let db = tandem_memory::db::MemoryDatabase::new(&self.state.memory_db_path)
            .await
            .expect("memory db");
        db.get_global_memory_for_tenant_scoped(
            id,
            "other-organization",
            "other-deployment",
            Some("other-deployment"),
            Some("eng"),
            Some("alice"),
        )
        .await
        .expect("foreign-tenant read")
    }

    async fn seed_source_record(&self, id: &str, content: String, owner_subject: Option<&str>) {
        let now = crate::now_ms();
        let mut metadata = json!({ "owner_org_unit_id": "eng" });
        if let Some(owner) = owner_subject {
            metadata["owner_subject"] = json!(owner);
        }
        let record = tandem_memory::types::GlobalMemoryRecord {
            id: id.to_owned(),
            user_id: "alice".to_owned(),
            source_type: "note".to_owned(),
            content_hash: format!("{:x}", Sha256::digest(content.as_bytes())),
            content,
            run_id: format!("source-run-{id}"),
            session_id: None,
            message_id: None,
            tool_name: None,
            project_tag: Some(PROJECT.to_owned()),
            channel_tag: None,
            host_tag: None,
            metadata: Some(metadata),
            provenance: Some(json!({
                "tenant_context": super::hosted_learning_tenant("alice")
            })),
            redaction_status: "passed".to_owned(),
            redaction_count: 0,
            visibility: if owner_subject.is_some() {
                "private".to_owned()
            } else {
                "shared".to_owned()
            },
            demoted: false,
            score_boost: 0.0,
            created_at_ms: now,
            updated_at_ms: now,
            expires_at_ms: None,
        };
        let db = tandem_memory::db::MemoryDatabase::new(&self.state.memory_db_path)
            .await
            .expect("memory db");
        assert!(
            db.put_global_memory_record(&record)
                .await
                .expect("canonical source record")
                .stored
        );
    }

    async fn seed_financial_source_record(&self, id: &str, binding_id: &str, content: String) {
        use tandem_types::{ResourceKind, ResourceRef};
        let now = crate::now_ms();
        let resource = ResourceRef::new(
            "org-learning",
            "dep-learning",
            ResourceKind::DocumentCollection,
            binding_id,
        );
        let record = tandem_memory::types::GlobalMemoryRecord {
            id: id.to_owned(),
            user_id: "alice".to_owned(),
            source_type: "note".to_owned(),
            content_hash: format!("{:x}", Sha256::digest(content.as_bytes())),
            content,
            run_id: format!("source-run-{id}"),
            session_id: None,
            message_id: None,
            tool_name: None,
            project_tag: Some(PROJECT.to_owned()),
            channel_tag: None,
            host_tag: None,
            metadata: Some(json!({
                "owner_org_unit_id": "eng",
                "enterprise_source_binding": {
                    "binding_id": binding_id,
                    "resource_ref": resource,
                    "data_class": "financial_record"
                }
            })),
            provenance: Some(json!({"tenant_context": super::hosted_learning_tenant("alice")})),
            redaction_status: "passed".to_owned(),
            redaction_count: 0,
            visibility: "shared".to_owned(),
            demoted: false,
            score_boost: 0.0,
            created_at_ms: now,
            updated_at_ms: now,
            expires_at_ms: None,
        };
        let db = tandem_memory::db::MemoryDatabase::new(&self.state.memory_db_path)
            .await
            .expect("financial source db");
        assert!(
            db.put_global_memory_record(&record)
                .await
                .expect("canonical financial source record")
                .stored
        );
    }

    fn prompts(&self) -> Vec<String> {
        self.prompts.lock().expect("prompt capture").clone()
    }
}

fn text_message(role: MessageRole, content: String) -> Message {
    Message::new(role, vec![MessagePart::Text { text: content }])
}

fn distill_body(session: &Session, message_ids: &[String], source_memory_ids: &[String]) -> Value {
    json!({
        "session_id": session.id,
        "project_id": PROJECT,
        "message_ids": message_ids,
        "source_memory_ids": source_memory_ids
    })
}

fn one_memory_id(payload: &Value) -> String {
    assert_eq!(
        payload["stored_count"], 1,
        "expected one new canonical fact: {payload}"
    );
    let ids = payload["memory_ids"].as_array().expect("memory IDs");
    assert_eq!(ids.len(), 1, "one stored memory ID: {payload}");
    ids[0].as_str().expect("string memory ID").to_owned()
}

fn lineage_mentions(record: &tandem_memory::types::GlobalMemoryRecord, source_id: &str) {
    let lineage = record
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("derived_memory_lineage"))
        .expect("persisted server-derived lineage");
    assert!(
        lineage.to_string().contains(source_id),
        "persisted lineage must name the canonical source"
    );
}

fn listed(payload: &Value, id: &str) -> bool {
    payload["items"]
        .as_array()
        .expect("governed memory items")
        .iter()
        .any(|item| item["id"] == id)
}

fn candidate_listed(payload: &Value, id: &str) -> bool {
    payload["candidates"]
        .as_array()
        .expect("governed candidates")
        .iter()
        .any(|item| item["candidate_id"] == id)
}
