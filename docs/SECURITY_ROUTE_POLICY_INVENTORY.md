# Security route-policy inventory

`SECURITY_ROUTE_POLICY_INVENTORY.json` is the review contract for Tandem's HTTP
surface. It records every discovered listener, exact path, and effective method,
including Axum's implicit `HEAD` dispatch for every `GET` handler. Each entry
declares both the ingress policy and the application authorization policy, plus
the required capability, server-side resource resolver, policy origin, and
source registration.

The inventory covers:

- all engine API route registrar modules;
- enterprise route extensions composed into the engine router;
- the configured Web UI GET/HEAD surface; and
- the separate loopback OpenAI Codex OAuth callback listener.

It deliberately excludes `#[cfg(test)]` fixture routers, outbound provider
URLs, and the feature-gated loopback ACME demo's deterministic Slack mock. Those
exclusions are recorded in the generated JSON and the verifier fails if another
production Axum registrar appears outside the known set.

## Verification and drift

Run:

```bash
node --test scripts/verify-route-policy-inventory.test.mjs
node scripts/verify-route-policy-inventory.mjs --self-test
node scripts/verify-route-policy-inventory.mjs
```

The final command regenerates the inventory in memory and byte-compares it with
the committed file. Any route, method, source, listener, or policy drift fails
CI. To intentionally update it:

```bash
node scripts/verify-route-policy-inventory.mjs --write
git diff -- docs/SECURITY_ROUTE_POLICY_INVENTORY.json
```

Review every changed entry. The generated policy is an explicit expected
contract and drift detector; it does not, by itself, prove the handler enforces
the contract. Enforcement evidence comes from the centralized request/context
gate, handler/state authorization boundaries, and the security retest matrix.

## Middleware-only methods

The engine auth gate permits `OPTIONS` so the CORS layer can answer preflight.
This is recorded under `scope.middleware_surfaces`; it is not an application
handler method. Unknown methods and paths still reach Axum's normal method/path
rejection and do not inherit an application route policy.
