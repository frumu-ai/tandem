---
title: Install CLI Binaries
---

Install prebuilt `tandem-engine` and `tandem-tui` binaries without compiling.

## 1. Install from npm (recommended)

Install both wrappers globally:

```bash
npm install -g @frumu/tandem @frumu/tandem-tui
```

Commands provided:

- `tandem-engine` (from `@frumu/tandem`)
- `tandem-tui` (from `@frumu/tandem-tui`)

Update behavior:

- On startup, each npm wrapper performs a short non-blocking check against npm latest.
- If a newer version exists, it prints an update notice and install command.

## 2. Download Release Assets (manual fallback)

Open: `https://github.com/frumu-ai/tandem/releases`

Download the archive for your OS/architecture and extract it.

Expected binaries:

- `tandem-engine`
- `tandem-tui`

## 3. Add to PATH

Add the extracted directory to your `PATH` so both commands are globally available.

## 4. Verify

```bash
tandem-engine --help
tandem-tui --help
```

## 5. Start Engine + TUI

```bash
tandem-engine serve --hostname 127.0.0.1 --port 39731
```

In a second terminal:

```bash
tandem-tui
```

Then complete [First Run Checklist](./first-run/).
