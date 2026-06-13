# Tandem Vision

Tandem is the governed runtime for AI-first businesses.

AI-first companies will not run on people copy-pasting between chat windows. They will run with agents that read company context, call tools, draft decisions, operate workflows, and coordinate work across systems. That shift creates a new control problem: the model cannot be the security boundary, the transcript cannot be the system of record, and prompts cannot be treated as permissions.

Tandem exists to put authority below the model.

The runtime controls what an agent can see, which tools it can discover, which actions it can execute, when a human must approve, and what evidence survives after the work is done. Agents propose and perform work inside projected authority. Tandem owns the state, policies, memory, approvals, artifacts, and audit trail.

## North Star

An AI-first business should be able to assign work to agents the same way it assigns work to people, but with stronger controls:

- scoped access to only the context needed for the job
- scoped tools and connector credentials
- human gates before consequential actions
- durable artifacts instead of prose-only outputs
- replayable run state and audit evidence
- tenant, workspace, and data-boundary enforcement

Tandem's north star is to become the runtime authority layer that makes this operationally safe.

## What We Are Building

- **Governed execution runtime:** Runs, workflows, context, tools, approvals, artifacts, and audit live in engine-owned state rather than in a chat transcript.
- **Authority projection:** Agents receive bounded access based on tenant, principal, resource, grant, data class, and workflow step.
- **Permissioned company memory:** Company knowledge becomes useful to agents without becoming flat global context.
- **Approval-gated action:** Consequential work pauses for approve, rework, or cancel decisions with durable evidence.
- **Provider-agnostic infrastructure:** Teams can use OpenAI, Anthropic, OpenRouter, OpenCode Zen, Ollama, or compatible endpoints without making the model provider the control plane.
- **Deployable runtime:** Tandem can run locally, headlessly, hosted, or inside customer infrastructure where company data and evidence need to live.

## Who It Is For

Tandem is for builders and teams turning AI from an assistant into an operating layer:

- AI-first startups building agentic products
- small businesses using agents to run real operational work
- platform teams that need governed internal AI systems
- security and compliance teams that need evidence, approval, and auditability
- integrators deploying agent workflows into client environments

## What Success Looks Like

A customer can give an agent a real business task and know:

1. what context the agent was allowed to use
2. which tools it was allowed to see and call
3. which actions required approval
4. what the agent produced as durable artifacts
5. why the runtime allowed, denied, paused, or resumed the work
6. what evidence remains for debugging, compliance, and trust

That is the boundary Tandem is building toward: not another chat UI, but the governed runtime underneath AI-first work.
