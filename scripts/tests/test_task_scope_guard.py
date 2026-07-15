import hashlib
import importlib.util
import json
import pathlib
import subprocess
import tempfile
import unittest


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


if __name__ == "__main__":
    unittest.main()
