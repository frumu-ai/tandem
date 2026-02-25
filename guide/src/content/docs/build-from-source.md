---
title: Build from Source
---

Use this path for contributors and advanced local development.

## Prerequisites

- Rust (stable)
- Node.js 20+
- `pnpm`

Platform-specific dependencies are listed in the repository README.

## 1. Clone

```bash
git clone https://github.com/frumu-ai/tandem.git
cd tandem
```

## 2. Install JS dependencies

```bash
pnpm install
```

## 3. Build engine binary

```bash
cargo build -p tandem-ai
```

This produces the `tandem-engine` binary from the `tandem-ai` package.

## 4. Run

```bash
cargo run -p tandem-ai -- serve --host 127.0.0.1 --port 39731
```

In another terminal:

```bash
cargo run -p tandem-tui
```

## 5. Development and testing references

- [Engine Testing](./engine-testing/)
- `docs/ENGINE_TESTING.md`
- `docs/ENGINE_CLI.md`

## 6. Build docs (Starlight)

From `guide/`:

```bash
pnpm install
DOCS_SITE_URL=https://tandem.docs.frumu.ai/ DOCS_BASE_PATH=/ pnpm build
```

Notes:

- Root-hosted docs (`https://tandem.docs.frumu.ai/`) should use `DOCS_BASE_PATH=/`.
- Reverse-proxy docs at subpath (`https://tandem.frumu.ai/docs/`) should use:

```bash
DOCS_SITE_URL=https://tandem.frumu.ai/ DOCS_BASE_PATH=/docs/ pnpm build
```

- Whatever base path you build with, your proxy/static host must serve both:
  - `<base>_astro/*`
  - `<base>pagefind/*`
