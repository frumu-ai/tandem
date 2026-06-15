# HTTP Error Codes

Tandem HTTP APIs return a stable error envelope for new and migrated routes:

```json
{
  "error": "Human-readable message",
  "code": "SESSION_NOT_FOUND",
  "retryable": false
}
```

`error` is diagnostic text. Clients should branch on `code` and use `retryable` as a hint for safe automatic retry. A `retryable: true` response means the same request may succeed later without changing the request payload; it does not guarantee retry success.

## Codes

| Code                             | Retryable | Category             | Meaning                                                                     |
| -------------------------------- | --------- | -------------------- | --------------------------------------------------------------------------- |
| `AUTH_REQUIRED`                  | no        | auth                 | Missing or invalid engine API token.                                        |
| `TENANT_CONTEXT_DENIED`          | no        | tenant               | Tenant context assertion or request principal was rejected.                 |
| `TENANT_SCOPE_DENIED`            | no        | tenant               | The requested resource is outside the caller's tenant scope.                |
| `VALIDATION_FAILED`              | no        | validation           | Request body or parameters failed validation.                               |
| `SESSION_NOT_FOUND`              | no        | sessions             | Session does not exist or is not visible to the caller.                     |
| `SESSION_RUN_CONFLICT`           | yes       | sessions             | Session already has an active run; attach to the active run or retry later. |
| `RATE_LIMITED`                   | yes       | policy               | Request was throttled; honor `Retry-After` when present.                    |
| `PROMPT_TIMEOUT`                 | yes       | provider/tool        | Prompt run did not finish before the HTTP timeout.                          |
| `ENGINE_STARTING`                | yes       | runtime              | Engine startup is still in progress.                                        |
| `ENGINE_STARTUP_FAILED`          | no        | runtime              | Engine startup failed and requires operator action.                         |
| `APPROVAL_REPLY_INVALID`         | no        | approval             | Approval or permission reply value is invalid.                              |
| `APPROVAL_REQUEST_NOT_FOUND`     | no        | approval             | Approval, question, or permission request was not found.                    |
| `APPROVAL_PERSISTENCE_FAILED`    | yes       | approval/persistence | Approval state could not be persisted or loaded.                            |
| `MCP_REQUEST_DENIED`             | no        | mcp                  | MCP registration or request was denied.                                     |
| `MCP_STDIO_TRANSPORT_DENIED`     | no        | mcp                  | Stdio MCP transport registration was attempted through HTTP.                |
| `MCP_REFRESH_FAILED`             | yes       | mcp                  | MCP refresh or reconnect failed.                                            |
| `MCP_OAUTH_FAILED`               | no        | mcp                  | MCP OAuth flow failed and likely needs user action.                         |
| `SKILLS_ERROR`                   | no        | skills/memory        | Skill or memory API request failed.                                         |
| `OPTIMIZATION_VALIDATION_FAILED` | no        | optimization         | Optimization API request failed validation.                                 |
| `OPTIMIZATION_NOT_FOUND`         | no        | optimization         | Optimization resource was not found.                                        |
| `OPTIMIZATION_CONFLICT`          | no        | optimization         | Optimization action conflicts with current state.                           |
| `PERSISTENCE_FAILED`             | yes       | persistence          | Storage operation failed.                                                   |
| `INTERNAL_ERROR`                 | yes       | internal             | Unexpected server-side failure.                                             |

## Migration Notes

TAN-201 introduces the enum in `tandem-wire` and mirrors it in the TypeScript and Python clients. Migrated high-traffic paths include auth/tenant middleware, prompt session execution, approval/permission replies, MCP registration and refresh responses, skill errors, and optimization errors.

Older routes may still return bare status codes or legacy ad-hoc JSON while they are migrated. Clients should continue to handle transport status as a fallback.
