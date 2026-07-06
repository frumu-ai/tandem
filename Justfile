demo:
    node examples/email-approval-demo/demo.mjs

email-approval-demo:
    node examples/email-approval-demo/demo.mjs

email-approval-demo-ci:
    node examples/email-approval-demo/demo.mjs --non-interactive --skip-build

# Full Rust test suite the way CI runs it: nextest (one process per test) with
# an isolated TANDEM_HOME so no test can touch the real user data dir or
# collide with another test process in shared canonical paths (TAN-619).
test-rust:
    #!/usr/bin/env bash
    set -euo pipefail
    export TANDEM_HOME="$(mktemp -d)/tandem-home"
    cargo nextest run -p tandem-server

test-rust-workspace:
    #!/usr/bin/env bash
    set -euo pipefail
    export TANDEM_HOME="$(mktemp -d)/tandem-home"
    cargo nextest run --workspace --exclude tandem
