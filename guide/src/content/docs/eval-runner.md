---
title: Eval Runner CLI
description: Run Tandem AI evaluation datasets with the eval-runner Cargo binary, including simulation mode, result JSON output, dataset filters, and current stub/live mode caveats.
---

The Tandem eval runner is the command-line entry point for AI quality evaluations and regression checks.

Agent search terms: `eval-runner`, `eval_runner`, `EvalRunner`, `Tandem Eval Runner`, `AI evaluation CLI`, `eval CLI`, `eval_datasets`, `critical_path.yaml`, `EngineMode::Simulation`, `EngineMode::Stub`, `EngineMode::Live`.

Code paths:

- CLI binary: `crates/tandem-eval/src/bin/eval_runner.rs`
- Runner implementation: `crates/tandem-eval/src/runner.rs`
- Dataset parser/types: `crates/tandem-eval/src/dataset.rs`
- Metrics/results: `crates/tandem-eval/src/metrics.rs`
- Regression detection: `crates/tandem-eval/src/regression_detection.rs`
- Datasets: `eval_datasets/*.yaml`
- Internal developer guide: `docs/dev/EVAL_FRAMEWORK.md`

## Quickstart

Run the critical path dataset in deterministic simulation mode:

```bash
cargo run -p tandem-eval --bin eval-runner -- \
  --dataset eval_datasets/critical_path.yaml \
  --engine-mode simulation \
  --output /tmp/tandem-eval-results.json
```

Simulation mode is the safest default for local checks and CI-style regression gates. It does not call an AI provider, does not require an API key, and should be deterministic.

Read the detailed JSON result:

```bash
jq . /tmp/tandem-eval-results.json
```

## Command Reference

```bash
cargo run -p tandem-eval --bin eval-runner -- --help
```

Supported options:

| Option                  | Purpose                                                            |
| ----------------------- | ------------------------------------------------------------------ |
| `--dataset <FILE>`      | Required path to an eval dataset YAML file.                        |
| `--output <FILE>`       | Results JSON path. Defaults to `./eval_results.json`.              |
| `--provider <NAME>`     | Provider name for live mode. Defaults to `anthropic`.              |
| `--model <NAME>`        | Model name for live mode. Defaults to `claude-haiku-4-5-20251001`. |
| `--engine-mode <MODE>`  | `simulation`, `stub`, or `live`. Defaults to `simulation`.         |
| `--simulation`          | Legacy alias for `--engine-mode simulation`.                       |
| `--num-workers <N>`     | Parallel worker count. Defaults to `1`.                            |
| `--filter-tag <TAG>`    | Run only test cases containing the given tag.                      |
| `--max-duration <SECS>` | Maximum duration per test. Defaults to `300`.                      |
| `--verbose`             | Print detailed execution output.                                   |
| `--help`                | Print CLI help.                                                    |

Exit codes:

| Code | Meaning                                  |
| ---- | ---------------------------------------- |
| `0`  | All tests passed.                        |
| `1`  | One or more tests failed.                |
| `2`  | Invalid arguments or dataset load error. |

## Engine Modes

The eval runner accepts three engine modes:

| Mode         | Engine path                                                 | Provider               | API key | Best use                                                   |
| ------------ | ----------------------------------------------------------- | ---------------------- | ------- | ---------------------------------------------------------- |
| `simulation` | No live engine path. Uses deterministic simulated outcomes. | None                   | No      | Fast local checks, per-PR quality gates, docs examples.    |
| `stub`       | Real Tandem engine path with scripted responses.            | `ScriptedEvalProvider` | No      | Engine-path validation and zero-cost baseline captures.    |
| `live`       | Real Tandem engine path with configured provider.           | Real provider          | Yes     | Human-run baseline captures and release confidence checks. |

Mode names are parsed case-insensitively. Synonyms include `sim` for `simulation`, `scripted` for `stub`, and `real` for `live`.

## Local Engine Bootstrap

When `--engine-mode stub` or `--engine-mode live` is used without `--engine-token`, the CLI bootstraps an isolated in-process `AppState`. Stub mode swaps in `ScriptedEvalProvider`; live mode uses configured providers. Passing `--engine-token` uses the remote engine path instead.

## Common Commands

Run every enabled test in the critical path dataset:

```bash
cargo run -p tandem-eval --bin eval-runner -- \
  --dataset eval_datasets/critical_path.yaml
```

Write results to a stable file:

```bash
cargo run -p tandem-eval --bin eval-runner -- \
  --dataset eval_datasets/critical_path.yaml \
  --output eval_results.json
```

Run only tests tagged `regression`:

```bash
cargo run -p tandem-eval --bin eval-runner -- \
  --dataset eval_datasets/critical_path.yaml \
  --filter-tag regression \
  --engine-mode simulation
```

Run with verbose output:

```bash
cargo run -p tandem-eval --bin eval-runner -- \
  --dataset eval_datasets/critical_path.yaml \
  --engine-mode simulation \
  --verbose
```

## Dataset Shape

Eval datasets live under `eval_datasets/` and are YAML files with a dataset header plus `test_cases`.

Important fields for agents editing or generating datasets:

| Field                          | Meaning                                                |
| ------------------------------ | ------------------------------------------------------ |
| `name`                         | Dataset name.                                          |
| `version`                      | Dataset schema/content version.                        |
| `description`                  | Human-readable purpose.                                |
| `tags`                         | Dataset-level labels.                                  |
| `test_cases[].id`              | Stable unique test case id.                            |
| `test_cases[].enabled`         | Whether the case runs by default.                      |
| `test_cases[].tags`            | Labels used by `--filter-tag`.                         |
| `test_cases[].automation_spec` | Workflow-like spec under evaluation.                   |
| `test_cases[].expected_output` | Required status, validators, output shape, and limits. |

Use `eval_datasets/critical_path.yaml` as the reference dataset before creating a new file.

## Result JSON

The runner writes a JSON result file containing aggregate metrics and per-test results. Use it for:

- pass-rate checks
- failure-mode review
- regression comparison
- release notes and quality evidence
- debugging individual failed eval cases

The stdout summary is useful for humans, but agents should prefer the JSON file when making decisions.

## When To Use This CLI

Use `eval-runner` when you are checking Tandem AI behavior across a versioned eval dataset.

Use regular Rust tests instead when you are validating a specific function, parser, HTTP handler, or engine unit. See [Engine Testing](./engine-testing/) for the broader test matrix.
