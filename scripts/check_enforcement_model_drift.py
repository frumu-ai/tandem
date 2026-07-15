#!/usr/bin/env python3
"""Keep the published enforcement-mode matrix tied to runtime gates and tests."""

import argparse
import re
import subprocess
from pathlib import Path


PAGE = Path("guide/src/content/docs/policy-and-enforcement-model.md")
MANIFEST = Path(".github/governance-audit-critical-tests.txt")
ENGINE_CI = Path(".github/workflows/engine-ci.yml")
DRIFT_WORKFLOW = Path(".github/workflows/enforcement-model-drift.yml")
RUST_TEST_DEFINITION = re.compile(
    r"(?m)^(?P<attrs>(?:\s*#\[[^\n]+\]\s*\n)+)"
    r"\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+"
    r"(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*\("
)
RUST_TEST_ATTRIBUTE = re.compile(r"#\[(?:tokio::)?test(?:\([^\]]*\))?\]")


def require(text: str, needle: str, source: Path) -> None:
    if needle not in text:
        raise SystemExit(f"{source} is missing required enforcement-model marker: {needle}")


def critical_tests(manifest: str) -> tuple[str, ...]:
    return tuple(
        line.strip()
        for line in manifest.splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    )


def require_manifest_does_not_shrink(current: tuple[str, ...], base_ref: str) -> None:
    try:
        base_manifest = subprocess.check_output(
            ["git", "show", f"{base_ref}:{MANIFEST.as_posix()}"],
            text=True,
            stderr=subprocess.PIPE,
        )
    except subprocess.CalledProcessError as error:
        detail = error.stderr.strip() or "git show failed"
        raise SystemExit(
            f"could not load trusted base audit manifest from {base_ref}: {detail}"
        ) from error
    removed = sorted(set(critical_tests(base_manifest)) - set(current))
    if removed:
        raise SystemExit(
            "audit-critical manifest coverage may not shrink relative to the trusted base; "
            f"removed={removed}"
        )


def require_manifest_tests_exist(critical: tuple[str, ...]) -> None:
    defined_tests: set[str] = set()
    for path in Path("crates").rglob("*.rs"):
        source = path.read_text(encoding="utf-8")
        for match in RUST_TEST_DEFINITION.finditer(source):
            if RUST_TEST_ATTRIBUTE.search(match.group("attrs")):
                defined_tests.add(match.group("name"))
    missing = sorted(set(critical) - defined_tests)
    if missing:
        raise SystemExit(
            "audit-critical manifest entries must resolve to real Rust test definitions; "
            f"missing={missing}"
        )


def main(base_ref: str | None = None) -> None:
    page = PAGE.read_text(encoding="utf-8")
    manifest = MANIFEST.read_text(encoding="utf-8")
    engine_ci = ENGINE_CI.read_text(encoding="utf-8")
    drift_workflow = DRIFT_WORKFLOW.read_text(encoding="utf-8")
    server_policy = Path("crates/tandem-server/src/agent_teams_parts/part01.rs").read_text(
        encoding="utf-8"
    )
    authored = Path(
        "crates/tandem-server/src/agent_teams_parts/enterprise_authored_policy.rs"
    ).read_text(encoding="utf-8")

    for marker in (
        "Local/default",
        "Governed server",
        "Premium/enterprise",
        "Deny-by-default tool dispatch",
        "Parameter-aware authored rules",
        "Risk-tier approval routing",
        "Egress DLP preflight",
        "Approval reviewer identity",
        "Approval timeout behavior",
        "Dispatch receipts",
        "Credential injection",
        "System-initiated service calls",
        "Boot composition guard",
    ):
        require(page, marker, PAGE)

    for marker in (
        ".github/workflows/enforcement-model-drift.yml",
        ".github/workflows/engine-ci.yml",
        ".github/governance-audit-critical-tests.txt",
        "crates/tandem-automation/src/types_tests.rs",
        "crates/tandem-server/src/app/state/tests/**",
        "crates/tandem-server/src/http/tests/approval_gate_matrix.rs",
        "crates/tandem-server/src/http/tests/governance_parts/**",
        "crates/tandem-server/src/pack_builder_parts/**",
        "crates/tandem-server/src/incident_monitor_*.rs",
        "scripts/check_enforcement_model_drift.py",
    ):
        require(drift_workflow, marker, DRIFT_WORKFLOW)

    critical = critical_tests(manifest)
    if not critical:
        raise SystemExit(f"{MANIFEST} contains no audit-critical tests")
    require_manifest_tests_exist(critical)
    if base_ref:
        require_manifest_does_not_shrink(critical, base_ref)
    for test in critical:
        require(page, test, PAGE)

    for marker in (
        "Verify governance audit tests were discovered and executed",
        ".github/governance-audit-critical-tests.txt",
        "governance_routes_fail_closed_without_premium_governance",
        "direct_mcp_calls=",
        "crates/tandem-server/src/pack_builder_parts",
        "crates/tandem-server/src/incident_monitor_mcp.rs",
    ):
        require(engine_ci, marker, ENGINE_CI)

    require(
        server_policy,
        "runtime_auth_mode_requires_verified_tool_context(runtime_auth_mode)",
        Path("crates/tandem-server/src/agent_teams_parts/part01.rs"),
    )
    require(
        authored,
        '#[cfg(feature = "premium-governance")]\n    #[tokio::test]\n    async fn authored_approval_resumes_exactly_one_execution',
        Path("crates/tandem-server/src/agent_teams_parts/enterprise_authored_policy.rs"),
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-ref")
    options = parser.parse_args()
    main(options.base_ref)
