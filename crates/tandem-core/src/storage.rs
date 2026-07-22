use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::task;
use uuid::Uuid;

use tandem_types::{Message, MessagePart, MessageRole, Session, TenantContext};

use crate::{
    derive_session_title_from_prompt, normalize_workspace_path, title_needs_repair,
    workspace_project_id,
};

#[path = "session_repository.rs"]
mod session_repository;

include!("storage_parts/part01.rs");
include!("storage_parts/part02.rs");

#[cfg(test)]
mod question_scope_tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn question_requests_are_tenant_and_session_scoped_with_one_atomic_winner() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = Arc::new(Storage::new(dir.path()).await.expect("storage"));
        let tenant_a = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
            "alice",
        );
        let tenant_b = TenantContext::explicit_user_workspace(
            "org-b",
            "workspace-b",
            Some("deployment-b".to_string()),
            "bob",
        );
        let mut session_a = Session::new(Some("A".to_string()), None);
        session_a.tenant_context = tenant_a.clone();
        let mut session_b = Session::new(Some("B".to_string()), None);
        session_b.tenant_context = tenant_b.clone();
        storage
            .save_session(session_a.clone())
            .await
            .expect("save tenant A session");
        storage
            .save_session(session_b.clone())
            .await
            .expect("save tenant B session");

        let request_a = storage
            .add_question_request(
                &session_a.id,
                "message-a",
                vec![json!({"question": "Approve A?"})],
            )
            .await
            .expect("tenant A question");
        let request_b = storage
            .add_question_request(
                &session_b.id,
                "message-b",
                vec![json!({"question": "Approve B?"})],
            )
            .await
            .expect("tenant B question");

        assert_eq!(
            storage
                .list_question_requests_for_tenant(&tenant_a)
                .await
                .len(),
            1
        );
        assert_eq!(
            storage
                .list_question_requests_for_tenant(&tenant_b)
                .await
                .len(),
            1
        );
        assert!(storage
            .get_question_request_for_tenant(&request_a.id, &tenant_b, None)
            .await
            .expect("cross-tenant lookup")
            .is_none());
        assert!(storage
            .get_question_request_for_tenant(&request_a.id, &tenant_a, Some(&session_b.id),)
            .await
            .expect("wrong-session lookup")
            .is_none());

        let first_storage = storage.clone();
        let first_tenant = tenant_a.clone();
        let first_id = request_a.id.clone();
        let first_session = session_a.id.clone();
        let first = tokio::spawn(async move {
            first_storage
                .decide_question_for_tenant(&first_id, &first_tenant, Some(&first_session))
                .await
                .expect("first decision")
        });
        let second_storage = storage.clone();
        let second_tenant = tenant_a.clone();
        let second_id = request_a.id.clone();
        let second_session = session_a.id.clone();
        let second = tokio::spawn(async move {
            second_storage
                .decide_question_for_tenant(&second_id, &second_tenant, Some(&second_session))
                .await
                .expect("second decision")
        });
        let winners = usize::from(first.await.expect("first task").is_some())
            + usize::from(second.await.expect("second task").is_some());
        assert_eq!(winners, 1);
        assert!(storage
            .list_question_requests_for_tenant(&tenant_a)
            .await
            .is_empty());
        assert_eq!(
            storage.list_question_requests_for_tenant(&tenant_b).await[0].id,
            request_b.id
        );
    }

    #[tokio::test]
    async fn hosted_question_requests_quarantine_expired_tampered_and_unbound_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = Storage::new(dir.path()).await.expect("storage");
        let tenant = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
            "alice",
        );
        let mut session = Session::new(Some("A".to_string()), None);
        session.tenant_context = tenant.clone();
        storage
            .save_session(session.clone())
            .await
            .expect("save session");
        let valid = storage
            .add_question_request(
                &session.id,
                "message-a",
                vec![json!({"question": "Approve?"})],
            )
            .await
            .expect("question");

        let mut tampered = valid.clone();
        tampered.id = "tampered-question".to_string();
        tampered.action_digest = "tampered".to_string();
        storage
            .repository
            .add_question(&tampered)
            .expect("insert tampered row");
        let tampered_error = storage
            .get_question_request_for_tenant(&tampered.id, &tenant, None)
            .await
            .expect_err("tampered row rejected");
        assert!(tampered_error.to_string().contains("ACTION_MISMATCH"));

        let mut expired = valid.clone();
        expired.id = "expired-question".to_string();
        expired.expires_at_ms = now_ms_u64().saturating_sub(1);
        storage
            .repository
            .add_question(&expired)
            .expect("insert expired row");
        let expired_error = storage
            .get_question_request_for_tenant(&expired.id, &tenant, None)
            .await
            .expect_err("expired row rejected");
        assert!(expired_error.to_string().contains("EXPIRED"));

        let mut unbound = valid;
        unbound.id = "unbound-question".to_string();
        unbound.action_digest.clear();
        storage
            .repository
            .add_question(&unbound)
            .expect("insert unbound row");
        let unbound_error = storage
            .get_question_request_for_tenant(&unbound.id, &tenant, None)
            .await
            .expect_err("unbound row rejected");
        assert!(unbound_error.to_string().contains("UNBOUND"));
    }
}
