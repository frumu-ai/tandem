# Observability Export Boundaries

Tandem observability is tenant tagged but content scrubbed. Exported metrics and
optional error events carry IDs, status labels, and error codes; they must not
carry raw prompts, model completions, tool arguments, auth headers, OAuth
tokens, or connector secrets.

## Tenant Tags

Structured observability events include these tenant identifiers when the caller
has an explicit tenant context:

| Field          | Meaning                    | Content Rule |
| -------------- | -------------------------- | ------------ |
| `org_id`       | Organization identifier    | ID only      |
| `workspace_id` | Workspace identifier       | ID only      |
| `session_id`   | Runtime session identifier | ID only      |
| `run_id`       | Runtime run identifier     | ID only      |
| `message_id`   | Runtime message identifier | ID only      |

`detail` remains local JSONL-only and must already be redacted before logging.
The scrubbed export payload omits `detail` entirely.

## Prometheus Metrics

The Prometheus endpoint is off by default. Enable it with:

```bash
TANDEM_OBSERVABILITY_PROMETHEUS_ENABLED=true
```

The endpoint is mounted at `GET /metrics` and remains behind the normal server
transport-auth middleware. It returns `404 Not Found` while disabled.

Current instruments:

| Metric                                           | Labels                      | Meaning                                                 |
| ------------------------------------------------ | --------------------------- | ------------------------------------------------------- |
| `tandem_scheduler_active_runs`                   | none                        | Automation scheduler active run count                   |
| `tandem_scheduler_queued_runs`                   | `reason`                    | Queued Automation V2 runs by scheduler reason           |
| `tandem_scheduler_admitted_total`                | none                        | Scheduler admission counter                             |
| `tandem_scheduler_completed_total`               | none                        | Scheduler completion counter                            |
| `tandem_scheduler_queue_wait_ms_avg`             | none                        | Rolling average scheduler queue wait                    |
| `tandem_scheduler_queue_wait_ms_p95`             | none                        | Rolling p95 scheduler queue wait                        |
| `tandem_scheduler_tick_latency_ms_count/sum/max` | none                        | Scheduler tick latency summary                          |
| `tandem_run_duration_ms_count/sum/max`           | `status`                    | Session run duration summary                            |
| `tandem_gate_wait_ms_count/sum/max`              | `decision`                  | Permission/approval wait summary                        |
| `tandem_tool_call_decisions_total`               | `decision`                  | Tool gate decisions (`allow`, `ask`, `deny`, `unknown`) |
| `tandem_provider_errors_total`                   | `provider_id`, `error_code` | Provider error counter                                  |

Metric labels are sanitized and bounded to ID/code-like strings.

## Sentry Export

Sentry export is compile-time feature gated and off in default builds:

```bash
cargo build -p tandem-observability --features sentry
```

The feature installs panic capture through the Sentry SDK and exposes
`init_sentry_export` plus `capture_sentry_error_event` for error-level
observability events. The `before_send` scrubber rebuilds outbound events with
only these fields:

| Field               | Source                       |
| ------------------- | ---------------------------- |
| `logger`            | Fixed `tandem.observability` |
| `level`             | Error                        |
| `message`           | Fixed scrubbed message       |
| `tags.process`      | Process kind                 |
| `tags.event`        | Observability event name     |
| `tags.component`    | Component name               |
| `tags.org_id`       | Organization ID              |
| `tags.workspace_id` | Workspace ID                 |
| `tags.error_code`   | Error code                   |
| `tags.status`       | Status, when supplied        |

The scrubber drops breadcrumbs, request bodies, user objects, exception text,
extra maps, raw details, prompts, completions, and tool argument payloads.
