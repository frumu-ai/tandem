---
title: HTTP Error Codes
description: Stable Tandem engine HTTP error codes and retry semantics for agents and SDK clients.
---

Migrated Tandem engine HTTP APIs return this error envelope:

```json
{
  "error": "Human-readable message",
  "code": "SESSION_NOT_FOUND",
  "retryable": false
}
```

Branch on `code`, not on the human-readable `error` string. Treat `retryable` as a hint that the same request may succeed later without changing the payload.

| Code                             | Retryable | Meaning                                                      |
| -------------------------------- | --------- | ------------------------------------------------------------ |
| `AUTH_REQUIRED`                  | no        | Missing or invalid engine API token.                         |
| `TENANT_CONTEXT_DENIED`          | no        | Tenant context assertion or request principal was rejected.  |
| `TENANT_SCOPE_DENIED`            | no        | Requested resource is outside the caller's tenant scope.     |
| `VALIDATION_FAILED`              | no        | Request body or parameters failed validation.                |
| `SESSION_NOT_FOUND`              | no        | Session does not exist or is not visible to the caller.      |
| `SESSION_RUN_CONFLICT`           | yes       | Session already has an active run.                           |
| `RATE_LIMITED`                   | yes       | Request was throttled; honor `Retry-After` when present.     |
| `PROMPT_TIMEOUT`                 | yes       | Prompt run did not finish before the HTTP timeout.           |
| `ENGINE_STARTING`                | yes       | Engine startup is still in progress.                         |
| `ENGINE_STARTUP_FAILED`          | no        | Engine startup failed and requires operator action.          |
| `APPROVAL_REPLY_INVALID`         | no        | Approval or permission reply value is invalid.               |
| `APPROVAL_REQUEST_NOT_FOUND`     | no        | Approval, question, or permission request was not found.     |
| `APPROVAL_PERSISTENCE_FAILED`    | yes       | Approval state could not be persisted or loaded.             |
| `MCP_REQUEST_DENIED`             | no        | MCP registration or request was denied.                      |
| `MCP_STDIO_TRANSPORT_DENIED`     | no        | Stdio MCP transport registration was attempted through HTTP. |
| `MCP_REFRESH_FAILED`             | yes       | MCP refresh or reconnect failed.                             |
| `MCP_OAUTH_FAILED`               | no        | MCP OAuth flow failed and likely needs user action.          |
| `SKILLS_ERROR`                   | no        | Skill or memory API request failed.                          |
| `OPTIMIZATION_VALIDATION_FAILED` | no        | Optimization API request failed validation.                  |
| `OPTIMIZATION_NOT_FOUND`         | no        | Optimization resource was not found.                         |
| `OPTIMIZATION_CONFLICT`          | no        | Optimization action conflicts with current state.            |
| `PERSISTENCE_FAILED`             | yes       | Storage operation failed.                                    |
| `INTERNAL_ERROR`                 | yes       | Unexpected server-side failure.                              |

Some older routes still return bare HTTP status codes while the API is migrated. SDKs should use the structured envelope when present and keep status-code fallback handling.
