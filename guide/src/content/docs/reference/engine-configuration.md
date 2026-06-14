---
title: Engine Configuration
description: Startup validation and environment variables for tandem-engine.
---

# Engine Configuration

Run `tandem-engine config check` before self-hosted or enterprise deployments to print the masked effective configuration and fail on invalid settings. Use `tandem-engine config check --json` in CI.

## Security Invariants

- Hosted or enterprise auth mode requires a context assertion verifier keyring.
- Hosted or enterprise auth mode requires an explicit transport token from `TANDEM_API_TOKEN`, `TANDEM_API_TOKEN_FILE`, or `--api-token`.
- Hosted or enterprise auth mode rejects `TANDEM_UNSAFE_NO_API_TOKEN`.
- Malformed verifier key material, invalid booleans, invalid modes, and out-of-range numeric settings fail fast.
- Unknown `TANDEM_*` variables are reported as warnings to catch typos without blocking local startup.

## Reference

The Markdown table in `docs/ENGINE_CONFIGURATION.md` is generated from the same registry used by `tandem-engine config reference`.
