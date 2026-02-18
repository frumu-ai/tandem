# tandem-tui

Rust crate for the Tandem terminal UI binary (`tandem-tui`).

## Build

From the workspace root:

```bash
cargo build -p tandem-tui
```

## Run

Start the engine in one terminal:

```bash
cargo run -p tandem-ai -- serve --hostname 127.0.0.1 --port 39731
```

Start the TUI in another terminal:

```bash
cargo run -p tandem-tui
```

## Verify Before Publishing

```bash
cargo check -p tandem-tui
cargo package -p tandem-tui
```

## Related Packages

- npm wrapper (prebuilt binaries): `packages/tandem-tui`
- Engine crate: `engine` (`tandem-ai`)

## Documentation

- Project docs: https://tandem.frumu.ai/docs
- GitHub releases: https://github.com/frumu-ai/tandem/releases
