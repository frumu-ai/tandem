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

Enable cross-project global memory tools (opt-in):

```bash
TANDEM_ENABLE_GLOBAL_MEMORY=1 cargo run -p tandem-ai -- serve
```

Standard installs should set only `TANDEM_STATE_DIR` and keep all Tandem
runtime files under that one root. The engine will then use:

- `<TANDEM_STATE_DIR>/memory.sqlite`
- `<TANDEM_STATE_DIR>/config.json`
- `<TANDEM_STATE_DIR>/storage/...`
- `<TANDEM_STATE_DIR>/logs/...`

`TANDEM_MEMORY_DB_PATH` remains available as an advanced override, but using a
separate memory DB path from the main Tandem state root is no longer the
recommended setup.

The engine no longer bootstraps docs knowledge into memory on startup. Tandem
specific guidance should come from the built-in `tandem-mcp` docs server or the
published docs site.

If you're cleaning an older install that still has the legacy `guide_docs:`
rows in memory, run:

```bash
tandem-engine storage cleanup --default-knowledge --quarantine --json
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

- Project docs: https://docs.tandem.ac/
- GitHub releases: https://github.com/frumu-ai/tandem/releases
