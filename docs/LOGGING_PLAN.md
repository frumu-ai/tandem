# Tandem Logging Plan (Implemented Baseline + Next Steps)

## What is implemented now

### Canonical storage

- Logs are written under `%APPDATA%\\tandem\\logs` (canonical shared root).
- New structured files are JSONL with process-specific names:
  - `tandem.engine.YYYY-MM-DD.jsonl`
  - `tandem.desktop.YYYY-MM-DD.jsonl`
  - `tandem.tui.YYYY-MM-DD.jsonl`
- Legacy text logs remain readable for compatibility.

### Shared observability runtime

- New crate: `crates/tandem-observability`
- Features:
  - JSONL logger initialization (`init_process_logging`)
  - startup event emission helpers (`emit_event`)
  - strict text redaction helper (`redact_text`)
  - startup retention cleanup (default 14 days)

### Process wiring

- Engine initializes JSONL logging after CLI parsing/state-dir resolution.
- Desktop (Tauri) initializes JSONL logging at startup.
- TUI initializes JSONL logging at startup.

### Correlation baseline

- Tauri generates correlation IDs on:
  - `send_message_streaming`
  - `index_workspace_command`
- Sidecar HTTP send includes headers:
  - `x-tandem-correlation-id`
  - `x-tandem-session-id`
- Server extracts correlation header and passes it into engine loop context.

### High-value instrumentation

- Engine emits provider lifecycle observability events:
  - `provider.call.start`
  - `provider.call.first_byte`
  - `provider.call.finish`
  - `provider.call.error`
- Stream hub emits stream lifecycle observability events:
  - `stream.subscribe.ok`
  - `stream.subscribe.error`
  - `stream.disconnected`
  - `stream.watchdog.no_events`

### Desktop logs UX baseline

- `LogsDrawer` can parse both classic text lines and JSON log lines.
- Existing filters continue working with JSON-derived level/target/message extraction.

## Remaining work (next milestones)

1. Add full query API (`query_logs`) for structured filtering on backend.
2. Expand Logs drawer filters with time-range presets beyond current-runtime toggle.
3. Add richer memory/index error-code taxonomy in all paths.
4. Add dedicated contract tests for correlation propagation and redaction guarantees.
5. Add dropped-event accounting surfaced in `get_runtime_diagnostics().logging.dropped_events`.
