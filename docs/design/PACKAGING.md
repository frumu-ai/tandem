# Packaging Plan

## NPM Packages

- `@frumu/tandem` (desktop distribution wrapper/meta package)
- `@frumu/tandem-tui` (CLI/TUI distribution)
- Optional per-OS binary packages via `optionalDependencies`.

## Cargo Naming

- Keep coherent crate names without scope support:
  - `tandem-engine`
  - `tandem-server`
  - `tandem-core`
  - `tandem-orchestrator` (new)

## Third-Party Builder Promise

External clients should be able to integrate via stable HTTP/SSE contracts:

- Run/session lifecycle APIs
- Orchestrator APIs
- Resource APIs
- Mission and routine event streams
