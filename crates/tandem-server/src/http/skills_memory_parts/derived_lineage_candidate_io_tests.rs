// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use std::path::{Path, PathBuf};
use tandem_automation::WorkflowLearningCandidateSourceBinding;

const BASE_MARKER: &str = "CANDIDATE_IO_PRIVATE_BASE";
const DENIED_MARKER: &str = "CANDIDATE_IO_PRIVATE_DENIED";
const RECOVERY_MARKER: &str = "CANDIDATE_IO_PRIVATE_RECOVERY";

fn private_candidate(
    original: &VerifiedTenantContext,
    id: &str,
    marker: &str,
) -> WorkflowLearningCandidate {
    let mut row = candidate(id, &format!("{marker}-summary"));
    row.fingerprint = format!("candidate-io-{id}");
    row.source_binding = Some(WorkflowLearningCandidateSourceBinding::Session {
        tenant_context: original.tenant_context.clone(),
        actor_id: "alice".into(),
        subject: "alice".into(),
        session_id: "candidate-io-session".into(),
    });
    row.proposed_memory_payload = Some(json!({"content": format!("{marker}-memory")}));
    row.proposed_revision_prompt = Some(format!("{marker}-revision"));
    row.evidence_refs = vec![json!({"nested": {"private_fact": format!("{marker}-evidence")}})];
    row.artifact_refs = vec![format!("artifact://{marker}-artifact")];
    row
}

async fn cache_snapshot(state: &AppState) -> Value {
    let rows = state.workflow_learning_candidates.read().await;
    serde_json::to_value(&*rows).unwrap()
}

async fn seeded_fixture() -> (CommitFixture, VerifiedTenantContext, (Value, Vec<u8>)) {
    let mut fixture = CommitFixture::new().await;
    fixture.state.workflow_learning_candidates_path = fixture
        .directory
        .path()
        .join("candidate-store")
        .join("candidates.json");
    let original = fixture.identity(60_000);
    seed_candidate(&fixture.state).await;
    with_hosted_candidate_crypto(
        fixture.state.put_workflow_learning_candidate(private_candidate(
            &original,
            "private-baseline",
            BASE_MARKER,
        )),
    )
    .await
    .expect("healthy hosted complete private candidate seed");
    let before = (
        cache_snapshot(&fixture.state).await,
        tokio::fs::read(&fixture.state.workflow_learning_candidates_path)
            .await
            .unwrap(),
    );
    assert!(before.1.starts_with(b"tgs1:"));
    (fixture, original, before)
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    fn visit(path: &Path, files: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(path).expect("read complete fixture root") {
            let path = entry.expect("fixture entry").path();
            if path.is_dir() {
                visit(&path, files);
            } else {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    visit(root, &mut files);
    files
}

fn assert_no_temporary_files(root: &Path) {
    assert!(
        files_under(root)
            .iter()
            .all(|path| !path.extension().is_some_and(|extension| extension == "tmp")),
        "candidate failure must not leave a prepared file"
    );
}

fn assert_missing_file(path: &Path) {
    assert_eq!(
        std::fs::metadata(path)
            .expect_err("missing durable candidate store must remain absent")
            .kind(),
        std::io::ErrorKind::NotFound
    );
}

fn assert_complete_root_is_sealed(root: &Path) {
    let files = files_under(root);
    assert!(
        !files.is_empty(),
        "raw-root witness must include actual files"
    );
    for path in files {
        let raw = std::fs::read(path).expect("read every raw fixture file without exclusions");
        for marker in [
            BASE_MARKER,
            DENIED_MARKER,
            RECOVERY_MARKER,
            "Original synthetic fact",
        ] {
            assert!(
                !raw.windows(marker.len())
                    .any(|window| window == marker.as_bytes()),
                "complete raw fixture root contains a private candidate marker"
            );
        }
    }
}

async fn assert_healthy_recovery(
    fixture: &CommitFixture,
    original: &VerifiedTenantContext,
    authority: MemoryCommitAuthority,
) {
    authority().expect("same original authority remains live at recovery");
    let saved = with_hosted_candidate_crypto(
        fixture
            .state
            .upsert_workflow_learning_candidate_with_commit_authority(
                private_candidate(original, "recovered-candidate", RECOVERY_MARKER),
                authority,
            ),
    )
    .await
    .expect("same-path healthy guarded publication recovers");
    assert_eq!(saved.candidate_id, "recovered-candidate");
    assert_eq!(
        serde_json::to_value(
            fixture
                .state
                .get_workflow_learning_candidate("recovered-candidate")
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(saved).unwrap()
    );
    // The existing helper creates the fresh state before entering test crypto.
    assert_sealed_durable_matches_cache(&fixture.state).await;
    assert_no_temporary_files(fixture.directory.path());
    assert_complete_root_is_sealed(fixture.directory.path());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_missing_candidate_store_denies_guarded_writer_and_recovers() {
    for cancel_caller in [false, true] {
        let (fixture, original, before) = seeded_fixture().await;
        let original_permissions =
            std::fs::metadata(&fixture.state.workflow_learning_candidates_path)
                .unwrap()
                .permissions();
        let authority = derived_memory_commit_authority(
            &fixture.state,
            &original.tenant_context,
            Some(&original),
        );
        authority().expect("original authority is healthy before the writer wait");
        let writer = fixture.state.workflow_learning_candidates.write().await;
        let state = fixture.state.clone();
        let commit_authority = authority.clone();
        let proposal = private_candidate(&original, "denied-candidate", DENIED_MARKER);
        let (queued, receiver) = oneshot::channel();
        let task = tokio::spawn(async move {
            with_hosted_candidate_crypto(
                state.upsert_workflow_learning_candidate_with_commit_authority_and_writer_wait(
                    proposal,
                    commit_authority,
                    queued,
                ),
            )
            .await
        });
        started(receiver).await;
        let reader = queue_reader_behind_candidate(&fixture.state, &task).await;
        assert_eq!(serde_json::to_value(&*writer).unwrap(), before.0);
        assert_eq!(
            tokio::fs::read(&fixture.state.workflow_learning_candidates_path)
                .await
                .unwrap(),
            before.1
        );
        tokio::fs::remove_file(&fixture.state.workflow_learning_candidates_path)
            .await
            .unwrap();
        assert_missing_file(&fixture.state.workflow_learning_candidates_path);
        assert_eq!(serde_json::to_value(&*writer).unwrap(), before.0);
        assert_no_temporary_files(fixture.directory.path());
        let publication = fixture
            .state
            .enterprise
            .hosted_policy
            .lock_publication_owned()
            .await;
        if cancel_caller {
            task.abort();
        }
        drop(writer);
        witness_candidate_before_reader(&fixture.state, &reader).await;
        assert_missing_file(&fixture.state.workflow_learning_candidates_path);
        drop(publication);
        if cancel_caller {
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            let error = finish_candidate(task)
                .await
                .expect_err("guarded writer cannot resurrect the missing initialized file");
            assert!(format!("{error:?}").contains("missing after initialization"));
        }
        tokio::time::timeout(Duration::from_secs(4), reader)
            .await
            .unwrap()
            .unwrap();
        authority().expect("denial must be missing storage, not stale authority");
        assert_eq!(cache_snapshot(&fixture.state).await, before.0);
        assert_missing_file(&fixture.state.workflow_learning_candidates_path);
        assert_no_temporary_files(fixture.directory.path());
        tokio::fs::write(&fixture.state.workflow_learning_candidates_path, &before.1)
            .await
            .unwrap();
        std::fs::set_permissions(
            &fixture.state.workflow_learning_candidates_path,
            original_permissions,
        )
        .unwrap();
        assert_candidates_unchanged(&fixture.state, &before).await;
        assert_healthy_recovery(&fixture, &original, authority).await;
    }
}

#[cfg(unix)]
struct PermissionRestore {
    path: PathBuf,
    permissions: std::fs::Permissions,
}

#[cfg(unix)]
impl PermissionRestore {
    fn restrict(path: &Path, mode: u32) -> Self {
        use std::os::unix::fs::PermissionsExt;

        let permissions = std::fs::metadata(path).unwrap().permissions();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        Self {
            path: path.to_path_buf(),
            permissions,
        }
    }

    fn restore(&self) {
        std::fs::set_permissions(&self.path, self.permissions.clone())
            .expect("restore exact original permissions");
    }
}

#[cfg(unix)]
impl Drop for PermissionRestore {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.path, self.permissions.clone());
    }
}

#[cfg(unix)]
fn assert_permission_denied(error: &anyhow::Error) {
    let io = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<std::io::Error>())
        .expect("native operation must preserve its actual I/O error");
    assert_eq!(io.kind(), std::io::ErrorKind::PermissionDenied);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tan_829_candidate_permission_denied_preserves_all_writers_and_recovers() {
    let (fixture, original, before) = seeded_fixture().await;
    let authority = derived_memory_commit_authority(
        &fixture.state,
        &original.tenant_context,
        Some(&original),
    );
    let path = &fixture.state.workflow_learning_candidates_path;
    for deny_read in [true, false] {
        let permissions = if deny_read {
            PermissionRestore::restrict(path, 0o000)
        } else {
            PermissionRestore::restrict(path.parent().unwrap(), 0o500)
        };
        let preflight = if deny_read {
            std::fs::read(path).expect_err("privileged read bypass must fail the denial witness")
        } else {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.with_extension("permission-probe.tmp"))
                .expect_err("privileged create bypass must fail the denial witness")
        };
        assert_eq!(preflight.kind(), std::io::ErrorKind::PermissionDenied);
        for operation in ["load", "put", "upsert", "update", "guarded"] {
            authority().expect("original identity and policy remain live during I/O denial");
            let result: anyhow::Result<()> = with_hosted_candidate_crypto(async {
                match operation {
                    "load" => fixture.state.load_workflow_learning_candidates().await,
                    "put" => fixture
                        .state
                        .put_workflow_learning_candidate(private_candidate(
                            &original,
                            "denied-candidate",
                            DENIED_MARKER,
                        ))
                        .await
                        .map(|_| ()),
                    "upsert" => fixture
                        .state
                        .upsert_workflow_learning_candidate(private_candidate(
                            &original,
                            "denied-candidate",
                            DENIED_MARKER,
                        ))
                        .await
                        .map(|_| ()),
                    "update" => fixture
                        .state
                        .update_workflow_learning_candidate("private-baseline", |row| {
                            row.summary = DENIED_MARKER.into();
                        })
                        .await
                        .map(|_| ()),
                    "guarded" => fixture
                        .state
                        .upsert_workflow_learning_candidate_with_commit_authority(
                            private_candidate(&original, "denied-candidate", DENIED_MARKER),
                            authority.clone(),
                        )
                        .await
                        .map(|_| ()),
                    _ => unreachable!(),
                }
            })
            .await;
            if operation == "load" && !deny_read {
                result.expect("readable sealed store remains healthy while creates are denied");
            } else {
                assert_permission_denied(&result.expect_err("OS permission denial must propagate"));
            }
            assert_eq!(cache_snapshot(&fixture.state).await, before.0);
            assert_no_temporary_files(fixture.directory.path());
            if !deny_read {
                assert_eq!(tokio::fs::read(path).await.unwrap(), before.1);
            }
        }
        permissions.restore();
        drop(permissions);
        assert_candidates_unchanged(&fixture.state, &before).await;
        assert_complete_root_is_sealed(fixture.directory.path());
    }
    assert_healthy_recovery(&fixture, &original, authority).await;
}
