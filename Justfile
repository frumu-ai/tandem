demo:
    node examples/email-approval-demo/demo.mjs

email-approval-demo:
    node examples/email-approval-demo/demo.mjs

email-approval-demo-ci:
    node examples/email-approval-demo/demo.mjs --non-interactive --skip-build

# Full Rust test suite the way CI runs it (engine-ci.yml "Workspace Tests"):
# nextest (one process per test), the ci profile (skips the TAN-220 quarantine
# of environment-sensitive tests, no fail-fast, flaky tests reported as FLAKY),
# and the same feature set. TANDEM_HOME points at a throwaway dir so no test
# can touch the real user data dir (TAN-619). RUST_MIN_STACK matches
# engine-ci.yml: debug builds of the deepest coder/task-runtime futures
# overflow the default 2 MiB test-thread stack.
test-rust:
    #!/usr/bin/env bash
    set -euo pipefail
    export TANDEM_HOME="$(mktemp -d)/tandem-home"
    export RUST_MIN_STACK=16777216
    cargo nextest run -p tandem-server --features premium-governance --profile ci

test-rust-workspace:
    #!/usr/bin/env bash
    set -euo pipefail
    export TANDEM_HOME="$(mktemp -d)/tandem-home"
    export RUST_MIN_STACK=16777216
    cargo nextest run --workspace --exclude tandem \
        --features tandem-ai/browser,tandem-server/premium-governance \
        --profile ci
