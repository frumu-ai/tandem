---
title: AI Quality Assurance with Eval-Runner
---

# AI Quality Assurance with Eval-Runner

Tandem's `eval-runner` CLI evaluates AI quality by running standardized test datasets and measuring success metrics like pass rate, repair iterations, and cost per task. This guide explains how to set up and run evaluations against your Tandem engine.

## Overview

The eval-runner has three execution modes:

| Mode           | Engine      | Provider         | Cost   | Use Case                                     |
| -------------- | ----------- | ---------------- | ------ | -------------------------------------------- |
| **Simulation** | None        | None             | $0     | Fast CI gate (no real AI)                    |
| **Stub**       | Real engine | Mock provider    | $0     | Test engine execution path deterministically |
| **Live**       | Real engine | Real AI provider | Real $ | Capture realistic baselines with actual AI   |

## Prerequisites

- **Tandem engine running** — accessible via HTTP (default: `http://127.0.0.1:39731`)
- **Engine API token** — for authentication (from systemd service or local keychain)
- **Eval datasets** — YAML test case definitions in `eval_datasets/`

## Setup

### 1. Start Your Engine

If you have Tandem running as a systemd service:

```bash
systemctl status tandem-engine
# Or restart if needed:
systemctl restart tandem-engine
```

For manual testing, start the engine in the foreground:

```bash
export TANDEM_API_TOKEN=tk_your_token_here
tandem-engine serve --hostname 127.0.0.1 --port 39731
```

### 2. Obtain Your Engine Token

The eval-runner needs your engine's API token for authentication. The token is resolved in this order:

1. `--engine-token` CLI flag (highest priority)
2. `TANDEM_API_TOKEN` environment variable
3. Token file at `~/.local/share/tandem/security/engine_api_token`
4. Shared keychain

For a systemd service, extract the token from the environment file:

```bash
export TANDEM_API_TOKEN="$(sed -n 's/^TANDEM_API_TOKEN=//p' /etc/tandem/engine.env | head -1)"
```

Verify it's correct:

```bash
curl -s "http://127.0.0.1:39731/config/providers" \
  -H "X-Tandem-Token: $TANDEM_API_TOKEN" | jq .
```

## Running Evaluations

### Simulation Mode (Fast, No API Cost)

For quick validation without using your AI provider:

```bash
cargo run -p tandem-eval --bin eval-runner -- \
  --dataset eval_datasets/critical_path.yaml \
  --engine-mode simulation \
  --output /tmp/results.json
```

**When to use:** Per-PR CI gate, quick smoke test.

### Live Mode (Real AI Quality)

To evaluate against your running engine with real AI provider calls:

```bash
export TANDEM_API_TOKEN="$(sed -n 's/^TANDEM_API_TOKEN=//p' /etc/tandem/engine.env | head -1)"

cargo run -p tandem-eval --bin eval-runner -- \
  --dataset eval_datasets/critical_path.yaml \
  --engine-mode live \
  --engine-url http://127.0.0.1:39731 \
  --engine-token $TANDEM_API_TOKEN \
  --verbose
```

**Options:**

- `--num-workers <N>` — Parallel execution (default: 1)
- `--filter-tag <TAG>` — Run only tests with this tag
- `--max-duration <SECS>` — Timeout per test (default: 300)
- `--verbose` — Print detailed progress

**Output:**

```
Tandem Eval Runner v0.1.0
Dataset: eval_datasets/critical_path.yaml
Output: ./eval_results.json
Mode: LIVE (anthropic/claude-haiku-4-5-20251001)

Loaded dataset 'critical_path' v1.0 (10 test cases)
Running evaluation...

=== Evaluation Results: critical_path v1.0 ===
Duration: 45000 ms
Tests: 10 total (9 passed, 1 failed, 0 skipped)
Pass Rate: 90.0%
Avg Repair Iterations: 1.4
Total Cost: $0.49

Failed Tests:
  - critical_005: Provider timeout after 3 retries

Results saved to: ./eval_results.json
```

### Stub Mode (Deterministic Engine Testing)

For testing the engine execution path with scripted (deterministic) AI responses:

```bash
export TANDEM_API_TOKEN="$(sed -n 's/^TANDEM_API_TOKEN=//p' /etc/tandem/engine.env | head -1)"

cargo run -p tandem-eval --bin eval-runner -- \
  --dataset eval_datasets/critical_path.yaml \
  --engine-mode stub \
  --engine-url http://127.0.0.1:39731 \
  --engine-token $TANDEM_API_TOKEN
```

**When to use:** After engine changes (validators, repair logic), before committing to cost of live runs.

## Automation: Nightly QA Workflow

Create a Tandem automation that runs evals on a schedule and gates baseline updates:

### Step 1: Create Workflow in Control Panel

In the **Workflow Studio**, create a new workflow with these nodes:

**Node 1: Run Evaluation**

```
Type: Research
Objective: |
  Run eval-runner against our engine in live mode
  Command:
  export TANDEM_API_TOKEN="$(sed -n 's/^TANDEM_API_TOKEN=//p' /etc/tandem/engine.env | head -1)"
  cargo run -p tandem-eval --bin eval-runner -- \
    --dataset eval_datasets/critical_path.yaml \
    --engine-mode live \
    --engine-url http://127.0.0.1:39731 \
    --engine-token $TANDEM_API_TOKEN \
    --output /tmp/nightly_eval.json \
    --verbose

  Return: evaluation results JSON
```

**Node 2: Compare to Baseline**

```
Type: Code
Objective: |
  Load eval_baselines/main_branch.json
  Compare against /tmp/nightly_eval.json using detect_regressions()
  Determine if any metrics regressed past thresholds:
  - Pass rate drop > 5 percentage points
  - Cost increase > 20%
  - Repair iterations increase > 30%

  Return: regression report with severity (PASS | WARNING | CRITICAL)
```

**Node 3: File Issues (if regressions)**

```
Type: Code
Objective: |
  If regressions detected:
  - Create GitHub issue using mcp.github.create_issue
  - Tag with "regression" label
  - Include test failure details and metrics diff

  If no regressions: skip
```

**Node 4: Human Review**

```
Type: Approval (conditional)
Objective: |
  If regressions detected:
  - Show regression summary
  - Request approval to accept baseline change
  - Options: Approve | Investigate | Revert

  If passing: auto-complete
```

**Node 5: Update Baseline (on approval)**

```
Type: Code
Objective: |
  If approved, commit baseline update:
  cp /tmp/nightly_eval.json eval_baselines/main_branch.json
  git add eval_baselines/main_branch.json
  git commit -m "Update eval baseline from nightly run"
  git push
```

**Node 6: Notify Discord**

```
Type: Summarization
Objective: |
  Post results to #quality-gate channel:
  [PASS] All evals passed - baseline updated
  or
  [REGRESSED] 2 metrics regressed - GitHub issues created
```

### Step 2: Set Schedule

In the automation card:

- Schedule: **Every day** at **2:00 AM**
- Timezone: Your local timezone

### Step 3: Test Manually

Click "Run now" to verify the workflow completes successfully before waiting for the nightly run.

## Interpreting Results

### JSON Output Structure

```json
{
  "name": "critical_path",
  "version": "1.0",
  "started_at_ms": 1715814000000,
  "finished_at_ms": 1715814045000,
  "duration_ms": 45000,
  "total_tests": 10,
  "passed_tests": 9,
  "failed_tests": 1,
  "skipped_tests": 0,
  "pass_rate": 90.0,
  "avg_repair_iterations": 1.4,
  "total_cost_usd": 0.49,
  "avg_cost_per_test": 0.049,
  "test_results": [
    {
      "test_id": "critical_001",
      "passed": true,
      "artifact_status": "completed",
      "repair_iterations": 1,
      "tokens_used": 1500,
      "cost_usd": 0.0045,
      "validators_passed": ["contract", "markdown_structure"],
      "validators_failed": [],
      "failure_mode": null
    },
    {
      "test_id": "critical_005",
      "passed": false,
      "artifact_status": "failed",
      "failure_mode": "ProviderTimeout",
      "error_message": "Provider timeout after 3 retries"
    }
  ]
}
```

### Key Metrics

- **Pass Rate**: % of tests completing successfully on first attempt or after repair
- **Avg Repair Iterations**: How many times the engine re-attempted failed tasks (lower is better)
- **Cost/Test**: Token usage × model pricing (track for budget awareness)
- **Failure Modes**: Categorized error types (ProviderTimeout, ValidationFailed, etc.)

## Troubleshooting

### Connection Error: "Engine unreachable"

```
Error: HTTP request failed: connection refused
```

**Solution:**

1. Verify engine is running: `curl -s http://127.0.0.1:39731/config/providers`
2. Check port: `ss -tlnp | grep 39731` (on Linux)
3. Verify firewall if remote engine

### Authentication Error: "HTTP 401"

```
Error: HTTP 401: {"error": "unauthorized"}
```

**Solution:**

1. Verify token is set: `echo $TANDEM_API_TOKEN`
2. Test token directly: `curl -H "X-Tandem-Token: $TANDEM_API_TOKEN" http://127.0.0.1:39731/config/providers`
3. Check token file: `cat ~/.local/share/tandem/security/engine_api_token`

### Timeout: "eval timeout after 300s"

Some tests may take longer than 300 seconds. Increase with `--max-duration`:

```bash
cargo run -p tandem-eval --bin eval-runner -- \
  --dataset eval_datasets/critical_path.yaml \
  --engine-mode live \
  --max-duration 600
```

### Subprocess Error: "command not found"

If running within a Tandem automation, ensure `cargo` is in the PATH:

```bash
export PATH="/usr/local/cargo/bin:$PATH"
cargo run -p tandem-eval --bin eval-runner -- ...
```

## Advanced: Custom Scripted Responses (Stub Mode)

To customize stub provider responses for a specific test:

In `crates/tandem-eval/src/scripted_provider.rs`, add patterns:

```rust
provider
    .with_pattern("your test objective", ScriptedResponse::Json(serde_json::json!({
        "summary": "Custom response for your test",
        "citations": ["https://example.com"],
        "content": "Detailed stub response..."
    })))
```

Rebuild and run with `--engine-mode stub`.

## See Also

- [EVAL_FRAMEWORK.md](../dev/EVAL_FRAMEWORK.md) — Deep dive on dataset format, architecture, regression detection
- [AI Quality Assurance](./ai-quality-assurance.md) — User-facing compliance guide
- Eval datasets: `eval_datasets/*.yaml`
- Baseline file: `eval_baselines/main_branch.json`
