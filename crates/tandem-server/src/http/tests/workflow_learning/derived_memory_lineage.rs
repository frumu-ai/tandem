// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Native HTTP regressions for canonical sources and derived memory scope.
//! The provider is deliberately local and records every extraction prompt.

use super::*;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use tandem_types::{Message, MessagePart, MessageRole, Session};
use tokio::sync::watch;

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
        self.prompts.lock().expect("prompt capture").push(prompt.to_owned());
        if let Some(gate) = &self.gate {
            gate.captured.send(true).expect("capture observer remains live");
            let mut released = gate.released.clone();
            while !*released.borrow() {
                released.changed().await.expect("release controller remains live");
            }
        }
        if self.same_fact {
            return Ok(fact_response(SAME_FACT));
        }
        let private = prompt.contains(PRIVATE_MARKER);
        let shared = prompt.contains(SHARED_MARKER);
        anyhow::ensure!(private ^ shared, "extraction must have one canonical disposition");
        Ok(fact_response(if private { PRIVATE_FACT } else { SHARED_FACT }))
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
            .replace_for_test(vec![Arc::new(provider)], Some("tan-829-source-probe".to_owned()))
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
        &self, actor: &str, verified: tandem_types::VerifiedTenantContext,
    ) -> axum::Router {
        let tenant = super::hosted_learning_tenant(actor);
        axum::Router::<AppState>::new()
            .route(
                "/memory/context/distill",
                axum::routing::post(skills_memory::context_distill),
            )
            .route("/memory/list", axum::routing::get(skills_memory::memory_list))
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
        use tandem_types::{AccessPermission, DataBoundary, DataClass, GrantSource, PrincipalRef,
            ResourceKind, ResourceRef, ScopedGrant};

        let mut verified = self.verified(actor);
        let mut strict = verified.strict_projection.take().expect("current hosted projection");
        strict.grants.push(ScopedGrant::new(
            format!("tan-829-financial-read-{actor}"), PrincipalRef::human_user(actor),
            ResourceRef::new("org-learning", "dep-learning", ResourceKind::DocumentCollection, binding_id),
            GrantSource::Direct,
        ).with_permissions(vec![AccessPermission::Read])
            .with_data_classes(vec![DataClass::FinancialRecord]));
        verified.strict_projection = Some(strict.with_data_boundary(DataBoundary::allow(vec![
            DataClass::Internal, DataClass::FinancialRecord,
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
        super::hosted_learning_request(self.router(actor), "POST", "/memory/context/distill", Some(body))
            .await
    }

    async fn memory_list(&self, actor: &str) -> Value {
        let (status, payload) = super::hosted_learning_request(
            self.router(actor), "GET", "/memory/list?project_id=tan-829-derived-memory", None,
        ).await;
        assert_eq!(status, StatusCode::OK, "governed memory list: {payload}");
        payload
    }

    async fn candidate_list(&self, actor: &str) -> Value {
        let (status, payload) = super::hosted_learning_request(
            self.router(actor), "GET", "/workflow-learning/candidates", None,
        ).await;
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

    async fn seed_source_record(
        &self,
        id: &str,
        content: String,
        owner_subject: Option<&str>,
    ) {
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
            "org-learning", "dep-learning", ResourceKind::DocumentCollection, binding_id,
        );
        let record = tandem_memory::types::GlobalMemoryRecord {
            id: id.to_owned(), user_id: "alice".to_owned(), source_type: "note".to_owned(),
            content_hash: format!("{:x}", Sha256::digest(content.as_bytes())), content,
            run_id: format!("source-run-{id}"), session_id: None, message_id: None,
            tool_name: None, project_tag: Some(PROJECT.to_owned()), channel_tag: None,
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
            redaction_status: "passed".to_owned(), redaction_count: 0,
            visibility: "shared".to_owned(), demoted: false, score_boost: 0.0,
            created_at_ms: now, updated_at_ms: now, expires_at_ms: None,
        };
        let db = tandem_memory::db::MemoryDatabase::new(&self.state.memory_db_path)
            .await.expect("financial source db");
        assert!(db.put_global_memory_record(&record).await
            .expect("canonical financial source record").stored);
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
    assert_eq!(payload["stored_count"], 1, "expected one new canonical fact: {payload}");
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
    payload["items"].as_array().expect("governed memory items")
        .iter().any(|item| item["id"] == id)
}

fn candidate_listed(payload: &Value, id: &str) -> bool {
    payload["candidates"].as_array().expect("governed candidates")
        .iter().any(|item| item["candidate_id"] == id)
}

#[tokio::test]
async fn tan_829_private_canonical_session_fact_survives_cold_reopen_without_peer_access() {
    let fixture = LineageFixture::new(false).await;
    let message = text_message(MessageRole::User, long_text(PRIVATE_MARKER));
    let message_id = message.id.clone();
    let session = fixture.session("alice", vec![message]).await;
    let (status, payload) = fixture
        .distill("alice", distill_body(&session, &[message_id.clone()], &[]))
        .await;
    assert_eq!(status, StatusCode::OK, "canonical private distill: {payload}");
    let id = one_memory_id(&payload);
    let prompts = fixture.prompts();
    assert_eq!(prompts.len(), 1, "one actual private extraction");
    assert!(prompts[0].contains(PRIVATE_MARKER));
    assert!(!prompts[0].contains(SHARED_MARKER));

    // record_for opens a fresh SQLite handle after the writer has completed.
    let own = fixture.record_for(&id, "alice").await.expect("Alice cold recall");
    assert_eq!(own.content, PRIVATE_FACT);
    assert_eq!(own.metadata.as_ref().unwrap()["owner_subject"], "alice");
    assert_eq!(own.metadata.as_ref().unwrap()["owner_org_unit_id"], "eng");
    lineage_mentions(&own, &message_id);
    assert!(fixture.record_for(&id, "bob").await.is_none(), "same-department peer");
    assert!(fixture.record_for(&id, "cara").await.is_none(), "other department");
    assert!(fixture.record_in_foreign_tenant(&id).await.is_none(), "foreign tenant");

    let candidate_id = payload["candidate_ids"][0].as_str().expect("candidate ID");
    let candidate = fixture
        .state
        .get_workflow_learning_candidate(candidate_id)
        .await
        .expect("persisted candidate");
    assert_eq!(candidate.source_memory_id.as_deref(), Some(id.as_str()));
}

#[tokio::test]
async fn tan_829_shared_only_canonical_source_remains_visible_to_same_department() {
    let fixture = LineageFixture::new(false).await;
    let source_id = "tan-829-shared-source".to_owned();
    fixture
        .seed_source_record(&source_id, long_text(SHARED_MARKER), None)
        .await;
    let session = fixture.session("alice", Vec::new()).await;
    let (status, payload) = fixture
        .distill("alice", distill_body(&session, &[], &[source_id.clone()]))
        .await;
    assert_eq!(status, StatusCode::OK, "shared-only distill: {payload}");
    let id = one_memory_id(&payload);
    let prompts = fixture.prompts();
    assert_eq!(prompts.len(), 1, "one actual shared-only extraction");
    assert!(prompts[0].contains(SHARED_MARKER));
    assert!(!prompts[0].contains(PRIVATE_MARKER));

    let own = fixture.record_for(&id, "alice").await.expect("Alice shared recall");
    assert_eq!(own.content, SHARED_FACT);
    assert_eq!(own.metadata.as_ref().unwrap()["owner_org_unit_id"], "eng");
    assert!(own.metadata.as_ref().unwrap().get("owner_subject").is_none());
    lineage_mentions(&own, &source_id);
    assert!(fixture.record_for(&id, "bob").await.is_some(), "real same-unit positive");
    assert!(fixture.record_for(&id, "cara").await.is_none(), "other department");
    assert!(fixture.record_in_foreign_tenant(&id).await.is_none(), "foreign tenant");
}

#[tokio::test]
async fn tan_829_financial_source_grant_preserves_class_and_cross_department_recall() {
    let fixture = LineageFixture::new(false).await;
    let source_id = "tan-829-financial-canonical-source";
    let binding_id = "tan-829-financial-binding";
    fixture.seed_financial_source_record(
        source_id, binding_id, long_text(SHARED_MARKER),
    ).await;
    let session = fixture.session("alice", Vec::new()).await;
    let body = distill_body(&session, &[], &[source_id.to_owned()]);
    let (status, _) = fixture.distill("alice", body.clone()).await;
    assert!(status.is_client_error(), "an Internal-only member cannot select FinancialRecord");
    assert!(fixture.prompts().is_empty(), "ungranted financial source reached provider");

    let (status, payload) = super::hosted_learning_request(
        fixture.router_with_financial_grant("alice", binding_id), "POST",
        "/memory/context/distill", Some(body),
    ).await;
    assert_eq!(status, StatusCode::OK, "actual granted financial extraction: {payload}");
    let id = one_memory_id(&payload);
    let owner = fixture.record_for(&id, "alice").await.expect("cold financial derived row");
    lineage_mentions(&owner, source_id);
    assert_eq!(owner.metadata.as_ref().unwrap()["classification"], "financial_record",
        "the persisted class must remain the canonical FinancialRecord class");
    let lineage = tandem_memory::derived_lineage::DerivedMemoryLineage::from_metadata(
        owner.metadata.as_ref(),
    ).expect("trusted stored lineage").expect("derived lineage present");
    assert_eq!(lineage.output_data_class(), tandem_types::DataClass::FinancialRecord);

    let (status, ungranted) = super::hosted_learning_request(
        fixture.router("bob"), "GET", "/memory/list?project_id=tan-829-derived-memory", None,
    ).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!listed(&ungranted, &id), "same-department Bob needs the source grant");
    for actor in ["bob", "cara"] {
        let (status, granted) = super::hosted_learning_request(
            fixture.router_with_financial_grant(actor, binding_id), "GET",
            "/memory/list?project_id=tan-829-derived-memory", None,
        ).await;
        assert_eq!(status, StatusCode::OK, "current {actor} FinancialRecord read: {granted}");
        assert!(listed(&granted, &id), "{actor} has a real FinancialRecord source grant");
        let item = granted["items"].as_array().unwrap().iter()
            .find(|item| item["id"] == id).expect("granted derived item");
        assert_eq!(item["classification"], "financial_record");
    }
    assert_eq!(fixture.prompts().len(), 1, "one authorized provider extraction");
}

#[tokio::test]
async fn tan_829_mixed_sources_use_separate_provider_prompts_and_separate_acls() {
    let fixture = LineageFixture::new(false).await;
    let message = text_message(MessageRole::User, long_text(PRIVATE_MARKER));
    let message_id = message.id.clone();
    let session = fixture.session("alice", vec![message]).await;
    let source_id = "tan-829-mixed-shared-source".to_owned();
    fixture
        .seed_source_record(&source_id, long_text(SHARED_MARKER), None)
        .await;
    let (status, payload) = fixture
        .distill(
            "alice",
            distill_body(&session, &[message_id.clone()], &[source_id.clone()]),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "mixed canonical distill: {payload}");
    assert_eq!(payload["stored_count"], 2, "two distinct trusted dispositions");
    let ids = payload["memory_ids"].as_array().expect("memory IDs");
    assert_eq!(ids.len(), 2);
    let prompts = fixture.prompts();
    assert_eq!(prompts.len(), 2, "one extraction per trusted disposition");
    assert!(prompts.iter().any(|prompt| prompt.contains(PRIVATE_MARKER)));
    assert!(prompts.iter().any(|prompt| prompt.contains(SHARED_MARKER)));
    assert!(
        prompts.iter().all(|prompt| !(prompt.contains(PRIVATE_MARKER) && prompt.contains(SHARED_MARKER))),
        "no provider prompt may combine private and shared source text"
    );
    let mut private_seen = false;
    let mut shared_seen = false;
    for id in ids.iter().map(|id| id.as_str().expect("string ID")) {
        let own = fixture.record_for(id, "alice").await.expect("Alice derived row");
        if own.content == PRIVATE_FACT {
            private_seen = true;
            lineage_mentions(&own, &message_id);
            assert!(fixture.record_for(id, "bob").await.is_none());
        } else {
            assert_eq!(own.content, SHARED_FACT);
            shared_seen = true;
            lineage_mentions(&own, &source_id);
            assert!(fixture.record_for(id, "bob").await.is_some());
        }
        assert!(fixture.record_for(id, "cara").await.is_none());
    }
    assert!(private_seen && shared_seen);
}

#[tokio::test]
async fn tan_829_client_lineage_and_modified_conversation_cannot_authorize_a_write() {
    let fixture = LineageFixture::new(false).await;
    let canonical = long_text(PRIVATE_MARKER);
    let message = text_message(MessageRole::User, canonical.clone());
    let message_id = message.id.clone();
    let session = fixture.session("alice", vec![message]).await;
    let mut cases = Vec::new();

    let mut changed_text = distill_body(&session, &[message_id.clone()], &[]);
    changed_text["conversation"] = json!([canonical.replace("review ownership", "public ownership")]);
    cases.push((changed_text, StatusCode::BAD_REQUEST));

    let mut forged_id = distill_body(&session, &["not-a-stored-message".to_owned()], &[]);
    forged_id["conversation"] = json!([canonical.clone()]);
    cases.push((forged_id, StatusCode::NOT_FOUND));

    for (field, value) in [
        ("private", json!(false)),
        ("source_lineage", json!({"owner_subject": null, "tenant_shared": true})),
        ("source_metadata", json!({"owner_org_unit_id": null})),
    ] {
        let mut injected = distill_body(&session, &[message_id.clone()], &[]);
        injected[field] = value;
        cases.push((injected, StatusCode::BAD_REQUEST));
    }

    for (body, expected) in cases {
        let (status, _) = fixture.distill("alice", body).await;
        assert_eq!(status, expected, "untrusted lineage must fail closed");
    }
    assert!(fixture.prompts().is_empty(), "invalid requests reached the provider");
    assert!(
        fixture
            .state
            .list_workflow_learning_candidates(None, None, None)
            .await
            .is_empty(),
        "invalid requests created review candidates"
    );
}

#[tokio::test]
async fn tan_829_omitted_selectors_still_use_only_owned_canonical_private_text() {
    let fixture = LineageFixture::new(false).await;
    let message = text_message(MessageRole::User, long_text(PRIVATE_MARKER));
    let message_id = message.id.clone();
    let session = fixture.session("alice", vec![message]).await;
    let (status, payload) = fixture
        .distill("alice", json!({"session_id": session.id, "project_id": PROJECT}))
        .await;
    assert_eq!(status, StatusCode::OK, "canonical default selection: {payload}");
    let id = one_memory_id(&payload);
    let row = fixture.record_for(&id, "alice").await.expect("owner read");
    lineage_mentions(&row, &message_id);
    assert_eq!(row.metadata.as_ref().unwrap()["owner_subject"], "alice");
    assert!(fixture.record_for(&id, "bob").await.is_none());
    assert_eq!(fixture.prompts().len(), 1);
}

#[tokio::test]
async fn tan_829_foreign_session_and_private_source_fail_before_extraction() {
    let fixture = LineageFixture::new(false).await;
    let bob_message = text_message(MessageRole::User, long_text(PRIVATE_MARKER));
    let bob_id = bob_message.id.clone();
    let bob_session = fixture.session("bob", vec![bob_message]).await;
    let (status, _) = fixture
        .distill("alice", distill_body(&bob_session, &[bob_id], &[]))
        .await;
    assert!(status.is_client_error(), "foreign session must be refused");

    let source_id = "tan-829-alice-private-source".to_owned();
    fixture
        .seed_source_record(&source_id, long_text(SHARED_MARKER), Some("alice"))
        .await;
    let (status, _) = fixture
        .distill(
            "bob",
            distill_body(&bob_session, &[], &[source_id.clone()]),
        )
        .await;
    assert!(status.is_client_error(), "same-department non-owner source");
    let cara_session = fixture.session("cara", Vec::new()).await;
    let (status, _) = fixture
        .distill("cara", distill_body(&cara_session, &[], &[source_id]))
        .await;
    assert!(status.is_client_error(), "foreign department and subject source");
    assert!(fixture.prompts().is_empty(), "foreign sources reached provider");
    assert!(
        fixture
            .state
            .list_workflow_learning_candidates(None, None, None)
            .await
            .is_empty(),
        "foreign source created a candidate"
    );
}

#[tokio::test]
async fn tan_829_incomplete_assistant_and_modified_durable_lineage_fail_closed() {
    let fixture = LineageFixture::new(false).await;
    let incomplete = text_message(MessageRole::Assistant, long_text(PRIVATE_MARKER));
    let incomplete_id = incomplete.id.clone();
    let session = fixture.session("alice", vec![incomplete]).await;
    let (status, _) = fixture
        .distill("alice", distill_body(&session, &[incomplete_id], &[]))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "assistant without completed native lineage");

    let mut altered = text_message(MessageRole::Assistant, long_text(PRIVATE_MARKER));
    let original_digest = tandem_types::canonical_message_digest(&altered);
    altered.source_lineage = Some(tandem_types::NativeMessageLineage {
        schema_version: 1,
        run_id: "tan-829-original-run".to_owned(),
        tenant_context: super::hosted_learning_tenant("alice"),
        subject: "alice".to_owned(),
        message_digest: original_digest,
        input_message_ids: Vec::new(),
        included_memory: Vec::new(),
        complete: true,
    });
    altered.parts = vec![MessagePart::Text {
        text: long_text("TAN829_CHANGED_AFTER_COMMIT"),
    }];
    let altered_id = altered.id.clone();
    let session = fixture.session("alice", vec![altered]).await;
    let (status, _) = fixture
        .distill("alice", distill_body(&session, &[altered_id], &[]))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "modified durable assistant text");
    assert!(fixture.prompts().is_empty());
}

#[tokio::test]
async fn tan_829_complete_canonical_assistant_inherits_private_input_scope() {
    let fixture = LineageFixture::new(false).await;
    let user = text_message(MessageRole::User, long_text("TAN829_ORIGINAL_USER_SOURCE"));
    let mut assistant = text_message(MessageRole::Assistant, long_text(PRIVATE_MARKER));
    let assistant_id = assistant.id.clone();
    assistant.source_lineage = Some(tandem_types::NativeMessageLineage {
        schema_version: 1,
        run_id: "tan-829-completed-assistant-run".to_owned(),
        tenant_context: super::hosted_learning_tenant("alice"),
        subject: "alice".to_owned(),
        message_digest: tandem_types::canonical_message_digest(&assistant),
        input_message_ids: vec![user.id.clone()],
        included_memory: Vec::new(),
        complete: true,
    });
    let session = fixture.session("alice", vec![user, assistant]).await;
    let (status, payload) = fixture
        .distill("alice", distill_body(&session, &[assistant_id.clone()], &[]))
        .await;
    assert_eq!(status, StatusCode::OK, "complete native lineage: {payload}");
    let id = one_memory_id(&payload);
    let own = fixture.record_for(&id, "alice").await.expect("private derived row");
    lineage_mentions(&own, &assistant_id);
    assert_eq!(own.metadata.as_ref().unwrap()["owner_subject"], "alice");
    assert!(fixture.record_for(&id, "bob").await.is_none());
    assert_eq!(fixture.prompts().len(), 1, "completed assistant extracted once");
}

#[tokio::test]
async fn tan_829_privacy_compatible_dedupe_never_reuses_a_broader_record() {
    let fixture = LineageFixture::new(true).await;
    let source_id = "tan-829-dedupe-shared-source".to_owned();
    fixture
        .seed_source_record(&source_id, long_text(SHARED_MARKER), None)
        .await;
    let message = text_message(MessageRole::User, long_text(PRIVATE_MARKER));
    let message_id = message.id.clone();
    let session = fixture.session("alice", vec![message]).await;

    let shared_body = distill_body(&session, &[], &[source_id.clone()]);
    let (status, shared) = fixture.distill("alice", shared_body).await;
    assert_eq!(status, StatusCode::OK, "shared first: {shared}");
    let shared_id = one_memory_id(&shared);
    let shared_before = fixture
        .record_for(&shared_id, "bob")
        .await
        .expect("shared record is genuinely peer-visible");
    lineage_mentions(&shared_before, &source_id);

    let private_body = distill_body(&session, &[message_id.clone()], &[]);
    let (status, private) = fixture.distill("alice", private_body.clone()).await;
    assert_eq!(status, StatusCode::OK, "private after shared: {private}");
    let private_id = one_memory_id(&private);
    assert_ne!(private_id, shared_id, "private source reused broader shared row");
    let private_row = fixture.record_for(&private_id, "alice").await.unwrap();
    lineage_mentions(&private_row, &message_id);
    assert!(fixture.record_for(&private_id, "bob").await.is_none());
    assert_eq!(
        fixture.record_for(&shared_id, "bob").await.unwrap().metadata,
        shared_before.metadata,
        "private distillation cannot narrow or rewrite prior shared row"
    );

    let (status, repeat) = fixture.distill("alice", private_body).await;
    assert_eq!(status, StatusCode::OK, "compatible repeat: {repeat}");
    assert_eq!(repeat["stored_count"], 0);
    assert_eq!(repeat["deduped_count"], 1);
    assert_eq!(repeat["memory_ids"], json!([private_id]));
}

#[tokio::test]
async fn tan_829_approved_private_promotion_preserves_owner_on_cold_http_recall() {
    let fixture = LineageFixture::new(false).await;
    let message = text_message(MessageRole::User, long_text(PRIVATE_MARKER));
    let session = fixture.session("alice", vec![message.clone()]).await;
    let (status, payload) = fixture
        .distill("alice", distill_body(&session, &[message.id.clone()], &[]))
        .await;
    assert_eq!(status, StatusCode::OK, "private canonical distill: {payload}");
    let id = one_memory_id(&payload);
    let candidate_id = payload["candidate_ids"][0].as_str().expect("derived candidate ID");
    let (status, reviewed) = super::hosted_learning_request(
        fixture.router("alice"), "POST",
        &format!("/workflow-learning/candidates/{candidate_id}/review"),
        Some(json!({"action": "approve"})),
    ).await;
    assert_eq!(status, StatusCode::OK, "Alice approved current private candidate: {reviewed}");
    assert_eq!(reviewed["candidate"]["status"], "approved");
    let (status, promoted) = super::hosted_learning_request(
        fixture.router("alice"), "POST",
        &format!("/workflow-learning/candidates/{candidate_id}/promote"),
        Some(json!({
            "run_id": "tan-829-private-promotion",
            "reviewer_id": "alice",
            "approval_id": "tan-829-current-alice-review",
            "reason": "approved private canonical learning"
        })),
    ).await;
    assert_eq!(status, StatusCode::OK, "promote private derived candidate: {promoted}");
    assert_eq!(promoted["promotion"]["promoted"], true);
    assert_eq!(promoted["candidate"]["promoted_memory_id"], id);

    // The HTTP projections and the fresh SQLite handle must agree after
    // promotion changes tier/visibility. Review cannot declassify a source.
    let owner = fixture.record_for(&id, "alice").await.expect("cold promoted owner row");
    assert_eq!(owner.metadata.as_ref().unwrap()["owner_subject"], "alice");
    lineage_mentions(&owner, &message.id);
    assert!(fixture.record_for(&id, "bob").await.is_none());
    assert!(fixture.record_for(&id, "cara").await.is_none());
    assert!(listed(&fixture.memory_list("alice").await, &id), "Alice HTTP recall");
    assert!(!listed(&fixture.memory_list("bob").await, &id), "Bob HTTP denial");
    assert!(!listed(&fixture.memory_list("cara").await, &id), "other unit HTTP denial");
}

#[tokio::test]
async fn tan_829_deleted_shared_source_hides_derived_recall_and_candidate() {
    let fixture = LineageFixture::new(false).await;
    let source_id = "tan-829-deleted-shared-source".to_owned();
    fixture.seed_source_record(&source_id, long_text(SHARED_MARKER), None).await;
    let session = fixture.session("alice", Vec::new()).await;
    let (status, payload) = fixture
        .distill("alice", distill_body(&session, &[], &[source_id.clone()]))
        .await;
    assert_eq!(status, StatusCode::OK, "shared source distill: {payload}");
    let id = one_memory_id(&payload);
    let candidate_id = payload["candidate_ids"][0].as_str().expect("derived candidate ID");
    assert!(listed(&fixture.memory_list("bob").await, &id), "current shared source positive");
    assert!(candidate_listed(&fixture.candidate_list("alice").await, candidate_id));

    let db = tandem_memory::db::MemoryDatabase::new(&fixture.state.memory_db_path)
        .await.expect("cold canonical source db");
    assert!(db.delete_global_memory_for_tenant_scoped(
        &source_id, "org-learning", "dep-learning", Some("dep-learning"),
        Some("eng"), Some("alice"),
    ).await.expect("delete current canonical source"));
    assert!(fixture.record_for(&id, "alice").await.is_some(), "retained row is audit evidence");
    assert!(!listed(&fixture.memory_list("alice").await, &id), "owner cannot use stale derivation");
    assert!(!listed(&fixture.memory_list("bob").await, &id), "peer cannot use stale derivation");
    assert!(!candidate_listed(&fixture.candidate_list("alice").await, candidate_id));
    let (status, _) = fixture.distill("alice", distill_body(&session, &[], &[source_id])).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "deleted canonical source cannot be reselected");
    assert_eq!(fixture.prompts().len(), 1, "stale retry reached the provider");
}

#[tokio::test]
async fn tan_829_corrected_private_message_hides_retained_derived_output() {
    let fixture = LineageFixture::new(false).await;
    let message = text_message(MessageRole::User, long_text(PRIVATE_MARKER));
    let mut session = fixture.session("alice", vec![message.clone()]).await;
    let (status, payload) = fixture
        .distill("alice", distill_body(&session, &[message.id.clone()], &[]))
        .await;
    assert_eq!(status, StatusCode::OK, "private source distill: {payload}");
    let id = one_memory_id(&payload);
    let candidate_id = payload["candidate_ids"][0].as_str().expect("derived candidate ID");
    assert!(listed(&fixture.memory_list("alice").await, &id));
    assert!(candidate_listed(&fixture.candidate_list("alice").await, candidate_id));

    session.messages[0].parts = vec![MessagePart::Text {
        text: long_text("TAN829_CORRECTED_CANONICAL_SOURCE"),
    }];
    fixture.state.storage.save_session(session).await.expect("canonical correction");
    assert!(fixture.record_for(&id, "alice").await.is_some(), "retained row is audit evidence");
    assert!(!listed(&fixture.memory_list("alice").await, &id), "corrected source invalidates recall");
    assert!(!candidate_listed(&fixture.candidate_list("alice").await, candidate_id));
    assert_eq!(fixture.prompts().len(), 1, "correction is a read-time check");
}

#[tokio::test]
async fn tan_829_workflow_injection_uses_current_execution_identity_and_source_scope() {
    let fixture = LineageFixture::new(false).await;
    let workspace_root = fixture.state.workspace_index.snapshot().await.root;
    let automation = fixture.state.put_automation_v2(super::hosted_learning_automation(
        &workspace_root, "tan-829-owned-workflow", "alice",
    )).await.expect("real Alice workflow source");
    let private_message = text_message(MessageRole::User, long_text(PRIVATE_MARKER));
    let alice_session = fixture.session("alice", vec![private_message.clone()]).await;
    let source_id = "tan-829-injection-shared-source".to_owned();
    fixture.seed_source_record(&source_id, long_text(SHARED_MARKER), None).await;

    let mut private_request = distill_body(&alice_session, &[private_message.id], &[]);
    private_request["workflow_id"] = json!(automation.automation_id);
    let (status, private) = fixture.distill("alice", private_request).await;
    assert_eq!(status, StatusCode::OK, "private workflow learning: {private}");
    let mut shared_request = distill_body(&alice_session, &[], &[source_id.clone()]);
    shared_request["workflow_id"] = json!(automation.automation_id);
    let (status, shared) = fixture.distill("alice", shared_request).await;
    assert_eq!(status, StatusCode::OK, "shared workflow learning: {shared}");
    let private_candidate = private["candidate_ids"][0].as_str().expect("private candidate");
    let shared_candidate = shared["candidate_ids"][0].as_str().expect("shared candidate");
    for candidate_id in [private_candidate, shared_candidate] {
        let (status, reviewed) = super::hosted_learning_request(
            fixture.router("alice"), "POST",
            &format!("/workflow-learning/candidates/{candidate_id}/review"),
            Some(json!({"action": "approve"})),
        ).await;
        assert_eq!(status, StatusCode::OK, "current Alice review: {reviewed}");
        assert_eq!(reviewed["candidate"]["status"], "approved");
    }

    let bob_session = fixture.session("bob", Vec::new()).await;
    let cara_session = fixture.session("cara", Vec::new()).await;
    let node = &automation.flow.nodes[0];
    let (alice_ids, alice_context) = fixture.state
        .workflow_learning_context_for_automation_node_session(&automation, node, Some(&alice_session.id))
        .await;
    assert!(alice_ids.contains(&private_candidate.to_owned()));
    assert!(alice_ids.contains(&shared_candidate.to_owned()));
    let alice_context = alice_context.expect("Alice current learning context");
    assert!(alice_context.contains(PRIVATE_FACT));
    assert!(alice_context.contains(SHARED_FACT));

    let (bob_ids, bob_context) = fixture.state
        .workflow_learning_context_for_automation_node_session(&automation, node, Some(&bob_session.id))
        .await;
    assert!(!bob_ids.contains(&private_candidate.to_owned()), "Bob must not receive Alice private fact");
    assert!(bob_ids.contains(&shared_candidate.to_owned()), "same-department Bob receives shared fact");
    let bob_context = bob_context.expect("Bob current shared context");
    assert!(!bob_context.contains(PRIVATE_FACT));
    assert!(bob_context.contains(SHARED_FACT));

    let (cara_ids, cara_context) = fixture.state
        .workflow_learning_context_for_automation_node_session(&automation, node, Some(&cara_session.id))
        .await;
    assert!(cara_ids.is_empty(), "other department receives no derived candidates");
    assert!(cara_context.is_none());

    let db = tandem_memory::db::MemoryDatabase::new(&fixture.state.memory_db_path)
        .await.expect("cold source store");
    assert!(db.delete_global_memory_for_tenant_scoped(
        &source_id, "org-learning", "dep-learning", Some("dep-learning"),
        Some("eng"), Some("alice"),
    ).await.expect("revoke shared canonical input"));
    let (bob_after_ids, bob_after_context) = fixture.state
        .workflow_learning_context_for_automation_node_session(&automation, node, Some(&bob_session.id))
        .await;
    assert!(bob_after_ids.is_empty(), "revoked shared source leaves Bob no usable candidate");
    assert!(bob_after_context.is_none());
    let (alice_after_ids, alice_after_context) = fixture.state
        .workflow_learning_context_for_automation_node_session(&automation, node, Some(&alice_session.id))
        .await;
    assert!(alice_after_ids.contains(&private_candidate.to_owned()));
    assert!(!alice_after_ids.contains(&shared_candidate.to_owned()));
    let alice_after_context = alice_after_context.expect("current private context remains");
    assert!(alice_after_context.contains(PRIVATE_FACT));
    assert!(!alice_after_context.contains(SHARED_FACT));
}

#[tokio::test]
async fn tan_829_revocation_during_real_provider_extraction_persists_nothing() {
    let (captured_tx, mut captured_rx) = watch::channel(false);
    let (release_tx, release_rx) = watch::channel(false);
    let fixture = LineageFixture::new_with_gate(false, Some(ProviderGate {
        captured: captured_tx,
        released: release_rx,
    })).await;
    let message = text_message(MessageRole::User, long_text(PRIVATE_MARKER));
    let session = fixture.session("alice", vec![message.clone()]).await;
    let request = distill_body(&session, &[message.id], &[]);
    let original_verified = fixture.verified("alice");
    let app = fixture.router("alice");
    let request_task = tokio::spawn(async move {
        super::hosted_learning_request(app, "POST", "/memory/context/distill", Some(request)).await
    });
    captured_rx.changed().await.expect("actual provider prompt captured");
    assert!(*captured_rx.borrow(), "provider reached the held completion path");
    let prompts = fixture.prompts();
    assert_eq!(prompts.len(), 1, "one actual extraction prompt");
    assert!(prompts[0].contains(PRIVATE_MARKER));

    let policy_path = fixture._policy.path().join("policy.json");
    let mut policy: Value = serde_json::from_slice(&std::fs::read(&policy_path).expect("v1 policy"))
        .expect("canonical fixture policy JSON");
    policy["policy_version"] = json!(2);
    policy["generated_at"] = json!(chrono::DateTime::from_timestamp_millis(crate::now_ms() as i64)
        .expect("current policy timestamp"));
    let alice = policy["users"].as_array_mut().expect("policy members")
        .iter_mut().find(|user| user["id"] == "alice").expect("Alice policy member");
    alice["is_active"] = json!(false);
    std::fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).expect("publish v2 revocation");
    fixture.state.reload_hosted_policy().await.expect("reload v2 Alice revocation");
    assert!(fixture.state.enterprise.hosted_policy.authorize(Some(&original_verified)).is_err(),
        "published v2 policy must reject the original Alice assertion");

    release_tx.send(true).expect("held provider receiver remains active");
    let (status, payload) = request_task.await.expect("distillation request completes");
    assert!(!status.is_success(), "stale authority cannot report success: {payload}");
    assert!(payload["memory_ids"].as_array().is_none_or(Vec::is_empty));
    assert!(payload["candidate_ids"].as_array().is_none_or(Vec::is_empty));
    assert_eq!(payload["stored_count"].as_u64().unwrap_or(0), 0);
    assert_eq!(payload["deduped_count"].as_u64().unwrap_or(0), 0);
    assert!(fixture.state.list_workflow_learning_candidates(None, None, None).await.is_empty(),
        "revoked extraction cannot leave an approved or proposed candidate");
    let cold = tandem_memory::db::MemoryDatabase::new(&fixture.state.memory_db_path)
        .await.expect("fresh memory db");
    let rows = cold.list_global_memory_for_tenant_scoped(
        "org-learning", "dep-learning", Some("dep-learning"), Some("alice"), "alice",
        None, Some(PROJECT), None, 100, 0, Some("eng"),
    ).await.expect("fresh canonical scoped listing");
    assert!(rows.is_empty(), "revoked completion persisted no canonical or deduped memory");
    assert_eq!(fixture.prompts().len(), 1, "provider really ran before authority changed");
}
