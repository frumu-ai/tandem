// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[cfg(test)]
mod tests {
    use crate::http::hosted_route_authority::required_permission;
    use axum::{body::Body, extract::Request, http::StatusCode, routing::any, Router};
    use tandem_types::AccessPermission;
    use tower::ServiceExt;

    async fn check(method: &str, pattern: &str, expected: Option<AccessPermission>) {
        let router = Router::new().route(
            pattern,
            any(move |request: Request| async move {
                assert_eq!(
                    required_permission(&request),
                    expected,
                    "operation grant for {} {}",
                    request.method(),
                    request.uri()
                );
                StatusCode::NO_CONTENT
            }),
        );
        let path = pattern
            .replace("{id}", "session-a")
            .replace("{run_id}", "run-a")
            .replace("{session_id}", "session-a")
            .replace("{orchestration_id}", "orchestration-a")
            .replace("{version}", "1")
            .replace("{tool_call_id}", "tool-a")
            .replace("{question_id}", "question-a");
        let response = router
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn hosted_planner_sessions_require_operation_grants() {
        for path in [
            "/workflow-plans/sessions",
            "/workflow-plans/sessions/{session_id}",
        ] {
            for method in ["GET", "HEAD"] {
                check(method, path, Some(AccessPermission::HostedAutomationRead)).await;
            }
        }
        check(
            "POST",
            "/workflow-plans/sessions",
            Some(AccessPermission::HostedAutomationWrite),
        )
        .await;
        for method in ["PATCH", "DELETE"] {
            check(
                method,
                "/workflow-plans/sessions/{session_id}",
                Some(AccessPermission::HostedAutomationWrite),
            )
            .await;
        }
        for suffix in [
            "duplicate",
            "start",
            "start-async",
            "message",
            "message-async",
            "reset",
        ] {
            check(
                "POST",
                &format!("/workflow-plans/sessions/{{session_id}}/{suffix}"),
                Some(AccessPermission::HostedAutomationWrite),
            )
            .await;
        }
    }

    #[tokio::test]
    async fn hosted_session_mutations_require_use_on_both_aliases() {
        for prefix in ["/session", "/api/session"] {
            for (method, suffix) in [
                ("POST", ""),
                ("PATCH", "/{id}"),
                ("DELETE", "/{id}"),
                ("POST", "/{id}/attach"),
                ("POST", "/{id}/workspace/override"),
                ("POST", "/{id}/message"),
                ("POST", "/{id}/prompt_async"),
                ("POST", "/{id}/prompt_sync"),
                ("POST", "/{id}/cancel"),
                ("POST", "/{id}/run/{run_id}/cancel"),
            ] {
                check(
                    method,
                    &format!("{prefix}{suffix}"),
                    Some(AccessPermission::HostedUse),
                )
                .await;
            }
        }
        for suffix in ["abort", "fork", "revert", "unrevert", "share", "summarize"] {
            check(
                "POST",
                &format!("/session/{{id}}/{suffix}"),
                Some(AccessPermission::HostedUse),
            )
            .await;
        }
        check(
            "DELETE",
            "/session/{id}/share",
            Some(AccessPermission::HostedUse),
        )
        .await;
    }

    #[tokio::test]
    async fn hosted_session_reads_and_separate_governance_keep_existing_grants() {
        for prefix in ["/session", "/api/session"] {
            for suffix in ["", "/{id}", "/{id}/message", "/{id}/todo", "/{id}/run"] {
                for method in ["GET", "HEAD"] {
                    check(method, &format!("{prefix}{suffix}"), None).await;
                }
            }
        }
        for path in [
            "/session/status",
            "/session/{id}/diff",
            "/session/{id}/children",
        ] {
            check("GET", path, None).await;
            check("HEAD", path, None).await;
        }
        // No-op, forbidden shell, and separately host-authorized read-only git
        // presets do not gain an additional HostedUse requirement.
        for suffix in ["init", "shell", "command"] {
            check("POST", &format!("/session/{{id}}/{suffix}"), None).await;
        }
        // These retain their independent hosted-admin/reviewer authorization.
        for path in [
            "/permission/{id}/reply",
            "/question/{id}/reply",
            "/question/{id}/reject",
            "/sessions/{session_id}/tools/{tool_call_id}/approve",
            "/sessions/{session_id}/tools/{tool_call_id}/deny",
            "/sessions/{session_id}/questions/{question_id}/answer",
        ] {
            check("POST", path, None).await;
        }
    }

    #[tokio::test]
    async fn hosted_orchestration_routes_classify_reads_and_authoring_separately() {
        for path in [
            "/orchestrations",
            "/orchestrations/{orchestration_id}",
            "/orchestrations/{orchestration_id}/versions",
            "/orchestrations/{orchestration_id}/versions/{version}",
            "/orchestrations/{orchestration_id}/stale-references",
        ] {
            check("GET", path, Some(AccessPermission::HostedAutomationRead)).await;
            check("HEAD", path, Some(AccessPermission::HostedAutomationRead)).await;
        }
        for path in [
            "/orchestrations/{orchestration_id}/validate",
            "/orchestrations/{orchestration_id}/dry-run",
        ] {
            check("POST", path, Some(AccessPermission::HostedAutomationRead)).await;
        }
        for (method, path) in [
            ("POST", "/orchestrations"),
            ("PUT", "/orchestrations/{orchestration_id}"),
            ("POST", "/orchestrations/{orchestration_id}/archive"),
            ("POST", "/orchestrations/{orchestration_id}/publish"),
            (
                "POST",
                "/orchestrations/{orchestration_id}/refresh-references",
            ),
        ] {
            check(method, path, Some(AccessPermission::HostedAutomationWrite)).await;
        }
    }
}
