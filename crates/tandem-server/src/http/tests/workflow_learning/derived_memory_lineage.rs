// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Native HTTP regressions for canonical sources and derived memory scope.
//! The provider is deliberately local and records every extraction prompt.

use super::*;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use tandem_types::{Message, MessagePart, MessageRole, Session};
use tokio::sync::watch;

include!("derived_memory_lineage_fixtures.rs");

async fn hosted_promotion_request_with_owned_crypto(
    app: axum::Router,
    uri: String,
    body: Value,
) -> (StatusCode, Value) {
    // The promotion owns a spawned commit task. Keep the fixture's hosted
    // provider available to that task as it appends the protected audit.
    crate::app::state::tests::encrypted_file_stores::with_hosted_candidate_crypto(async move {
        crate::encrypted_file_store::spawn_protected_blocking(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("hosted promotion fixture runtime")
                .block_on(super::hosted_learning_request(
                    app,
                    "POST",
                    &uri,
                    Some(body),
                ))
        })
        .await
        .expect("hosted promotion fixture executor")
    })
    .await
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
    assert_eq!(
        status,
        StatusCode::OK,
        "canonical private distill: {payload}"
    );
    let id = one_memory_id(&payload);
    let prompts = fixture.prompts();
    assert_eq!(prompts.len(), 1, "one actual private extraction");
    assert!(prompts[0].contains(PRIVATE_MARKER));
    assert!(!prompts[0].contains(SHARED_MARKER));

    // record_for opens a fresh SQLite handle after the writer has completed.
    let own = fixture
        .record_for(&id, "alice")
        .await
        .expect("Alice cold recall");
    assert_eq!(own.content, PRIVATE_FACT);
    assert_eq!(own.metadata.as_ref().unwrap()["owner_subject"], "alice");
    assert_eq!(own.metadata.as_ref().unwrap()["owner_org_unit_id"], "eng");
    lineage_mentions(&own, &message_id);
    assert!(
        fixture.record_for(&id, "bob").await.is_none(),
        "same-department peer"
    );
    assert!(
        fixture.record_for(&id, "cara").await.is_none(),
        "other department"
    );
    assert!(
        fixture.record_in_foreign_tenant(&id).await.is_none(),
        "foreign tenant"
    );

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

    let own = fixture
        .record_for(&id, "alice")
        .await
        .expect("Alice shared recall");
    assert_eq!(own.content, SHARED_FACT);
    assert_eq!(own.metadata.as_ref().unwrap()["owner_org_unit_id"], "eng");
    assert!(own
        .metadata
        .as_ref()
        .unwrap()
        .get("owner_subject")
        .is_none());
    lineage_mentions(&own, &source_id);
    assert!(
        fixture.record_for(&id, "bob").await.is_some(),
        "real same-unit positive"
    );
    assert!(
        fixture.record_for(&id, "cara").await.is_none(),
        "other department"
    );
    assert!(
        fixture.record_in_foreign_tenant(&id).await.is_none(),
        "foreign tenant"
    );
}

#[tokio::test]
async fn tan_829_financial_source_grant_preserves_class_and_cross_department_recall() {
    let fixture = LineageFixture::new(false).await;
    let source_id = "tan-829-financial-canonical-source";
    let binding_id = "tan-829-financial-binding";
    fixture
        .seed_financial_source_record(source_id, binding_id, long_text(SHARED_MARKER))
        .await;
    let session = fixture.session("alice", Vec::new()).await;
    let body = distill_body(&session, &[], &[source_id.to_owned()]);
    let (status, _) = fixture.distill("alice", body.clone()).await;
    assert!(
        status.is_client_error(),
        "an Internal-only member cannot select FinancialRecord"
    );
    assert!(
        fixture.prompts().is_empty(),
        "ungranted financial source reached provider"
    );

    let (status, payload) = super::hosted_learning_request(
        fixture.router_with_financial_grant("alice", binding_id),
        "POST",
        "/memory/context/distill",
        Some(body),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "actual granted financial extraction: {payload}"
    );
    let id = one_memory_id(&payload);
    let owner = fixture
        .record_for(&id, "alice")
        .await
        .expect("cold financial derived row");
    lineage_mentions(&owner, source_id);
    assert_eq!(
        owner.metadata.as_ref().unwrap()["classification"],
        "financial_record",
        "the persisted class must remain the canonical FinancialRecord class"
    );
    let lineage = tandem_memory::derived_lineage::DerivedMemoryLineage::from_metadata(
        owner.metadata.as_ref(),
    )
    .expect("trusted stored lineage")
    .expect("derived lineage present");
    assert_eq!(
        lineage.output_data_class(),
        tandem_types::DataClass::FinancialRecord
    );

    let (status, ungranted) = super::hosted_learning_request(
        fixture.router("bob"),
        "GET",
        "/memory/list?project_id=tan-829-derived-memory",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !listed(&ungranted, &id),
        "same-department Bob needs the source grant"
    );
    for actor in ["bob", "cara"] {
        let (status, granted) = super::hosted_learning_request(
            fixture.router_with_financial_grant(actor, binding_id),
            "GET",
            "/memory/list?project_id=tan-829-derived-memory",
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "current {actor} FinancialRecord read: {granted}"
        );
        assert!(
            listed(&granted, &id),
            "{actor} has a real FinancialRecord source grant"
        );
        let item = granted["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == id)
            .expect("granted derived item");
        assert_eq!(item["classification"], "financial_record");
    }
    assert_eq!(
        fixture.prompts().len(),
        1,
        "one authorized provider extraction"
    );
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
    assert_eq!(
        payload["stored_count"], 2,
        "two distinct trusted dispositions"
    );
    let ids = payload["memory_ids"].as_array().expect("memory IDs");
    assert_eq!(ids.len(), 2);
    let prompts = fixture.prompts();
    assert_eq!(prompts.len(), 2, "one extraction per trusted disposition");
    assert!(prompts.iter().any(|prompt| prompt.contains(PRIVATE_MARKER)));
    assert!(prompts.iter().any(|prompt| prompt.contains(SHARED_MARKER)));
    assert!(
        prompts
            .iter()
            .all(|prompt| !(prompt.contains(PRIVATE_MARKER) && prompt.contains(SHARED_MARKER))),
        "no provider prompt may combine private and shared source text"
    );
    let mut private_seen = false;
    let mut shared_seen = false;
    for id in ids.iter().map(|id| id.as_str().expect("string ID")) {
        let own = fixture
            .record_for(id, "alice")
            .await
            .expect("Alice derived row");
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
    changed_text["conversation"] =
        json!([canonical.replace("review ownership", "public ownership")]);
    cases.push((changed_text, StatusCode::BAD_REQUEST));

    let mut forged_id = distill_body(&session, &["not-a-stored-message".to_owned()], &[]);
    forged_id["conversation"] = json!([canonical.clone()]);
    cases.push((forged_id, StatusCode::NOT_FOUND));

    for (field, value) in [
        ("private", json!(false)),
        (
            "source_lineage",
            json!({"owner_subject": null, "tenant_shared": true}),
        ),
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
    assert!(
        fixture.prompts().is_empty(),
        "invalid requests reached the provider"
    );
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
        .distill(
            "alice",
            json!({"session_id": session.id, "project_id": PROJECT}),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "canonical default selection: {payload}"
    );
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
        .distill("bob", distill_body(&bob_session, &[], &[source_id.clone()]))
        .await;
    assert!(status.is_client_error(), "same-department non-owner source");
    let cara_session = fixture.session("cara", Vec::new()).await;
    let (status, _) = fixture
        .distill("cara", distill_body(&cara_session, &[], &[source_id]))
        .await;
    assert!(
        status.is_client_error(),
        "foreign department and subject source"
    );
    assert!(
        fixture.prompts().is_empty(),
        "foreign sources reached provider"
    );
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
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "assistant without completed native lineage"
    );

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
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "modified durable assistant text"
    );
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
        .distill(
            "alice",
            distill_body(&session, &[assistant_id.clone()], &[]),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "complete native lineage: {payload}");
    let id = one_memory_id(&payload);
    let own = fixture
        .record_for(&id, "alice")
        .await
        .expect("private derived row");
    lineage_mentions(&own, &assistant_id);
    assert_eq!(own.metadata.as_ref().unwrap()["owner_subject"], "alice");
    assert!(fixture.record_for(&id, "bob").await.is_none());
    assert_eq!(
        fixture.prompts().len(),
        1,
        "completed assistant extracted once"
    );
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
    assert_ne!(
        private_id, shared_id,
        "private source reused broader shared row"
    );
    let private_row = fixture.record_for(&private_id, "alice").await.unwrap();
    lineage_mentions(&private_row, &message_id);
    assert!(fixture.record_for(&private_id, "bob").await.is_none());
    assert_eq!(
        fixture
            .record_for(&shared_id, "bob")
            .await
            .unwrap()
            .metadata,
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
    assert_eq!(
        status,
        StatusCode::OK,
        "private canonical distill: {payload}"
    );
    let id = one_memory_id(&payload);
    let candidate_id = payload["candidate_ids"][0]
        .as_str()
        .expect("derived candidate ID");
    let (status, reviewed) = super::hosted_learning_request(
        fixture.router("alice"),
        "POST",
        &format!("/workflow-learning/candidates/{candidate_id}/review"),
        Some(json!({"action": "approve"})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "Alice approved current private candidate: {reviewed}"
    );
    assert_eq!(reviewed["candidate"]["status"], "approved");
    assert_eq!(
        reviewed["candidate"]["workflow_id"],
        format!("session:{}", session.id)
    );
    assert_eq!(reviewed["candidate"]["project_id"], PROJECT);
    let (status, promoted) = hosted_promotion_request_with_owned_crypto(
        fixture.router_with_session_memory_read("alice", &session),
        format!("/workflow-learning/candidates/{candidate_id}/promote"),
        json!({
            "run_id": "tan-829-private-promotion",
            "reviewer_id": "alice",
            "approval_id": "tan-829-current-alice-review",
            "reason": "approved private canonical learning"
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "promote private derived candidate: {promoted}"
    );
    assert_eq!(promoted["promotion"]["promoted"], true);
    assert_eq!(promoted["candidate"]["promoted_memory_id"], id);

    // The HTTP projections and the fresh SQLite handle must agree after
    // promotion changes tier/visibility. Review cannot declassify a source.
    let owner = fixture
        .record_for(&id, "alice")
        .await
        .expect("cold promoted owner row");
    assert_eq!(owner.metadata.as_ref().unwrap()["owner_subject"], "alice");
    let policy = tandem_memory::KnowledgeScopePolicy::from_metadata(owner.metadata.as_ref())
        .expect("valid backfilled knowledge scope")
        .expect("promotion retains its grant-governed knowledge scope");
    assert_eq!(
        policy.resource_ref.resource_kind,
        tandem_types::ResourceKind::SourceBinding
    );
    assert_eq!(
        policy.resource_ref.resource_id,
        format!("workflow:session:{}", session.id)
    );
    assert_eq!(policy.resource_ref.project_id.as_deref(), Some(PROJECT));
    lineage_mentions(&owner, &message.id);
    assert!(fixture.record_for(&id, "bob").await.is_none());
    assert!(fixture.record_for(&id, "cara").await.is_none());
    assert!(
        listed(
            &fixture
                .memory_list_with_session_memory_read("alice", &session)
                .await,
            &id
        ),
        "Alice HTTP recall"
    );
    assert!(
        !listed(
            &fixture
                .memory_list_with_session_memory_read("bob", &session)
                .await,
            &id
        ),
        "Bob HTTP denial"
    );
    assert!(
        !listed(
            &fixture
                .memory_list_with_session_memory_read("cara", &session)
                .await,
            &id
        ),
        "other unit HTTP denial"
    );
}

#[tokio::test]
async fn tan_829_deleted_shared_source_hides_derived_recall_and_candidate() {
    let fixture = LineageFixture::new(false).await;
    let source_id = "tan-829-deleted-shared-source".to_owned();
    fixture
        .seed_source_record(&source_id, long_text(SHARED_MARKER), None)
        .await;
    let session = fixture.session("alice", Vec::new()).await;
    let (status, payload) = fixture
        .distill("alice", distill_body(&session, &[], &[source_id.clone()]))
        .await;
    assert_eq!(status, StatusCode::OK, "shared source distill: {payload}");
    let id = one_memory_id(&payload);
    let candidate_id = payload["candidate_ids"][0]
        .as_str()
        .expect("derived candidate ID");
    assert!(
        listed(&fixture.memory_list("bob").await, &id),
        "current shared source positive"
    );
    assert!(candidate_listed(
        &fixture.candidate_list("alice").await,
        candidate_id
    ));

    let db = tandem_memory::db::MemoryDatabase::new(&fixture.state.memory_db_path)
        .await
        .expect("cold canonical source db");
    assert!(db
        .delete_global_memory_for_tenant_scoped(
            &source_id,
            "org-learning",
            "dep-learning",
            Some("dep-learning"),
            Some("eng"),
            Some("alice"),
        )
        .await
        .expect("delete current canonical source"));
    assert!(
        fixture.record_for(&id, "alice").await.is_some(),
        "retained row is audit evidence"
    );
    assert!(
        !listed(&fixture.memory_list("alice").await, &id),
        "owner cannot use stale derivation"
    );
    assert!(
        !listed(&fixture.memory_list("bob").await, &id),
        "peer cannot use stale derivation"
    );
    assert!(!candidate_listed(
        &fixture.candidate_list("alice").await,
        candidate_id
    ));
    let (status, _) = fixture
        .distill("alice", distill_body(&session, &[], &[source_id]))
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "deleted canonical source cannot be reselected"
    );
    assert_eq!(
        fixture.prompts().len(),
        1,
        "stale retry reached the provider"
    );
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
    let candidate_id = payload["candidate_ids"][0]
        .as_str()
        .expect("derived candidate ID");
    assert!(listed(&fixture.memory_list("alice").await, &id));
    assert!(candidate_listed(
        &fixture.candidate_list("alice").await,
        candidate_id
    ));

    session.messages[0].parts = vec![MessagePart::Text {
        text: long_text("TAN829_CORRECTED_CANONICAL_SOURCE"),
    }];
    fixture
        .state
        .storage
        .save_session(session)
        .await
        .expect("canonical correction");
    assert!(
        fixture.record_for(&id, "alice").await.is_some(),
        "retained row is audit evidence"
    );
    assert!(
        !listed(&fixture.memory_list("alice").await, &id),
        "corrected source invalidates recall"
    );
    assert!(!candidate_listed(
        &fixture.candidate_list("alice").await,
        candidate_id
    ));
    assert_eq!(
        fixture.prompts().len(),
        1,
        "correction is a read-time check"
    );
}

#[tokio::test]
async fn tan_829_workflow_injection_uses_current_execution_identity_and_source_scope() {
    let fixture = LineageFixture::new(false).await;
    let workspace_root = fixture.state.workspace_index.snapshot().await.root;
    let automation = fixture
        .state
        .put_automation_v2(super::hosted_learning_automation(
            &workspace_root,
            "tan-829-owned-workflow",
            "alice",
        ))
        .await
        .expect("real Alice workflow source");
    let private_message = text_message(MessageRole::User, long_text(PRIVATE_MARKER));
    let alice_session = fixture
        .session("alice", vec![private_message.clone()])
        .await;
    let source_id = "tan-829-injection-shared-source".to_owned();
    fixture
        .seed_source_record(&source_id, long_text(SHARED_MARKER), None)
        .await;

    let mut private_request = distill_body(&alice_session, &[private_message.id], &[]);
    private_request["workflow_id"] = json!(automation.automation_id);
    let (status, private) = fixture.distill("alice", private_request).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "private workflow learning: {private}"
    );
    let mut shared_request = distill_body(&alice_session, &[], &[source_id.clone()]);
    shared_request["workflow_id"] = json!(automation.automation_id);
    let (status, shared) = fixture.distill("alice", shared_request).await;
    assert_eq!(status, StatusCode::OK, "shared workflow learning: {shared}");
    let private_candidate = private["candidate_ids"][0]
        .as_str()
        .expect("private candidate");
    let shared_candidate = shared["candidate_ids"][0]
        .as_str()
        .expect("shared candidate");
    for candidate_id in [private_candidate, shared_candidate] {
        let (status, reviewed) = super::hosted_learning_request(
            fixture.router("alice"),
            "POST",
            &format!("/workflow-learning/candidates/{candidate_id}/review"),
            Some(json!({"action": "approve"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "current Alice review: {reviewed}");
        assert_eq!(reviewed["candidate"]["status"], "approved");
    }

    let bob_session = fixture.session("bob", Vec::new()).await;
    let cara_session = fixture.session("cara", Vec::new()).await;
    let node = &automation.flow.nodes[0];
    let (alice_ids, alice_context) = fixture
        .state
        .workflow_learning_context_for_automation_node_session(
            &automation,
            node,
            Some(&alice_session.id),
        )
        .await;
    assert!(alice_ids.contains(&private_candidate.to_owned()));
    assert!(alice_ids.contains(&shared_candidate.to_owned()));
    let alice_context = alice_context.expect("Alice current learning context");
    assert!(alice_context.contains(PRIVATE_FACT));
    assert!(alice_context.contains(SHARED_FACT));

    let (bob_ids, bob_context) = fixture
        .state
        .workflow_learning_context_for_automation_node_session(
            &automation,
            node,
            Some(&bob_session.id),
        )
        .await;
    assert!(
        !bob_ids.contains(&private_candidate.to_owned()),
        "Bob must not receive Alice private fact"
    );
    assert!(
        bob_ids.contains(&shared_candidate.to_owned()),
        "same-department Bob receives shared fact"
    );
    let bob_context = bob_context.expect("Bob current shared context");
    assert!(!bob_context.contains(PRIVATE_FACT));
    assert!(bob_context.contains(SHARED_FACT));

    let (cara_ids, cara_context) = fixture
        .state
        .workflow_learning_context_for_automation_node_session(
            &automation,
            node,
            Some(&cara_session.id),
        )
        .await;
    assert!(
        cara_ids.is_empty(),
        "other department receives no derived candidates"
    );
    assert!(cara_context.is_none());

    let db = tandem_memory::db::MemoryDatabase::new(&fixture.state.memory_db_path)
        .await
        .expect("cold source store");
    assert!(db
        .delete_global_memory_for_tenant_scoped(
            &source_id,
            "org-learning",
            "dep-learning",
            Some("dep-learning"),
            Some("eng"),
            Some("alice"),
        )
        .await
        .expect("revoke shared canonical input"));
    let (bob_after_ids, bob_after_context) = fixture
        .state
        .workflow_learning_context_for_automation_node_session(
            &automation,
            node,
            Some(&bob_session.id),
        )
        .await;
    assert!(
        bob_after_ids.is_empty(),
        "revoked shared source leaves Bob no usable candidate"
    );
    assert!(bob_after_context.is_none());
    let (alice_after_ids, alice_after_context) = fixture
        .state
        .workflow_learning_context_for_automation_node_session(
            &automation,
            node,
            Some(&alice_session.id),
        )
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
    let fixture = LineageFixture::new_with_gate(
        false,
        Some(ProviderGate {
            captured: captured_tx,
            released: release_rx,
        }),
    )
    .await;
    let message = text_message(MessageRole::User, long_text(PRIVATE_MARKER));
    let session = fixture.session("alice", vec![message.clone()]).await;
    let request = distill_body(&session, &[message.id], &[]);
    let original_verified = fixture.verified("alice");
    let app = fixture.router("alice");
    let request_task = tokio::spawn(async move {
        super::hosted_learning_request(app, "POST", "/memory/context/distill", Some(request)).await
    });
    captured_rx
        .changed()
        .await
        .expect("actual provider prompt captured");
    assert!(
        *captured_rx.borrow(),
        "provider reached the held completion path"
    );
    let prompts = fixture.prompts();
    assert_eq!(prompts.len(), 1, "one actual extraction prompt");
    assert!(prompts[0].contains(PRIVATE_MARKER));

    let policy_path = fixture._policy.path().join("policy.json");
    let mut policy: Value =
        serde_json::from_slice(&std::fs::read(&policy_path).expect("v1 policy"))
            .expect("canonical fixture policy JSON");
    policy["policy_version"] = json!(2);
    policy["generated_at"] = json!(
        chrono::DateTime::from_timestamp_millis(crate::now_ms() as i64)
            .expect("current policy timestamp")
    );
    let alice = policy["users"]
        .as_array_mut()
        .expect("policy members")
        .iter_mut()
        .find(|user| user["id"] == "alice")
        .expect("Alice policy member");
    alice["is_active"] = json!(false);
    std::fs::write(&policy_path, serde_json::to_vec(&policy).unwrap())
        .expect("publish v2 revocation");
    fixture
        .state
        .reload_hosted_policy()
        .await
        .expect("reload v2 Alice revocation");
    assert!(
        fixture
            .state
            .enterprise
            .hosted_policy
            .authorize(Some(&original_verified))
            .is_err(),
        "published v2 policy must reject the original Alice assertion"
    );

    release_tx
        .send(true)
        .expect("held provider receiver remains active");
    let (status, payload) = request_task.await.expect("distillation request completes");
    assert!(
        !status.is_success(),
        "stale authority cannot report success: {payload}"
    );
    assert!(payload["memory_ids"].as_array().is_none_or(Vec::is_empty));
    assert!(payload["candidate_ids"]
        .as_array()
        .is_none_or(Vec::is_empty));
    assert_eq!(payload["stored_count"].as_u64().unwrap_or(0), 0);
    assert_eq!(payload["deduped_count"].as_u64().unwrap_or(0), 0);
    assert!(
        fixture
            .state
            .list_workflow_learning_candidates(None, None, None)
            .await
            .is_empty(),
        "revoked extraction cannot leave an approved or proposed candidate"
    );
    let cold = tandem_memory::db::MemoryDatabase::new(&fixture.state.memory_db_path)
        .await
        .expect("fresh memory db");
    let rows = cold
        .list_global_memory_for_tenant_scoped(
            "org-learning",
            "dep-learning",
            Some("dep-learning"),
            Some("alice"),
            "alice",
            None,
            Some(PROJECT),
            None,
            100,
            0,
            Some("eng"),
        )
        .await
        .expect("fresh canonical scoped listing");
    assert!(
        rows.is_empty(),
        "revoked completion persisted no canonical or deduped memory"
    );
    assert_eq!(
        fixture.prompts().len(),
        1,
        "provider really ran before authority changed"
    );
}
