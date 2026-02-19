# tandem-ai (tandem-engine)

```text
TTTTT   A   N   N DDDD  EEEEE M   M
  T    A A  NN  N D   D E     MM MM
  T   AAAAA N N N D   D EEEE  M M M
  T   A   A N  NN D   D E     M   M
  T   A   A N   N DDDD  EEEEE M   M
```

## What This Is

`tandem-ai` is the Rust crate that builds the `tandem-engine` binary.  
It runs the headless Tandem runtime (HTTP + SSE APIs, tools, sessions, orchestration/agent workflows).

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

Disable memory embeddings for a server run:

```bash
cargo run -p tandem-ai -- serve --disable-embeddings
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
