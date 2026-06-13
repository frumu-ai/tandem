# Tandem North Star

Tandem's north star is to be the governed runtime for AI-first businesses.

An AI-first business needs agents that can do more than answer questions. Agents need to read company knowledge, use tools, prepare artifacts, coordinate workflows, and request approval before actions that matter. Without a runtime authority layer, that work collapses into unsafe patterns: overbroad context, hidden tool access, fragile transcripts, unclear approvals, and weak evidence.

Tandem's job is to make agentic work operationally controllable.

## The Core Belief

The model is not the perimeter.

A model can reason, draft, classify, summarize, code, and propose actions. It should not decide its own authority. Tandem sits below the model and controls access, execution, approval, state, and evidence.

```text
Intent -> Authority Projection -> Scoped Execution -> Approval Gates -> Artifacts -> Audit Trail
```

## The Product Boundary

Tandem is not primarily a chat product. Chat, desktop, web, TUI, SDKs, and channels are entrypoints.

The durable product is the runtime:

- sessions and runs
- workflows and automations
- context projection
- permissioned memory
- scoped built-in tools and MCP connectors
- provider and connector secret references
- approval gates
- artifacts and validation metadata
- runtime events, tool ledgers, and protected audit records

## The Customer Problem

AI-first businesses need agents to operate inside company systems without giving every agent every file, tool, credential, customer record, project, or action.

They need answers to operational questions that chat wrappers cannot reliably answer:

- What was this agent allowed to see?
- Which tools were visible at each step?
- Which credential or connector was used?
- Which action required approval?
- Who approved it, rejected it, or requested rework?
- What artifact was produced?
- Can this run be replayed, debugged, or audited?
- Did the runtime deny anything, and why?

## The Wedge

The wedge is governed AI work for teams that cannot rely on prompt-only controls.

Strong early use cases:

- approval-gated email and external updates
- governed coding agents with worktree and handoff evidence
- permissioned company knowledge and memory
- compliance, risk, and policy research workflows
- connector-governed MCP tool execution
- client-deployed AI operations where tenant boundaries matter

## What Must Stay True

Tandem should remain:

- **Runtime-first:** clients are interfaces, not separate engines
- **Authority-first:** prompts do not define permissions
- **Evidence-first:** important work leaves inspectable records
- **Human-gated where needed:** consequential actions require runtime-controlled approval
- **Provider-neutral:** the model provider is replaceable
- **Deployable where data lives:** local, headless, hosted, or customer infrastructure
- **Honest about maturity:** shipped primitives must be separated from enterprise roadmap claims

## What We Should Avoid

Tandem should not drift back into being described as:

- a personal productivity assistant
- a generic local-first desktop workspace
- a workflow prompt library
- a Zapier clone
- a model router
- consulting wrapped in prompts
- a product whose trust boundary depends on the model behaving correctly

## Practical Definition

Tandem is the runtime authority layer for AI-first work.

It lets a business give an agent a bounded job, scoped context, scoped tools, approval gates, and durable evidence.

That is the product.
