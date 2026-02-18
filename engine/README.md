# tandem-ai (tandem-engine)

Rust crate for the Tandem engine service and CLI binary (`tandem-engine`).

## Build

From the workspace root:

```bash
cargo build -p tandem-ai
```

## Run

Start the HTTP/SSE engine server:

```bash
cargo run -p tandem-ai -- serve --hostname 127.0.0.1 --port 39731
```

Run a one-off prompt:

```bash
cargo run -p tandem-ai -- run "What is the capital of France?"
```

List available providers:

```bash
cargo run -p tandem-ai -- providers
```

## Verify Before Publishing

```bash
cargo check -p tandem-ai
cargo package -p tandem-ai
```

## Related Packages

- npm wrapper (prebuilt binaries): `packages/tandem-engine`
- TUI crate: `crates/tandem-tui`

## Documentation

- Project docs: https://tandem.frumu.ai/docs
- GitHub releases: https://github.com/frumu-ai/tandem/releases
