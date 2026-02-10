---
name: brainstorming
description: You MUST use this before any creative work (features, products, content, strategy, systems, or behavior changes). Start by classifying what we’re brainstorming, then run thorough one-question-at-a-time discovery, propose 2–3 approaches, and converge on a validated plan/spec.
version: 2.1.0
tags: [brainstorming, discovery, requirements, design, planning, ideation]
---

# Brainstorming Ideas Into Plans

## Purpose

Turn vague ideas into a validated plan or spec through natural, collaborative dialogue.

This is domain-generic: it can be used for product features, websites, backend systems, marketing campaigns, business ideas, content, workflows, or anything else.

Core rules:

- Classify first: ask what type of brainstorming this is (multiple choice).
- One question per message: keep cognitive load low.
- Discovery-first: do not draft solutions until intent and constraints are understood.
- Converge to a deliverable: end with a plan/spec that can be executed.

## When to Use

- Any new idea that could branch into multiple directions
- Ambiguous requests (we should…, let’s build…, can we improve…?)
- Planning work that needs clarified goals, scope, trade-offs, and acceptance criteria

## Avoid When

- The task is purely mechanical or trivial
- The user provides a complete spec and explicitly wants immediate execution

## Inputs That Improve Quality (Optional)

- Constraints: budget, time, team size, deadline
- Existing materials: docs, links, codebase, screenshots, examples
- Success criteria: metrics, target outcomes, definition of done
- Non-goals: what to avoid or explicitly exclude

## Step 0: Classification (Required)

Start every brainstorming session by asking a single multiple-choice question:

What are we brainstorming today?

- A) Product / feature (app, SaaS, tool)
- B) Website / UX / UI / design
- C) Engineering / architecture / systems (backend, infra, data)
- D) Bugfix / troubleshooting / quality improvements
- E) Content (blog, video, social, brand)
- F) Marketing / growth / distribution
- G) Business model / pricing / sales
- H) Process / operations / team workflow
- I) Something else (describe in one sentence)

Then ask one follow-up question that is specific to the chosen category.

Rule: until classification is answered, do not assume domain.

## The Process

### Phase 1: Discovery (One Question at a Time)

Ask one question per message. Prefer multiple choice when possible.
Use this ladder in order; skip only if already answered.

#### 1) The Why (Intent and Motivation)

- What problem are we solving, and why now?
- What happens if we do nothing?

#### 2) The Who (Audience / User / Stakeholders)

- Who is this for?
- Who decides what good looks like?

#### 3) The What (Scope)

- What must be true for this to be considered successful?
- What’s in-scope for MVP vs out-of-scope?

Use MoSCoW:

- Must / Should / Could / Won’t (for now)

#### 4) Constraints (Hard Limits)

- Deadline, budget, team capacity
- Tools/stack constraints
- Compliance/legal/privacy constraints
- Brand/tone constraints (for content/marketing)

#### 5) Inputs and Dependencies

- What do we need that we don’t control? (APIs, vendors, approvals, assets)
- What existing systems/content must be reused?

#### 6) Risks and Unknowns

- What are the biggest uncertainties?
- What’s the cost of being wrong?

Unknowns Register rule:

- Any unknown becomes a discovery task with a timebox and decision point.

#### 7) Definition of Done

- What specifically will be delivered?
- How will we validate it worked?

### Phase 2: Synthesis (Reflect Back)

Summarize in 150–250 words:

- Problem, audience, MVP scope, constraints, success criteria, open questions
  Then ask:
- Is this accurate before I propose options?

### Phase 3: Explore Approaches (2–3 Options)

Present 2–3 options with trade-offs and a recommendation:

- Option A (Recommended): why it best fits constraints
- Option B: simpler/faster alternative
- Option C: more ambitious/extensible alternative

Each option includes:

- Pros/cons
- Key risks
- Rough effort class (S/M/L) or time range if requested
- What to validate first

Then ask:

- Which option should we proceed with (A/B/C), or should I adjust?

### Phase 4: Produce the Deliverable (Incremental Validation)

Once an option is chosen, produce the output in small sections (200–300 words) and ask after each:

- Does this look right so far?

Choose the output type based on the classification:

#### If A) Product / feature

- Goals and non-goals
- User stories and acceptance criteria
- UX flow (if relevant)
- Data/logic overview
- MVP vs V2
- Rollout and metrics

#### If B) Website / UX / UI

- Site goals and audience
- Sitemap / IA
- Page-level sections and messaging
- Visual direction and components
- SEO/accessibility basics
- Analytics/events

#### If C) Engineering / architecture / systems

- Requirements (functional and non-functional)
- Components/services and responsibilities
- Data flow and contracts
- Failure modes and retries
- Security and observability
- Testing and rollout plan

#### If D) Bugfix / troubleshooting

- Repro and root cause hypotheses
- Fastest validation steps
- Fix options and risk
- Test plan
- Rollback strategy

#### If E) Content

- Audience and promise
- Angle/hook options
- Outline/script structure
- Examples and CTAs
- Distribution plan

#### If F) Marketing / growth

- Target segment and channel assumptions
- Positioning and messaging
- Funnel (top to conversion)
- Experiments and metrics
- Timeline and budget assumptions

#### If G) Business model / pricing / sales

- ICP and value props
- Pricing hypotheses
- Packaging options
- Sales motion and objections
- Metrics and tests

#### If H) Process / operations / workflow

- Current state and pain points
- Desired outcomes
- Proposed process changes
- Tools and responsibilities
- Rollout and check-ins

#### If I) Something else

- Ask what output format they want (plan, spec, checklist, outline, roadmap)

### Output Handling

Always ask whether the user wants the final deliverable saved to a file.
If yes, write a Markdown file and confirm the filename.
Default filename: brainstorming-plan.md

## Category-Specific Starter Questions (Pick One)

### Product / feature

- What user action should this enable that’s currently impossible or painful?

### Website / UX / UI

- What is the single most important action you want visitors to take?

### Engineering / systems

- What are the top 3 non-functional requirements: latency, cost, reliability, security, or something else?

### Bugfix / troubleshooting

- Do we have consistent reproduction steps (Yes/No/Not sure)?

### Content

- What platform is this for (YouTube, X, blog, email, TikTok, other)?

### Marketing / growth

- Which channel are we prioritizing first (SEO, paid ads, social, partnerships, outbound, other)?

### Business model / pricing

- Who is the buyer (end user, team lead, company)?

### Process / workflow

- What’s the current workflow step that causes the most friction?

## Key Principles

- Classify first (no domain assumptions)
- One question at a time (always)
- Multiple choice preferred (reduce friction)
- Start broad, then narrow
- Explore alternatives (2–3 options)
- YAGNI ruthlessly (MVP-first)
- Incremental validation (section-by-section)
- Make assumptions explicit (and confirm them)
- Stop and clarify whenever uncertainty appears
