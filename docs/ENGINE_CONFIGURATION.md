# Tandem Engine Configuration Reference

This page is generated from the engine config registry used by `tandem-engine config check`.

| Variable                                      | Default               | Notes                                                                                                               |
| --------------------------------------------- | --------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `TANDEM_RUNTIME_AUTH_MODE`                    | `local_single_tenant` | Runtime trust mode: local_single_tenant, hosted_single_tenant, or enterprise_required.                              |
| `TANDEM_DATA_BOUNDARY_MODE`                   | `off`                 | Data boundary evaluation at every production LLM-provider dispatch: off, audit, or enforce. Enforce can block, transform, or require approval. |
| `TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY`    | `block`               | Treatment of raw sensitive data headed to unapproved external providers: allow, audit, redact, approval, require_local, or block. |
| `TANDEM_DATA_BOUNDARY_MAX_PAYLOAD_BYTES`      | `unset`               | Optional provider-payload byte cap; blocks oversized dispatches in enforce mode.                                    |
| `TANDEM_DATA_BOUNDARY_APPROVAL_CLASSES`       | `unset`               | Comma-separated sensitive data classes requiring approval (e.g. credential,customer_data).                          |
| `TANDEM_DATA_BOUNDARY_REDACT_CLASSES`         | `unset`               | Comma-separated sensitive data classes to redact before external dispatch.                                          |
| `TANDEM_DATA_BOUNDARY_BLOCK_CLASSES`          | `unset`               | Comma-separated sensitive data classes that must never leave for a provider.                                        |
| `TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES`       | `unset`               | Comma-separated provider_id=boundary_class mappings (e.g. openai=approved_external, ollama=local). All unmapped providers classify as unknown - builtin loopback ids get no id-based trust because their base URLs can be reconfigured to remote endpoints. |
| `TANDEM_DATA_BOUNDARY_STRICT`                 | `false`               | Strict enterprise posture: enforce mode fails closed on missing tenant/run/session authority or unknown provider classification. A local-implicit tenant counts as missing - tenancy must be positively established. |
| `TANDEM_API_TOKEN`                            | `unset`               | Explicit HTTP transport bearer token. Secret value is never printed by config check.                                |
| `TANDEM_API_TOKEN_FILE`                       | `unset`               | File containing the HTTP transport bearer token. Required in hosted/enterprise mode unless --api-token is supplied. |
| `TANDEM_UNSAFE_NO_API_TOKEN`                  | `false`               | Local loopback development only; rejected in hosted/enterprise mode.                                                |
| `TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS`        | `unset`               | JSON Ed25519 verifier keyring with explicit purpose and status metadata. Required in hosted/enterprise mode.        |
| `TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS_FILE`   | `unset`               | Secure metadata-keyring file; hosted Unix deployments require owner-only permissions.                               |
| `TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY`         | `unset`               | Legacy single verifier key for local migration only; rejected in hosted/enterprise mode.                            |
| `TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY_FILE`    | `unset`               | Legacy single verifier key file for local migration only; rejected in hosted/enterprise mode.                       |
| `TANDEM_CONTEXT_ASSERTION_ISSUER`             | `tandem-web`          | Expected context assertion issuer.                                                                                  |
| `TANDEM_CONTEXT_ASSERTION_AUDIENCE`           | `tandem-runtime`      | Expected context assertion audience.                                                                                |
| `TANDEM_CONTEXT_ASSERTION_REPLAY_MODE`        | `bound`               | Replay handling: bound, one_shot, or off. Off is rejected in hosted/enterprise mode.                                |
| `TANDEM_CONTEXT_ASSERTION_REPLAY_STORE_FILE`  | `unset`               | Shared durable transactional SQLite replay database. Required and opened before bind in hosted/enterprise mode; Unix files require owner-only permissions and reject symlinks.                        |
| `TANDEM_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS` | `10000`               | Allowed future clock skew for assertions; valid range 10000..=60000.                                                |
| `TANDEM_CONTEXT_ASSERTION_MAX_LIFETIME_MS`    | `900000`              | Maximum assertion lifetime; valid range 1..=3600000 and enforced with checked arithmetic.                           |
| `TANDEM_HOSTED_CONTROL_PLANE_URL`             | `unset`               | Hosted control-plane URL; enables enterprise-scoped memory policy.                                                  |
| `TANDEM_ENTERPRISE_CONTROL_PLANE_URL`         | `unset`               | Enterprise control-plane URL alias.                                                                                 |
| `TANDEM_CROSS_TENANT_GRANT_SIGNING_KEY`       | `unset`               | Secret signing key for cross-tenant grants.                                                                         |
| `TANDEM_CROSS_TENANT_GRANT_SIGNING_KEY_FILE`  | `unset`               | File containing the cross-tenant grant signing key.                                                                 |
| `TANDEM_AUDIT_HMAC_KEY`                       | `unset`               | Deployment secret used for privacy-preserving predicate evidence and exact-action approval bindings. Required when those authored policies execute in hosted/enterprise mode. |
| `TANDEM_AUDIT_HMAC_KEY_FILE`                  | `unset`               | File containing the deployment policy-evidence audit HMAC key.                                                      |
| `TANDEM_RUN_STALE_MS`                         | `120000`              | Run staleness threshold; valid range 30000..=600000.                                                                |
| `TANDEM_TOKEN_COST_PER_1K_USD`                | `0.0`                 | Non-negative token cost used for estimates.                                                                         |
| `TANDEM_AUTOMATION_STRICT_RESEARCH_QUALITY`   | `true`                | Enable strict automation research quality checks.                                                                   |
| `TANDEM_AUTOMATION_QUALITY_LEGACY_ROLLBACK`   | `false`               | Enable legacy rollback behavior for automation quality checks.                                                      |
| `TANDEM_AUTOMATION_EXECUTE_NODE_TIMEOUT_MS`   | `1800000`             | Automation node timeout; valid range 180000..=3600000.                                                              |
| `TANDEM_SCHEDULER_MODE`                       | `multi`               | Scheduler mode: single or multi.                                                                                    |
| `TANDEM_SCHEDULER_MAX_CONCURRENT_RUNS`        | `8`                   | Positive maximum concurrent scheduler runs.                                                                         |
| `TANDEM_SCHEDULER_SHUTDOWN_TIMEOUT_SECS`      | `30`                  | Positive scheduler shutdown timeout.                                                                                |
| `TANDEM_STATE_DIR`                            | `shared path`         | Engine state directory.                                                                                             |
| `TANDEM_STORAGE_DIR`                          | `state dir`           | Storage directory override.                                                                                         |
| `TANDEM_STORAGE_BACKEND`                      | `sqlite`              | Stateful store backend: sqlite or postgres. Fail-closed on invalid values.                                         |
| `TANDEM_STORAGE_POSTGRES_URL`                 | `unset`               | PostgreSQL connection URL; required when TANDEM_STORAGE_BACKEND=postgres.                                          |
| `TANDEM_ENGINE_HOST`                          | `127.0.0.1`           | Default engine bind host for CLI commands.                                                                          |
| `TANDEM_ENGINE_PORT`                          | `39731`               | Default engine bind port for CLI commands.                                                                          |
| `TANDEM_DISABLE_EMBEDDINGS`                   | `false`               | Disable semantic memory embeddings.                                                                                 |
| `TANDEM_WEB_UI`                               | `false`               | Enable embedded web admin UI.                                                                                       |
| `TANDEM_WEB_UI_PREFIX`                        | `/admin`              | Embedded web admin UI path prefix.                                                                                  |

`tandem-engine config check` validates these startup invariants before the server binds:

- Hosted or enterprise auth mode requires a metadata-bearing context assertion verifier keyring; legacy single-key mode is rejected.
- Hosted or enterprise auth mode requires `TANDEM_CONTEXT_ASSERTION_REPLAY_STORE_FILE` and rejects replay mode `off`.
- Assertion lifetime is bounded to a 15-minute default and one-hour hard ceiling.
- Hosted or enterprise auth mode requires an explicit transport token from `TANDEM_API_TOKEN`, `TANDEM_API_TOKEN_FILE`, or `--api-token`.
- Hosted or enterprise auth mode rejects `TANDEM_UNSAFE_NO_API_TOKEN`.
- Malformed verifier key material, invalid booleans, invalid modes, and out-of-range numeric settings fail fast.
- Unknown `TANDEM_*` variables are reported as warnings to catch typos without blocking local startup.

Predicate-governed decisions and enterprise-authored exact-action approvals additionally fail closed at decision time in hosted/enterprise mode unless `TANDEM_AUDIT_HMAC_KEY` or `TANDEM_AUDIT_HMAC_KEY_FILE` is configured.
