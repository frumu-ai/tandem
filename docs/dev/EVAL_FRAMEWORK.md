# Tandem AI Evaluation Framework - Developer Guide

## Overview

The Tandem evaluation framework provides structured testing, regression detection, and compliance documentation for AI quality assurance. It consists of:

- **Evaluation Datasets**: Version-controlled YAML/JSON test case definitions
- **Eval Runner CLI**: `cargo run --bin eval-runner` tool for bulk test execution  
- **Metrics Aggregation**: Pass rate, cost, repair iterations, failure mode tracking
- **Regression Detection**: Baseline comparison with configurable alert thresholds
- **Failure Mode Taxonomy**: 30+ categorized AI failure types for post-mortem analysis

## Quick Start

### Running an Evaluation

```bash
# Run critical path tests in simulation mode (no AI calls, deterministic)
cargo run --release --bin eval-runner -- \
  --dataset eval_datasets/critical_path.yaml \
  --output /tmp/results.json \
  --simulation

# Run against live provider and filter by tag
cargo run --bin eval-runner -- \
  --dataset eval_datasets/critical_path.yaml \
  --provider anthropic \
  --model claude-opus-4-7 \
  --filter-tag happy_path \
  --num-workers 4

# View results
jq . /tmp/results.json
```

### Interpreting Results

The CLI outputs:
1. **Human-readable summary** on stdout with pass_rate, costs, failure modes
2. **JSON file** with detailed metrics for each test (for CI/regression detection)

Example output:
```
=== Evaluation Results: critical_path v1.0 ===
Duration: 45000 ms
Tests: 10 total (9 passed, 1 failed, 0 skipped)
Pass Rate: 90.0%
Avg Repair Iterations: 1.4
Total Cost: $0.49
Avg Cost/Test: $0.049

Failure Modes:
  - ProviderTimeout: 1

Failed Tests:
  - critical_005 (Multi-step workflow error recovery): Provider timeout after 3 retries
```

## Architecture

### Module Structure

```
crates/tandem-server/src/
├── eval/
│   ├── mod.rs                  # Module exports
│   ├── dataset.rs              # EvalDataset, EvalTestCase, AutomationSpec
│   ├── metrics.rs              # EvalMetrics, EvalRunResult aggregation
│   ├── runner.rs               # EvalRunner execution engine
│   └── regression_detection.rs # Baseline comparison & thresholds
├── failures/
│   └── mod.rs                  # AIFailureMode taxonomy (30+ failure types)
└── bin/
    └── eval_runner.rs          # CLI entry point
```

### Data Flow

```
eval_datasets/*.yaml (dataset definition)
        ↓
EvalRunner::load_dataset() → EvalDataset struct
        ↓
run_dataset() → execute each EvalTestCase
        ↓
[Simulation mode: deterministic] [Live mode: actual AI calls]
        ↓
EvalMetrics struct (aggregated)
        ↓
detect_regressions() → RegressionReport
        ↓
CI gate (.github/workflows/eval-regression-gate.yml)
        ↓
Pass/Fail decision
```

## Adding New Evaluation Datasets

### 1. Create YAML File

Create `eval_datasets/my_feature.yaml`:

```yaml
name: "my_feature"
version: "1.0"
description: "Tests for feature X"
tags:
  - "feature"
  - "weekly"
created_at: "2026-05-15T00:00:00Z"

test_cases:
  - id: "feature_001"
    description: "Happy path test for feature X"
    priority: 1
    enabled: true
    tags:
      - "happy_path"
    automation_spec:
      name: "test_feature_x"
      nodes:
        - id: "node_1"
          node_type: "generation"
          objective: "Generate output using feature X"
          output_contract: "JSON with field 'result'"
      validators:
        - "contract"
      config:
        max_repair_iterations: 3
    expected_output:
      artifact_status: "completed"
      required_validators:
        - "contract"
      max_repair_iterations: 3
      output_format: "json"
```

### 2. Key Fields

- **`id`**: Unique test case identifier (alphanumeric, lowercase)
- **`priority`**: 1 (critical), 2 (important), 3 (nice-to-have)
- **`enabled`**: Set to `false` to skip test in normal runs
- **`tags`**: Used for `--filter-tag` CLI filtering
- **`automation_spec.nodes`**: Workflow steps (generation, research, code, summarization, etc.)
- **`validators`**: Quality checks to run (`contract`, `citations`, `web_sources`, etc.)
- **`expected_output.artifact_status`**: Expected result (Completed, CompletedWithWarnings, Blocked, Failed)
- **`expected_output.required_validators`**: Validators that must pass for test to pass

### 3. Test Case Guidelines

**Happy path tests** (tag: `happy_path`):
- Simple, successful scenarios
- Should pass >95% of the time
- Use priority 1

**Edge cases** (tag: `edge_case`):
- Unusual inputs or constraints
- Priority 2-3

**Regression tests** (tag: `regression`):
- Tests for previously fixed bugs
- Should fail if bug reappears
- Priority 1

**Slow/expensive tests** (tag: `slow`):
- Set `enabled: false` by default
- Use for weekly/nightly runs
- Priority 3

## Regression Detection

### How It Works

1. **Baseline creation**: `EvalBaseline::from_metrics()` saves current metrics to `eval_baselines/main_branch.json`
2. **Comparison**: `detect_regressions()` compares PR metrics against baseline
3. **Thresholds**: Configurable per-metric (default: 5% pass_rate drop, 20% cost increase)
4. **CI gate**: Workflow in `.github/workflows/eval-regression-gate.yml` fails PR if threshold exceeded

### Default Thresholds

```rust
RegressionThresholds {
    pass_rate_drop_warning: 0.02,              // 2 percentage points
    pass_rate_drop_critical: 0.05,             // 5 percentage points
    cost_increase_warning: 0.10,               // 10% relative increase
    cost_increase_critical: 0.20,              // 20% relative increase
    repair_iter_increase_warning: 0.15,        // 15% relative increase
    repair_iter_increase_critical: 0.30,       // 30% relative increase
    provider_failure_increase_warning: 0.02,   // 2 percentage points
    provider_failure_increase_critical: 0.05,  // 5 percentage points
}
```

### Customizing Thresholds

Modify `detect_regressions()` call in eval_runner.rs:

```rust
let mut thresholds = RegressionThresholds::default();
thresholds.pass_rate_drop_critical = 0.03;  // More lenient: 3pp
thresholds.cost_increase_critical = 0.30;   // More lenient: 30%
let report = detect_regressions(&metrics, &baseline, &thresholds);
```

Or set via environment variables (if parsing is added to runner):

```bash
EVAL_PASS_RATE_CRITICAL=0.03 cargo run --bin eval-runner -- --dataset ...
```

## Failure Mode Taxonomy

The `failures` module categorizes 30+ AI failure types:

### Critical Failures
- **ArtifactValidationFailed**: Output doesn't pass quality validators
- **ContractViolation**: Output structure doesn't match contract
- **ProviderTimeout**: Provider didn't respond in time
- **SourceAccessDenied**: Can't reach data source for validation

### High-Impact Failures
- **RepairBudgetExhausted**: Hit max repair iterations without passing
- **TokenBudgetExhausted**: Consumed max tokens before completion
- **ProviderModelNotFound**: Requested model doesn't exist
- **AuthorizationFailed**: Invalid API key or insufficient permissions

### Medium-Impact Failures
- **DataCorruption**: Retrieved data is malformed
- **ConfigurationError**: Invalid settings
- **SessionTimeout**: Execution took too long

Use in tests to track root causes:

```rust
// In metrics aggregation
for result in test_results {
    if let Some(failure_mode) = &result.failure_mode {
        // Track which failure types occur most often
        failure_modes_histogram[failure_mode] += 1;
    }
}
```

## Running Tests Locally

```bash
# Build eval runner
cargo build --release --bin eval-runner -p tandem-server

# Run with verbose output
./target/release/eval-runner \
  --dataset eval_datasets/critical_path.yaml \
  --simulation \
  --verbose

# Filter to specific tests
./target/release/eval-runner \
  --dataset eval_datasets/provider_failures.yaml \
  --filter-tag transient \
  --num-workers 1

# Check what tests would run without executing
grep "id:" eval_datasets/critical_path.yaml | grep -v "^#"
```

## CI Integration

### PR Workflow

When you push to a PR:

1. `.github/workflows/eval-regression-gate.yml` triggers
2. Builds eval-runner and runs critical_path.yaml
3. Compares results to eval_baselines/main_branch.json
4. If metrics regress >threshold, fails the check
5. Posts results as PR comment

Example PR comment:
```
## 📊 AI Evaluation Results
Pass Rate: 89.5%
Tests: 9/10 passed
See artifacts for detailed results.
```

### Main Branch Updates

On merge to `main`:
- Workflow saves new results as baseline
- Commits eval_baselines/main_branch.json with git metadata
- Baseline becomes the new comparison target for future PRs

## Troubleshooting

### eval-runner binary won't build

Check for ort-sys TLS errors (known environment issue). The CLI still works in simulation mode:

```bash
LIBONNXRUNTIME_STATIC=1 cargo build --release --bin eval-runner
```

Or run with simulation flag (doesn't require AI provider setup):

```bash
./target/release/eval-runner --dataset eval_datasets/critical_path.yaml --simulation
```

### Tests passing locally but failing in CI

Common causes:
1. **Model differences**: Test expects Claude 3 Opus but CI uses Haiku
2. **Provider rate limits**: CI has lower quota than local testing
3. **Nondeterministic AI outputs**: Use simulation mode for stable tests
4. **Dataset version mismatch**: Check eval dataset version in code vs file

### Regression false positive

If a genuine improvement causes regression alert:
1. Review the PR changes
2. Update baseline manually: `cp /tmp/eval_results.json eval_baselines/main_branch.json`
3. Or adjust thresholds for that metric

### High costs per test

Check:
- Model being used: `claude-opus-4-7` > `claude-sonnet-4-6` > `claude-haiku-4-5`
- Token usage: long objectives/contracts = more tokens
- Use `--simulation` to avoid costs during development

## FAQ

**Q: Why simulation mode by default?**
A: Deterministic results enable reliable CI gates. Simulation tests that outputs match contracts without making AI calls. Use `--simulation false` for full integration testing.

**Q: Can I parallelize test execution?**
A: Yes, pass `--num-workers 4` to run 4 tests concurrently. Default is 1 (sequential).

**Q: How do I add a new validator type?**
A: Validators are defined in the automation engine (validation.rs). Add a new validator class, then reference it in test YAML via `validators: ["new_validator"]`.

**Q: Can I use eval_runner in my CI/CD?**
A: Yes! The JSON output and exit codes are designed for CI integration. Example for GitHub Actions:
```yaml
- run: cargo run --bin eval-runner -- --dataset eval_datasets/critical_path.yaml --output results.json
- run: jq '.pass_rate' results.json | awk '{if ($1 < 0.85) exit 1}'
```

**Q: What's the difference between disabled and failed tests?**
A: **Disabled** (`enabled: false`): Not run at all, don't count toward metrics. **Failed** (`artifact_status: blocked`): Ran but didn't pass expected validators, counts as failure in pass_rate.

**Q: How often should I update the baseline?**
A: After merging PRs that intentionally improve AI quality, update baseline. The CI workflow does this automatically on main branch. Manual updates are rare.

## Resources

- **Failure taxonomy reference**: `crates/tandem-server/src/failures/mod.rs`
- **Dataset examples**: `eval_datasets/*.yaml`
- **Metrics internals**: `crates/tandem-server/src/eval/metrics.rs`
- **Regression thresholds**: `crates/tandem-server/src/eval/regression_detection.rs`
- **CLI usage**: `cargo run --bin eval-runner -- --help`
