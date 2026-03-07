# tandem-tui

```text
TTTTT   A   N   N DDDD  EEEEE M   M
  T    A A  NN  N D   D E     MM MM
  T   AAAAA N N N D   D EEEE  M M M
  T   A   A N  NN D   D E     M   M
  T   A   A N   N DDDD  EEEEE M   M
```

## What This Is

`tandem-tui` is the Rust crate for the terminal client binary.  
It connects to `tandem-engine` and provides a keyboard-first chat + agent workflow UI in the terminal.

Coding workflow helpers:

- `Alt+P` / `/files [query]`: fuzzy file search and `@path` insertion
- `Alt+D` / `/diff`: structured git diff pager overlay
- `Alt+E` / `/edit`: edit current draft in `$VISUAL`/`$EDITOR`

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
