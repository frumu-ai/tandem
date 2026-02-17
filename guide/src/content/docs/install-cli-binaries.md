---
title: Install CLI Binaries
---

Install prebuilt `tandem-engine` and `tandem-tui` binaries without compiling.

## 1. Download Release Assets

Open: `https://github.com/frumu-ai/tandem/releases`

Download the archive for your OS/architecture and extract it.

Expected binaries:

- `tandem-engine`
- `tandem-tui`

## 2. Add to PATH

Add the extracted directory to your `PATH` so both commands are globally available.

## 3. Verify

```bash
tandem-engine --help
tandem-tui --help
```

## 4. Start Engine + TUI

```bash
tandem-engine serve --hostname 127.0.0.1 --port 39731
```

In a second terminal:

```bash
tandem-tui
```

Then complete [First Run Checklist](./first-run/).
