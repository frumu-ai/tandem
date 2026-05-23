---
title: Start Here
---

Tandem is the authority layer for AI-first work. Choose the path that matches where you want runtime authority to live: local desktop, terminal, headless service, hosted/private deployment, or customer infrastructure.

## Path 1: CLI Binaries

Use this if you want local execution through the master `tandem` CLI, direct `tandem-engine` runtime, and `tandem-tui` from the terminal.

- npm packages:
  - `@frumu/tandem` (master CLI + engine)
  - `@frumu/tandem-tui` (TUI)
- Install: [Install CLI Binaries](./install-cli-binaries/)
- Then: [First Run Checklist](./first-run/)

## Path 2: Web Control Panel

Use this if you want a browser-first control surface for agents, workflows, approvals, channels, memory, and runtime evidence.

- npm add-on for the official ready-to-run panel:
  - `@frumu/tandem-panel`
- Install it through Tandem:
  - `tandem install panel`
- Then bootstrap it:
  - `tandem panel init`
- Legacy compatibility still exists during migration:
  - `tandem-setup`
  - `tandem-control-panel`
- npm scaffold for a fully editable app:
  - `create-tandem-panel`
- Install + run: [Control Panel (Web Admin)](./control-panel/)

## Path 3: Build from Source

Use this if you are contributing, debugging internals, or need custom builds.

- Build guide: [Build from Source](./build-from-source/)
- Developer checks: [Engine Testing](./engine-testing/)

## Canonical Repo and Releases

- Repo: `https://github.com/frumu-ai/tandem`
- Releases: `https://github.com/frumu-ai/tandem/releases`
