# Email Approval Demo

This example packages a credential-free approval-gated email flow:

1. Start a local HTTP MCP stub that exposes `email.draft` and `email.send`.
2. Register the stub as the `email_demo` MCP server.
3. Draft a seeded email through the bridge tool `mcp.email_demo.email_draft`.
4. Pause an Automation V2 run on a human approval gate.
5. Approve, cancel, or rework the gate.
6. Send only after approval through the bridge tool `mcp.email_demo.email_send`.
7. Print and write proof artifacts: gate history, tool-dispatch ledger events,
   and the stub outbox.

Run from the repository root:

```bash
just demo
```

For CI or unattended checks:

```bash
just email-approval-demo-ci
```

The non-interactive mode auto-approves and exits with a non-zero status if the
gate, MCP tool dispatch, or outbox evidence is missing. Artifacts are written
under `.tmp/email-approval-demo/artifacts/`.

Useful variants:

```bash
node examples/email-approval-demo/demo.mjs --decision cancel
node examples/email-approval-demo/demo.mjs --decision rework-then-approve
```

The MCP stub is HTTP JSON-RPC because Tandem currently registers stdio MCP
servers only outside the HTTP API, while runtime `tools/call` dispatch requires
an HTTP/S MCP transport.
