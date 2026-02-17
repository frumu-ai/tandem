# Tandem Logging Schema (JSONL)

Each JSONL line is one structured tracing event.

## Core fields

- `timestamp` (RFC3339)
- `level` (`TRACE|DEBUG|INFO|WARN|ERROR`)
- `target` (module/target)
- `fields.message` (message)

## Observability fields (target=`tandem.obs`)

- `fields.process` (`engine|desktop|tui`)
- `fields.component`
- `fields.event`
- `fields.correlation_id` (optional)
- `fields.session_id` (optional)
- `fields.run_id` (optional)
- `fields.message_id` (optional)
- `fields.provider_id` (optional)
- `fields.model_id` (optional)
- `fields.status` (optional)
- `fields.error_code` (optional)
- `fields.detail` (optional, redacted-safe text)

## Redaction policy

- Never log API keys, auth headers, raw prompt text, or raw model completion text.
- For sensitive free text, log hashes/lengths or explicit redacted placeholders.

## Example

```json
{
  "timestamp": "2026-02-13T18:22:31.123Z",
  "level": "INFO",
  "target": "tandem.obs",
  "fields": {
    "message": "observability_event",
    "process": "engine",
    "component": "engine.loop",
    "event": "provider.call.start",
    "correlation_id": "2c8...",
    "session_id": "abc...",
    "provider_id": "openrouter",
    "model_id": "google/gemini-2.5-flash",
    "status": "start"
  }
}
```
