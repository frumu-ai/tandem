import argparse
import hashlib
import importlib.util
import json
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock


SCRIPT = pathlib.Path(__file__).parents[1] / "task_scope_guard.py"
SPEC = importlib.util.spec_from_file_location("task_scope_guard", SCRIPT)
guard = importlib.util.module_from_spec(SPEC)
assert SPEC.loader
SPEC.loader.exec_module(guard)


def write_json(path: pathlib.Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value), encoding="utf-8")


def git(root: pathlib.Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def create_repository(root: pathlib.Path, approve_scope: bool):
    git(root, "init", "-q")
    git(root, "config", "user.name", "Task Scope Test")
    git(root, "config", "user.email", "scope-test@example.com")
    scope_path = root / ".tandem" / "task-scope.json"
    registry_path = root / ".tandem" / "approved-task-scopes.json"
    write_json(
        scope_path,
        {
            "schema_version": 1,
            "task_id": "scope-test",
            "authorization": {
                "approved_by": "human@example.com",
                "approved_at": "2026-07-15T07:25:15Z",
                "source": "test fixture",
            },
            "issues": [{"id": "TAN-748", "state": "approved"}],
            "repository_areas": ["crates/tandem-server"],
            "permitted_deliverables": ["code"],
        },
    )
    digest = hashlib.sha256(scope_path.read_bytes()).hexdigest()
    approvals = []
    if approve_scope:
        approvals.append(
            {
                "task_id": "scope-test",
                "scope_digest": digest,
                "approved_by": "human@example.com",
                "approved_at": "2026-07-15T07:25:15Z",
                "source": "test fixture",
            }
        )
    write_json(registry_path, {"schema_version": 1, "approved_scopes": approvals})
    git(root, "add", ".tandem/approved-task-scopes.json")
    git(root, "commit", "-q", "-m", "trusted registry")
    return scope_path, registry_path, digest, git(root, "rev-parse", "HEAD")


class TaskScopeRegistryTrustTests(unittest.TestCase):
    def test_candidate_registry_edit_cannot_self_approve_scope(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            scope_path, registry_path, digest, base = create_repository(root, False)
            write_json(
                registry_path,
                {
                    "schema_version": 1,
                    "approved_scopes": [
                        {
                            "task_id": "scope-test",
                            "scope_digest": digest,
                            "approved_by": "human@example.com",
                            "approved_at": "2026-07-15T08:00:00Z",
                            "source": "candidate diff",
                        }
                    ],
                },
            )

            with self.assertRaisesRegex(ValueError, "trusted human-approved registry"):
                guard.load_scope(scope_path, registry_path, base)

    def test_registry_is_loaded_from_trusted_ref_not_worktree(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            scope_path, registry_path, digest, base = create_repository(root, True)
            trusted_registry_raw = git(
                root, "show", f"{base}:.tandem/approved-task-scopes.json"
            ).encode()
            write_json(registry_path, {"schema_version": 1, "approved_scopes": []})

            _, loaded_digest, approval, registry_digest = guard.load_scope(
                scope_path, registry_path, base
            )

            self.assertEqual(loaded_digest, digest)
            self.assertEqual(approval["source"], "test fixture")
            self.assertEqual(
                registry_digest, hashlib.sha256(trusted_registry_raw).hexdigest()
            )


def scope():
    return {
        "schema_version": 1,
        "task_id": "scope-test",
        "authorization": {
            "approved_by": "evan@frumu.ai",
            "approved_at": "2026-07-15T07:25:15Z",
            "source": "test fixture",
        },
        "issues": [
            {"id": "TAN-748", "state": "approved"},
            {"id": "TAN-742", "state": "parked"},
            {"id": "TAN-743", "state": "parked"},
        ],
        "repository_areas": ["crates/tandem-server", "docs/scope.md"],
        "permitted_deliverables": ["code", "tests"],
        "scope_expansion_approvals": [],
    }


def trust_approval():
    return {
        "task_id": "scope-test",
        "scope_digest": "digest",
        "approved_by": "evan@frumu.ai",
        "approved_at": "2026-07-15T07:25:15Z",
        "source": "test fixture",
    }


class TaskScopeGuardTests(unittest.TestCase):
    def test_parked_issues_are_denied(self):
        self.assertEqual(guard.effective_issue_ids(scope()), {"TAN-748"})

    def test_spawn_or_retry_cannot_expand_paths(self):
        task_scope = scope()
        self.assertTrue(guard.path_is_allowed(task_scope, "crates/tandem-server/src/lib.rs"))
        self.assertFalse(guard.path_is_allowed(task_scope, "packages/control-panel/src/app.tsx"))

    def test_exact_human_approval_can_expand_issue_without_unparking_adjacent_issue(self):
        task_scope = scope()
        task_scope["scope_expansion_approvals"] = [
            {
                "kind": "issue",
                "value": "TAN-742",
                "decision": "approved",
                "approved_by": "evan@frumu.ai",
                "approved_at": "2026-07-15T10:00:00Z",
            }
        ]
        self.assertEqual(guard.effective_issue_ids(task_scope), {"TAN-748", "TAN-742"})
        self.assertNotIn("TAN-743", guard.effective_issue_ids(task_scope))

    def test_agent_self_approval_is_rejected(self):
        task_scope = scope()
        task_scope["scope_expansion_approvals"] = [
            {
                "kind": "issue",
                "value": "TAN-742",
                "decision": "approved",
                "approved_by": "codex-fleet",
                "approved_at": "2026-07-15T10:00:00Z",
            }
        ]
        self.assertNotIn("TAN-742", guard.effective_issue_ids(task_scope))

    def test_preflight_fails_without_an_explicit_linked_issue(self):
        args = argparse.Namespace(issue=[], deliverable=["code"], receipt=None)
        self.assertEqual(
            guard.preflight(args, scope(), "digest", trust_approval(), "registry"),
            2,
        )

    def test_parked_issue_cannot_pass_the_pr_diff_guard(self):
        args = argparse.Namespace(
            base="base",
            head="head",
            linked_issue=["TAN-742"],
            receipt=None,
        )
        with mock.patch.object(
            guard,
            "changed_files",
            return_value=["crates/tandem-server/src/lib.rs"],
        ):
            self.assertEqual(
                guard.diff_guard(
                    args, scope(), "digest", trust_approval(), "registry"
                ),
                2,
            )


if __name__ == "__main__":
    unittest.main()
