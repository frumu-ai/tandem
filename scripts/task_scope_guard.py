#!/usr/bin/env python3
"""Fail-closed preflight and PR diff guard for repository-modifying fleet tasks."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import subprocess
import sys
from typing import Any


DENIED_STATES = {"parked", "canceled", "cancelled", "blocked", "excluded"}


def human_actor(value: Any) -> bool:
    actor = str(value or "").strip()
    return bool(actor) and not actor.lower().startswith(("agent", "codex", "fleet"))


def read_trusted_registry(
    trust_registry_path: pathlib.Path, trust_ref: str
) -> bytes:
    requested_path = trust_registry_path.resolve()
    repository_root = pathlib.Path(
        subprocess.run(
            [
                "git",
                "-C",
                str(requested_path.parent),
                "rev-parse",
                "--show-toplevel",
            ],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    ).resolve()
    try:
        repository_path = requested_path.relative_to(repository_root).as_posix()
    except ValueError as error:
        raise ValueError("task scope trust registry must be inside the repository") from error
    verified_ref = subprocess.run(
        [
            "git",
            "-C",
            str(repository_root),
            "rev-parse",
            "--verify",
            f"{trust_ref}^{{commit}}",
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if not verified_ref:
        raise ValueError("task scope trust ref did not resolve to a commit")
    return subprocess.run(
        ["git", "-C", str(repository_root), "show", f"{verified_ref}:{repository_path}"],
        check=True,
        capture_output=True,
    ).stdout


def load_scope(
    path: pathlib.Path, trust_registry_path: pathlib.Path, trust_ref: str
) -> tuple[dict[str, Any], str, dict[str, Any], str]:
    raw = path.read_bytes()
    scope = json.loads(raw)
    if scope.get("schema_version") != 1:
        raise ValueError("task scope schema_version must be 1")
    for field in (
        "task_id",
        "authorization",
        "issues",
        "repository_areas",
        "permitted_deliverables",
    ):
        if not scope.get(field):
            raise ValueError(f"task scope requires {field}")
    authorization = scope["authorization"]
    approver = str(authorization.get("approved_by", "")).strip()
    if (
        not human_actor(approver)
        or not authorization.get("approved_at")
        or not authorization.get("source")
    ):
        raise ValueError("task scope requires a recorded human authorization")
    digest = hashlib.sha256(raw).hexdigest()

    registry_raw = read_trusted_registry(trust_registry_path, trust_ref)
    registry = json.loads(registry_raw)
    if registry.get("schema_version") != 1:
        raise ValueError("task scope trust registry schema_version must be 1")
    approval = next(
        (
            row
            for row in registry.get("approved_scopes", [])
            if row.get("task_id") == scope["task_id"] and row.get("scope_digest") == digest
        ),
        None,
    )
    if (
        not approval
        or not human_actor(approval.get("approved_by"))
        or not approval.get("approved_at")
        or not approval.get("source")
    ):
        raise ValueError(
            "task scope digest is not present in the trusted human-approved registry"
        )
    return scope, digest, approval, hashlib.sha256(registry_raw).hexdigest()


def human_approval(scope: dict[str, Any], kind: str, value: str) -> dict[str, Any] | None:
    for approval in scope.get("scope_expansion_approvals", []):
        approver = str(approval.get("approved_by", "")).strip()
        if (
            approval.get("kind") == kind
            and approval.get("value") == value
            and approval.get("decision") == "approved"
            and approver
            and not approver.lower().startswith(("agent", "codex", "fleet"))
            and approval.get("approved_at")
        ):
            return approval
    return None


def effective_issue_ids(scope: dict[str, Any]) -> set[str]:
    allowed: set[str] = set()
    for issue in scope["issues"]:
        issue_id = str(issue.get("id", "")).upper()
        state = str(issue.get("state", "")).lower()
        if state == "approved":
            allowed.add(issue_id)
        elif state in DENIED_STATES and human_approval(scope, "issue", issue_id):
            allowed.add(issue_id)
    return allowed


def effective_repository_areas(scope: dict[str, Any]) -> list[str]:
    areas = [normalize_repo_path(value) for value in scope["repository_areas"]]
    areas.extend(
        normalize_repo_path(approval["value"])
        for approval in scope.get("scope_expansion_approvals", [])
        if approval.get("kind") == "repository_area"
        and human_approval(scope, "repository_area", approval.get("value", ""))
    )
    return sorted(set(areas))


def normalize_repo_path(value: str) -> str:
    path = value.replace("\\", "/").strip()
    while path.startswith("./"):
        path = path[2:]
    if not path or path.startswith("../") or "/../" in path:
        raise ValueError(f"invalid repository path: {value!r}")
    return path.rstrip("/")


def path_is_allowed(scope: dict[str, Any], candidate: str) -> bool:
    candidate = normalize_repo_path(candidate)
    areas = effective_repository_areas(scope)
    return any(candidate == area or candidate.startswith(f"{area}/") for area in areas)


def write_receipt(path: str | None, receipt: dict[str, Any]) -> None:
    encoded = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
    if path:
        pathlib.Path(path).write_text(encoded, encoding="utf-8")
    print(encoded, file=sys.stdout if receipt["allowed"] else sys.stderr, end="")


def preflight(
    args: argparse.Namespace,
    scope: dict[str, Any],
    digest: str,
    trust_approval: dict[str, Any],
    trust_registry_digest: str,
) -> int:
    requested = {issue.upper() for issue in args.issue}
    allowed = effective_issue_ids(scope)
    denied = sorted(requested - allowed)
    deliverable_denied = sorted(set(args.deliverable) - set(scope["permitted_deliverables"]))
    receipt = {
        "receipt_type": "fleet_task_scope_preflight",
        "task_id": scope["task_id"],
        "task_authorization": scope["authorization"],
        "scope_digest": digest,
        "trusted_scope_approval": trust_approval,
        "trust_registry_digest": trust_registry_digest,
        "requested_issues": sorted(requested),
        "effective_issues": sorted(allowed),
        "effective_repository_areas": effective_repository_areas(scope),
        "requested_deliverables": sorted(set(args.deliverable)),
        "scope_expansion_approvals": scope.get("scope_expansion_approvals", []),
        "denied_expansion_attempts": {
            "issues": denied,
            "deliverables": deliverable_denied,
            "missing_linked_issue": not requested,
        },
        "allowed": bool(requested) and not denied and not deliverable_denied,
    }
    write_receipt(args.receipt, receipt)
    return 0 if receipt["allowed"] else 2


def changed_files(base: str, head: str) -> list[str]:
    result = subprocess.run(
        ["git", "diff", "--name-only", f"{base}...{head}"],
        check=True,
        capture_output=True,
        text=True,
    )
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]


def diff_guard(
    args: argparse.Namespace,
    scope: dict[str, Any],
    digest: str,
    trust_approval: dict[str, Any],
    trust_registry_digest: str,
) -> int:
    files = changed_files(args.base, args.head)
    denied_files = sorted(path for path in files if not path_is_allowed(scope, path))
    linked = {issue.upper() for issue in args.linked_issue}
    denied_issues = sorted(linked - effective_issue_ids(scope))
    receipt = {
        "receipt_type": "fleet_task_scope_diff",
        "task_id": scope["task_id"],
        "task_authorization": scope["authorization"],
        "scope_digest": digest,
        "trusted_scope_approval": trust_approval,
        "trust_registry_digest": trust_registry_digest,
        "base": args.base,
        "head": args.head,
        "linked_issues": sorted(linked),
        "effective_issues": sorted(effective_issue_ids(scope)),
        "effective_repository_areas": effective_repository_areas(scope),
        "scope_expansion_approvals": scope.get("scope_expansion_approvals", []),
        "final_linked_changes": files,
        "denied_expansion_attempts": {
            "issues": denied_issues,
            "repository_paths": denied_files,
            "missing_linked_issue": not linked,
        },
        "allowed": bool(linked) and not denied_files and not denied_issues,
    }
    write_receipt(args.receipt, receipt)
    return 0 if receipt["allowed"] else 2


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser()
    root.add_argument("--scope", required=True)
    root.add_argument("--trust-registry", required=True)
    root.add_argument(
        "--trust-ref",
        required=True,
        help="protected base commit/ref from which the trust registry must be read",
    )
    sub = root.add_subparsers(dest="command", required=True)
    pre = sub.add_parser("preflight")
    pre.add_argument("--issue", action="append", default=[])
    pre.add_argument("--deliverable", action="append", default=[])
    pre.add_argument("--receipt")
    diff = sub.add_parser("diff")
    diff.add_argument("--base", required=True)
    diff.add_argument("--head", required=True)
    diff.add_argument("--linked-issue", action="append", default=[])
    diff.add_argument("--receipt")
    return root


def main() -> int:
    args = parser().parse_args()
    try:
        scope, digest, trust_approval, trust_registry_digest = load_scope(
            pathlib.Path(args.scope), pathlib.Path(args.trust_registry), args.trust_ref
        )
        if args.command == "preflight":
            return preflight(
                args, scope, digest, trust_approval, trust_registry_digest
            )
        return diff_guard(
            args, scope, digest, trust_approval, trust_registry_digest
        )
    except (OSError, ValueError, json.JSONDecodeError, subprocess.CalledProcessError) as error:
        print(json.dumps({"allowed": False, "error": str(error)}), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
