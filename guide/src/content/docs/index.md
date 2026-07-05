---
title: Tandem Authority Layer Guide
description: Tandem is the authority layer for AI-first work. Use these docs to set up runtime authority for agents, tools, memory, approvals, and audit trails.
template: doc
---

Welcome to the **Tandem documentation hub**. Tandem is the authority layer for AI-first work: an engine-owned runtime that controls what agents can see, which tools they can use, when humans must approve, and what evidence survives after the work is done.

To help you find what you need quickly, please select the path that best describes how you plan to use Tandem:

> AI assistant or agent? Start at the [For LLMs section](https://tandem.ac/#for-llms) for the fast path to correct results.

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

_You want to deploy Tandem to a VPS, headless server, hosted/private environment, or customer infrastructure so agents can operate through runtime authority rather than prompt-only permissions._

- **[Control Panel (Web Admin)](./control-panel/)** — Install the packaged web admin or generate an editable control panel app.
- **[Enterprise Client Onboarding Runbook](./enterprise-client-onboarding-runbook/)** — Bring a client pilot online quickly, then harden it for enterprise rollout.
- **[Enterprise Data Governance](./enterprise-data-governance/)** — Configure tenant-scoped org units, source bindings, connector credentials, quarantine review, and enterprise MCP governance.
- **[Headless Service](./headless-service/)** — Run the Tandem Engine in headless API mode.
- **[Channel Integrations](./channel-integrations/)** — Connect Telegram, Discord, and Slack with media-aware prompt flow.
- **[Deployment Guide](./desktop/headless-deployment/)** — Learn best practices for securely exposing Tandem.
- **[Protocol Matrix](./protocol-matrix/)** — Understand the ports and network boundaries.

### Setting up enterprise systems with agents

Start with the [Enterprise Client Onboarding Runbook](./enterprise-client-onboarding-runbook/) when the goal is to get a client live quickly, then use [Enterprise Data Governance](./enterprise-data-governance/) for the deeper endpoint and policy details.

Agents can help operators design and verify enterprise setups, but they should
work through the runtime's governance surfaces instead of bypassing them. A
useful agent can:

- map business domains into org units and access grants for admin review
- draft connector, source-binding, and data-class plans before credentials are attached
- inspect MCP availability with `mcp_list` and `mcp_list_catalog`
- request missing connector capabilities instead of self-connecting them
- generate staged automation definitions with narrow `tool_policy` and `mcp_policy`
- create runbooks for quarantine review, connector rotation, and incident response

Agents should not paste raw secrets, grant themselves enterprise admin access,
or treat catalog visibility as permission to execute connector tools.

---

## 💻 I am a Developer

_You want to build custom clients, connect external tools via MCP, or programmatically trigger AI-first workflows with scoped execution, approvals, permissioned memory, and evidence._

- **[Building Automated Agents](./mcp-automated-agents/)** — Trigger agent pipelines automatically.
- **[Enterprise Client Onboarding Runbook](./enterprise-client-onboarding-runbook/)** — Agent-facing pilot and hardening checklist for getting clients online fast.
- **[Enterprise Data Governance](./enterprise-data-governance/)** — Teach agents how hosted admins scope company data, connector credentials, source bindings, quarantine review, and enterprise MCP governance.
- **[Eval Runner CLI](./eval-runner/)** — Run versioned AI quality evaluation datasets with `cargo run -p tandem-eval --bin eval-runner`.
- **[Self-Operator Playbook](./self-operator-playbook/)** — Operate governed recursive automations safely. Premium governance feature set required for mutation flows.
- **[MCP Capability Discovery And Request Flow](./mcp-capability-discovery-and-request-flow/)** — Distinguish connected, cataloged, and uncataloged MCPs before requesting new capabilities.
- **[Prompting Workflows And Missions](./prompting-workflows-and-missions/)** — Turn human intent into strong staged workflows and missions.
- **[Agent Workflow And Mission Quickstart](./agent-workflow-mission-quickstart/)** — Minimal checklist for agents creating and running Tandem systems.
- **[Tandem Wow Demo Playbook](./tandem-wow-demo-playbook/)** — Teach agents how to turn docs into showcase payloads with clear handoffs and tight tool scopes.
- **[Choosing Providers And Models For Agents](./choosing-providers-and-models-for-agents/)** — Pick stable defaults and targeted overrides without burying model choices in prompts.
- **[Creating And Running Workflows And Missions](./creating-and-running-workflows-and-missions/)** — Choose the right Tandem path and operate it correctly.
- **[Automation Examples For Teams](./automation-examples-for-teams/)** — End-to-end workflow proofs for control-panel and SDK-driven automation.
- **[Automation V2 Webhooks](./automation-v2-webhooks/)** — Configure signed external triggers, including Notion and Linear issue webhook setup.
- **[Build an Automation With the AI Assistant](./automation-composer-workflows/)** — Prompt-first workflow authoring with preview, validation, and run-now.
- **[Memory Internals](./memory-internals/)** — Learn how Tandem stores transcript history, retrieval memory, replay state, and reusable knowledge.
- **[Storage Maintenance For Agents](./storage-maintenance/)** — Clean local storage safely and remove stale repo-local managed worktrees when `.tandem/worktrees` leaks after blocked or failed runs.
- **[Governance Reference](./reference/governance/)** — Review provenance chains, capability grants, approval queues, and audit events. Premium governance feature set required for managed mutation flows.
- **[Automation Governance Lifecycle](./reference/governance-lifecycle/)** — Inspect the concrete review, pause, approval, and retirement state machine used by the premium governance engine.
- **[Engine Authentication For Agents](./engine-authentication-for-agents/)** — Get the token, authorize calls, and connect agents safely.
- **[Autonomous Coding Agents with GitHub Projects](./autonomous-coding-agents-github-projects/)** — Build coding agents on Tandem's engine-native GitHub MCP path.
- **[Coding Tasks With Tandem](./coding-tasks-with-tandem/)** — Learn the execution contract for worktrees, diffs, commits, and verification.
- **[Incident Monitor Reference](./reference/incident-monitor/)** — Turn failures and operator findings into governed drafts, approvals, receipts, and destination publishes.
- **[Incident Monitor External Log Intake](./incident-monitor-external-log-intake/)** — Teach agents and operators how to connect external project logs, scoped intake keys, and reset/replay debug actions.
- **[WebMCP for Agents](./webmcp-for-agents/)** — Expose local HTTP APIs to your agents.
- **[Browser Setup and Testing](./browser-setup-and-testing/)** — Build, install, validate, and incorporate `tandem-browser`.
- **SDKs:** Integrate Tandem into your own codebases using our official libraries.
  - 📘 **[TypeScript SDK](./sdk/typescript/)**
  - 🐍 **[Python SDK](./sdk/python/)**
- **[How Tandem Works Under the Hood](./how-tandem-works/)** — Canonical runtime reference for sessions, runs, context, memory, and channels.
- **[Tandem Architecture](./architecture/)** — Understand the internal design of the Engine.

---

> **First time here?** Start with the **[Start Here](./start-here/)** guide!
