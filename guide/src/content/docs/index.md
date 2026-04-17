---
title: Tandem Engine Guide
description: Welcome to the Tandem documentation hub. Choose your path based on how you intend to use Tandem.
template: doc
---

Welcome to the **Tandem Engine Documentation**. Tandem is an **engine-owned workflow runtime** for coordinated autonomous work, built to scale beyond the limitations of chat-first AI assistants and operate in multiple highly-concurrent configurations.

To help you find what you need quickly, please select the path that best describes how you plan to use Tandem:

---

## 🖥️ I am a Desktop User

_You want to run the native Tandem desktop app or terminal UI to assist you with local file tasks, writing, coding, or managing agents._

- **[TUI Guide](./tui-guide/)** — Learn how to navigate the Terminal UI.
- **[Control Panel (Web Admin)](./control-panel/)** — Run the official browser UI or scaffold an editable panel app.
- **[How Tandem Works Under the Hood](./how-tandem-works/)** — Canonical runtime reference for sessions, runs, context, memory, and channels.
- **[Agents & Sessions](./agents-and-sessions/)** — Understand how sessions and context work.
- **[Agent Teams](./agent-teams/)** — Learn how Tandem orchestrates specialized sub-agents.
- **[Configuration](./configuration/)** — Setup providers, API keys, and system instructions.

---

## ☁️ I am a Server Admin

_You want to deploy Tandem to a VPS or headless server so that you, or your external applications, can access autonomous agents remotely._

- **[Control Panel (Web Admin)](./control-panel/)** — Install the packaged web admin or generate an editable control panel app.
- **[Headless Service](./headless-service/)** — Run the Tandem Engine in headless API mode.
- **[Channel Integrations](./channel-integrations/)** — Connect Telegram, Discord, and Slack with media-aware prompt flow.
- **[Deployment Guide](./desktop/headless-deployment/)** — Learn best practices for securely exposing Tandem.
- **[Protocol Matrix](./protocol-matrix/)** — Understand the ports and network boundaries.

---

## 💻 I am a Developer

_You want to build custom clients, connect external tools via MCP, or programmatically trigger agent workflows._

- **[Building Automated Agents](./mcp-automated-agents/)** — Trigger agent pipelines automatically.
- **[Prompting Workflows And Missions](./prompting-workflows-and-missions/)** — Turn human intent into strong staged workflows and missions.
- **[Agent Workflow And Mission Quickstart](./agent-workflow-mission-quickstart/)** — Minimal checklist for agents creating and running Tandem systems.
- **[Tandem Wow Demo Playbook](./tandem-wow-demo-playbook/)** — Teach agents how to turn docs into showcase payloads with clear handoffs and tight tool scopes.
- **[Choosing Providers And Models For Agents](./choosing-providers-and-models-for-agents/)** — Pick stable defaults and targeted overrides without burying model choices in prompts.
- **[Creating And Running Workflows And Missions](./creating-and-running-workflows-and-missions/)** — Choose the right Tandem path and operate it correctly.
- **[Automation Examples For Teams](./automation-examples-for-teams/)** — End-to-end workflow proofs for control-panel and SDK-driven automation.
- **[Build an Automation With the AI Assistant](./automation-composer-workflows/)** — Prompt-first workflow authoring with preview, validation, and run-now.
- **[Engine Authentication For Agents](./engine-authentication-for-agents/)** — Get the token, authorize calls, and connect agents safely.
- **[Autonomous Coding Agents with GitHub Projects](./autonomous-coding-agents-github-projects/)** — Build coding agents on Tandem's engine-native GitHub MCP path.
- **[Coding Tasks With Tandem](./coding-tasks-with-tandem/)** — Learn the execution contract for worktrees, diffs, commits, and verification.
- **[WebMCP for Agents](./webmcp-for-agents/)** — Expose local HTTP APIs to your agents.
- **[Browser Setup and Testing](./browser-setup-and-testing/)** — Build, install, validate, and incorporate `tandem-browser`.
- **SDKs:** Integrate Tandem into your own codebases using our official libraries.
  - 📘 **[TypeScript SDK](./sdk/typescript/)**
  - 🐍 **[Python SDK](./sdk/python/)**
- **[How Tandem Works Under the Hood](./how-tandem-works/)** — Canonical runtime reference for sessions, runs, context, memory, and channels.
- **[Tandem Architecture](./architecture/)** — Understand the internal design of the Engine.

---

> **First time here?** Start with the **[Start Here](./start-here/)** guide!
