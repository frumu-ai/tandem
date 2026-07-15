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
DRIFT_TRIGGER_PATHS = (
    "guide/src/content/docs/policy-and-enforcement-model.md",
    ".github/workflows/enforcement-model-drift.yml",
    ".github/workflows/engine-ci.yml",
    ".github/governance-audit-critical-tests.txt",
    "crates/tandem-enterprise-contract/src/policy_predicates.rs",
    "crates/tandem-tools/src/tool_dispatcher.rs",
    "crates/tandem-automation/src/orchestration.rs",
    "crates/tandem-automation/src/types_tests.rs",
    "crates/tandem-server/src/agent_teams_parts/**",
    "crates/tandem-server/src/app/state/app_state_impl_parts/part01.rs",
    "crates/tandem-server/src/app/state/automation_v2_wait_nodes.rs",
    "crates/tandem-server/src/app/state/governance_action_gate.rs",
    "crates/tandem-server/src/app/state/mod.rs",
    "crates/tandem-server/src/app/state/tests/**",
    "crates/tandem-server/src/app/state/tool_dispatch_outbox.rs",
    "crates/tandem-server/src/benchmarking/mod.rs",
    "crates/tandem-server/src/http/coder_parts/part05.rs",
    "crates/tandem-server/src/http/pack_builder.rs",
    "crates/tandem-server/src/http/governance.rs",
    "crates/tandem-server/src/http/mcp.rs",
    "crates/tandem-server/src/http/mcp/**",
    "crates/tandem-server/src/http/mcp_run_as.rs",
    "crates/tandem-server/src/http/tests/governance_parts/**",
    "crates/tandem-server/src/http/tests/approval_gate_matrix.rs",
    "crates/tandem-server/src/http/tests/governance.rs",
    "crates/tandem-server/src/incident_monitor_*.rs",
    "crates/tandem-server/src/incident_monitor/**",
    "crates/tandem-server/src/pack_builder.rs",
    "crates/tandem-server/src/pack_builder_parts/**",
    "crates/tandem-runtime/src/mcp_parts/part01.rs",
    "scripts/check_enforcement_model_drift.py",
)
DIRECT_MCP_AUDIT_PATHS = (
    "crates/tandem-server/src/http/coder_parts/part05.rs",
    "crates/tandem-server/src/pack_builder_parts",
    "crates/tandem-server/src/benchmarking/mod.rs",
    "crates/tandem-server/src/incident_monitor_github.rs",
    "crates/tandem-server/src/incident_monitor_linear.rs",
    "crates/tandem-server/src/incident_monitor_mcp.rs",
    "crates/tandem-server/src/incident_monitor_webhook.rs",
)
RUST_TEST_DEFINITION = re.compile(
    r"(?m)^(?P<attrs>(?:\s*#\[[^\n]+\]\s*\n)+)"
    r"\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+"
    r"(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*\("
)
RUST_TEST_ATTRIBUTE = re.compile(r"#\[(?:tokio::)?test(?:\([^\]]*\))?\]")


def require(text: str, needle: str, source: Path) -> None:
    if needle not in text:
        raise SystemExit(f"{source} is missing required enforcement-model marker: {needle}")


def pull_request_paths(workflow: str) -> set[str]:
    paths: set[str] = set()
    collecting = False
    for line in workflow.splitlines():
        if line == "    paths:":
            collecting = True
            continue
        if not collecting:
            continue
        if line.startswith("      - "):
            value = line.removeprefix("      - ").strip()
            if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
                value = value[1:-1]
            paths.add(value)
            continue
        if line.strip():
            break
    if not paths:
        raise SystemExit(f"{DRIFT_WORKFLOW} has no pull_request.paths entries")
    return paths


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
            "required enforcement tests must resolve to real Rust test definitions; "
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

    missing_trigger_paths = sorted(
        set(DRIFT_TRIGGER_PATHS) - pull_request_paths(drift_workflow)
    )
    if missing_trigger_paths:
        raise SystemExit(
            f"{DRIFT_WORKFLOW} is missing required pull_request paths: "
            f"{missing_trigger_paths}"
        )

    critical = critical_tests(manifest)
    if not critical:
        raise SystemExit(f"{MANIFEST} contains no audit-critical tests")
    require_manifest_tests_exist(
        critical + ("governance_routes_fail_closed_without_premium_governance",)
    )
    if base_ref:
        require_manifest_does_not_shrink(critical, base_ref)
    for test in critical:
        require(page, test, PAGE)

    for marker in (
        "Verify governance audit tests were discovered and executed",
        ".github/governance-audit-critical-tests.txt",
        "governance_routes_fail_closed_without_premium_governance",
        "direct_mcp_calls=",
    ):
        require(engine_ci, marker, ENGINE_CI)
    for path in DIRECT_MCP_AUDIT_PATHS:
        require(engine_ci, path, ENGINE_CI)

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
