fn default_issue_fix_head_branch(record: &CoderRunRecord) -> String {
    record
        .branch_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            format!(
                "coder/issue-{}-fix",
                record
                    .github_ref
                    .as_ref()
                    .map(|row| row.number)
                    .unwrap_or_default()
            )
        })
}

fn issue_fix_handoff_workspace_root(record: &CoderRunRecord) -> String {
    record
        .managed_worktree
        .as_ref()
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| record.repo_binding.workspace_root.clone())
}

fn git_output_text(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout,
        (true, false) => stderr,
        (false, false) => format!("{stdout}\n{stderr}"),
    }
}

fn run_git_command(workspace_root: &str, args: &[&str]) -> Result<std::process::Output, String> {
    std::process::Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run git {}: {error}", args.join(" ")))
}

async fn ensure_issue_fix_handoff_branch_pushed(
    record: &CoderRunRecord,
    head_branch: &str,
) -> Result<Value, String> {
    let workspace_root = issue_fix_handoff_workspace_root(record);
    let head_branch = head_branch.to_string();
    let github_ref = record.github_ref.clone();
    let coder_run_id = record.coder_run_id.clone();
    tokio::task::spawn_blocking(move || {
        let inside = run_git_command(&workspace_root, &["rev-parse", "--is-inside-work-tree"])?;
        if !inside.status.success() {
            return Err(format!(
                "handoff workspace is not a git worktree: {}",
                crate::truncate_text(&git_output_text(&inside), 500)
            ));
        }
        let current_branch_output =
            run_git_command(&workspace_root, &["branch", "--show-current"])?;
        if !current_branch_output.status.success() {
            return Err(format!(
                "failed to read handoff branch: {}",
                crate::truncate_text(&git_output_text(&current_branch_output), 500)
            ));
        }
        let current_branch = String::from_utf8_lossy(&current_branch_output.stdout)
            .trim()
            .to_string();
        if current_branch != head_branch {
            return Err(format!(
                "managed worktree is on branch `{current_branch}` but PR head is `{head_branch}`"
            ));
        }

        let status_before_output = run_git_command(&workspace_root, &["status", "--porcelain"])?;
        if !status_before_output.status.success() {
            return Err(format!(
                "failed to inspect handoff diff: {}",
                crate::truncate_text(&git_output_text(&status_before_output), 500)
            ));
        }
        let status_before = String::from_utf8_lossy(&status_before_output.stdout).to_string();
        let committed = if status_before.trim().is_empty() {
            false
        } else {
            let add = run_git_command(&workspace_root, &["add", "-A"])?;
            if !add.status.success() {
                return Err(format!(
                    "failed to stage worker diff: {}",
                    crate::truncate_text(&git_output_text(&add), 500)
                ));
            }
            let subject = github_ref
                .as_ref()
                .filter(|reference| matches!(reference.kind, CoderGithubRefKind::Issue))
                .map(|reference| format!("Fix issue #{} via Tandem Coder", reference.number))
                .unwrap_or_else(|| format!("Apply Tandem Coder handoff for {coder_run_id}"));
            let commit = run_git_command(
                &workspace_root,
                &[
                    "-c",
                    "user.name=Tandem Coder",
                    "-c",
                    "user.email=coder@tandem.local",
                    "commit",
                    "-m",
                    &subject,
                ],
            )?;
            if !commit.status.success() {
                return Err(format!(
                    "failed to commit worker diff: {}",
                    crate::truncate_text(&git_output_text(&commit), 500)
                ));
            }
            true
        };

        let commit_sha_output = run_git_command(&workspace_root, &["rev-parse", "HEAD"])?;
        if !commit_sha_output.status.success() {
            return Err(format!(
                "failed to read handoff commit: {}",
                crate::truncate_text(&git_output_text(&commit_sha_output), 500)
            ));
        }
        let commit_sha = String::from_utf8_lossy(&commit_sha_output.stdout)
            .trim()
            .to_string();

        let push = run_git_command(&workspace_root, &["push", "-u", "origin", &head_branch])?;
        if !push.status.success() {
            return Err(format!(
                "failed to push handoff branch `{head_branch}`: {}",
                crate::truncate_text(&git_output_text(&push), 500)
            ));
        }
        let status_after_output = run_git_command(&workspace_root, &["status", "--porcelain"])?;
        let status_after = if status_after_output.status.success() {
            String::from_utf8_lossy(&status_after_output.stdout).to_string()
        } else {
            String::new()
        };
        Ok(json!({
            "ok": true,
            "workspace_root": workspace_root,
            "branch_name": head_branch,
            "commit_sha": commit_sha,
            "committed": committed,
            "pushed": true,
            "status_before": status_before,
            "status_after": status_after,
            "push_output": crate::truncate_text(&git_output_text(&push), 2_000),
        }))
    })
    .await
    .map_err(|error| format!("handoff git task failed: {error}"))?
}

async fn block_issue_fix_pr_handoff(
    state: &AppState,
    record: &mut CoderRunRecord,
    submission_payload: &mut Value,
    code: &str,
    reason: &str,
    handoff_status: &str,
) -> Result<Json<Value>, StatusCode> {
    if let Some(obj) = submission_payload.as_object_mut() {
        obj.insert("submitted".to_string(), json!(false));
        obj.insert("follow_on_runs".to_string(), json!([]));
        obj.insert("spawned_follow_on_runs".to_string(), json!([]));
        obj.insert("external_action".to_string(), Value::Null);
        obj.insert("duplicate_linkage_candidate".to_string(), Value::Null);
    }
    let artifact = write_coder_artifact(
        state,
        &record.linked_context_run_id,
        &format!("issue-fix-pr-submit-{}", Uuid::new_v4().simple()),
        "coder_pr_submission",
        "artifacts/issue_fix.pr_submission.json",
        submission_payload,
    )
    .await?;
    publish_coder_artifact_added(state, record, &artifact, Some("approval"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("pr_submission"));
        extra.insert("submitted".to_string(), json!(false));
        extra.insert("blocked".to_string(), json!(true));
        extra.insert("code".to_string(), json!(code));
        extra
    });
    let gate = json!({
        "status": "blocked",
        "reason": reason,
        "message": reason,
        "artifact_path": artifact.path,
    });
    record.handoff_status = Some(handoff_status.to_string());
    record.completion_gate = Some(gate.clone());
    record.updated_at_ms = crate::now_ms();
    save_coder_run_record(state, record).await?;
    let transitioned = coder_run_transition(
        state,
        record,
        "run_blocked",
        ContextRunStatus::Blocked,
        Some(reason.to_string()),
    )
    .await?;
    Ok(Json(json!({
        "ok": false,
        "code": code,
        "error": reason,
        "artifact": artifact,
        "completion_gate": gate,
        "coder_run": transitioned.get("coder_run").cloned().unwrap_or(Value::Null),
        "run": transitioned.get("run").cloned().unwrap_or(Value::Null),
    })))
}

#[cfg(test)]
mod issue_fix_handoff_tests {
    use super::*;

    fn test_git(workspace_root: &str, args: &[&str]) {
        let output = run_git_command(workspace_root, args).expect("run git command");
        assert!(output.status.success(), "{}", git_output_text(&output));
    }

    fn test_issue_fix_record(
        workspace_root: String,
        branch_name: Option<String>,
    ) -> CoderRunRecord {
        CoderRunRecord {
            coder_run_id: "coder-issue-fix-handoff-git".to_string(),
            workflow_mode: CoderWorkflowMode::IssueFix,
            linked_context_run_id: "context-run-handoff-git".to_string(),
            repo_binding: CoderRepoBinding {
                project_id: "proj-engine".to_string(),
                workspace_id: "ws-tandem".to_string(),
                workspace_root: workspace_root.clone(),
                repo_slug: "user123/tandem".to_string(),
                default_branch: Some("main".to_string()),
            },
            github_ref: Some(CoderGithubRef {
                kind: CoderGithubRefKind::Issue,
                number: 313,
                url: None,
            }),
            source_client: None,
            model_provider: None,
            model_id: None,
            parent_coder_run_id: None,
            origin: None,
            origin_artifact_type: None,
            origin_policy: None,
            github_project_ref: None,
            remote_sync_state: None,
            worker_session_id: None,
            worker_run_id: None,
            managed_worktree: Some(json!({
                "path": workspace_root,
                "branch": "tandem/issue-313-fix",
            })),
            branch_name,
            commit_sha: None,
            pr_url: None,
            changed_files: None,
            validation_status: Some("passed".to_string()),
            handoff_status: Some("patch_ready".to_string()),
            completion_gate: None,
            created_at_ms: crate::now_ms(),
            updated_at_ms: crate::now_ms(),
        }
    }

    #[test]
    fn default_issue_fix_head_branch_prefers_recorded_worker_branch() {
        let record = test_issue_fix_record(
            "/tmp/tandem-coder-test".to_string(),
            Some("tandem/issue-313-fix".to_string()),
        );
        assert_eq!(
            default_issue_fix_head_branch(&record),
            "tandem/issue-313-fix"
        );
    }

    #[tokio::test]
    async fn issue_fix_handoff_commits_and_pushes_worker_branch() {
        let workspace_root =
            std::env::temp_dir().join(format!("tandem-coder-handoff-worktree-{}", Uuid::new_v4()));
        let remote_root =
            std::env::temp_dir().join(format!("tandem-coder-handoff-remote-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&workspace_root).expect("create worktree dir");
        std::fs::create_dir_all(&remote_root).expect("create remote dir");

        let workspace = workspace_root.to_string_lossy().to_string();
        let remote = remote_root.to_string_lossy().to_string();
        let init = std::process::Command::new("git")
            .args(["init"])
            .current_dir(&workspace_root)
            .output()
            .expect("git init");
        assert!(init.status.success(), "{}", git_output_text(&init));
        let init_remote = std::process::Command::new("git")
            .args(["init", "--bare"])
            .current_dir(&remote_root)
            .output()
            .expect("git init bare");
        assert!(
            init_remote.status.success(),
            "{}",
            git_output_text(&init_remote)
        );
        std::fs::write(workspace_root.join("README.md"), "initial\n").expect("write readme");
        test_git(&workspace, &["add", "README.md"]);
        test_git(
            &workspace,
            &[
                "-c",
                "user.name=Tandem Test",
                "-c",
                "user.email=test@tandem.local",
                "commit",
                "-m",
                "init",
            ],
        );
        test_git(&workspace, &["remote", "add", "origin", &remote]);
        test_git(&workspace, &["checkout", "-b", "tandem/issue-313-fix"]);
        std::fs::write(workspace_root.join("README.md"), "initial\nfixed\n").expect("write fix");

        let record =
            test_issue_fix_record(workspace.clone(), Some("tandem/issue-313-fix".to_string()));
        let handoff = ensure_issue_fix_handoff_branch_pushed(&record, "tandem/issue-313-fix")
            .await
            .expect("handoff git push");
        assert_eq!(
            handoff.get("committed").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(handoff.get("pushed").and_then(Value::as_bool), Some(true));
        assert_eq!(
            handoff.get("branch_name").and_then(Value::as_str),
            Some("tandem/issue-313-fix")
        );
        assert!(handoff
            .get("commit_sha")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty()));
        let status = run_git_command(&workspace, &["status", "--porcelain"]).expect("git status");
        assert!(status.status.success(), "{}", git_output_text(&status));
        assert!(
            String::from_utf8_lossy(&status.stdout).trim().is_empty(),
            "{}",
            String::from_utf8_lossy(&status.stdout)
        );
        let remote_ref = std::process::Command::new("git")
            .args([
                "--git-dir",
                &remote,
                "rev-parse",
                "refs/heads/tandem/issue-313-fix",
            ])
            .output()
            .expect("read remote ref");
        assert!(
            remote_ref.status.success(),
            "{}",
            git_output_text(&remote_ref)
        );

        let _ = std::fs::remove_dir_all(&workspace_root);
        let _ = std::fs::remove_dir_all(&remote_root);
    }
}
async fn call_merge_pull_request(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    owner: &str,
    repo: &str,
    pull_number: u64,
) -> Result<tandem_types::ToolResult, StatusCode> {
    let preferred = json!({
        "owner": owner,
        "repo": repo,
        "pull_number": pull_number,
        "merge_method": "squash",
    });
    let fallback = json!({
        "owner": owner,
        "repo": repo,
        "number": pull_number,
    });
    let first = state.mcp.call_tool(server_name, tool_name, preferred).await;
    match first {
        Ok(result) => Ok(result),
        Err(_) => state
            .mcp
            .call_tool(server_name, tool_name, fallback)
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY),
    }
}
