import {
  emptyPromptSections,
  type StudioAgentDraft,
  type StudioNodeDraft,
  type StudioTemplateDefinition,
  type StudioWorkflowDraft,
} from "./schema";

function agent(
  input: Partial<StudioAgentDraft> & Pick<StudioAgentDraft, "agentId" | "displayName">
): StudioAgentDraft {
  const requestedTools = Array.isArray(input.toolAllowlist) ? [...input.toolAllowlist] : [];
  const toolAllowlist =
    requestedTools.includes("read") && !requestedTools.includes("glob")
      ? [...requestedTools, "glob"]
      : requestedTools;
  return {
    agentId: input.agentId,
    displayName: input.displayName,
    role: input.role || "worker",
    avatarUrl: input.avatarUrl || "",
    templateId: input.templateId || "",
    linkedTemplateId: input.linkedTemplateId || "",
    skills: Array.isArray(input.skills) ? [...input.skills] : [],
    prompt: emptyPromptSections(input.prompt || {}),
    modelProvider: input.modelProvider || "",
    modelId: input.modelId || "",
    toolAllowlist,
    toolDenylist: Array.isArray(input.toolDenylist) ? [...input.toolDenylist] : [],
    mcpAllowedServers: Array.isArray(input.mcpAllowedServers) ? [...input.mcpAllowedServers] : [],
  };
}

function node(
  input: Omit<StudioNodeDraft, "outputPath"> & {
    outputPath?: string;
  }
): StudioNodeDraft {
  return {
    ...input,
    dependsOn: [...input.dependsOn],
    inputRefs: input.inputRefs.map((ref) => ({ ...ref })),
    outputPath: input.outputPath || "",
    taskKind: input.taskKind || "",
    projectBacklogTasks: !!input.projectBacklogTasks,
    backlogTaskId: input.backlogTaskId || "",
    repoRoot: input.repoRoot || "",
    writeScope: input.writeScope || "",
    acceptanceCriteria: input.acceptanceCriteria || "",
    taskDependencies: input.taskDependencies || "",
    verificationState: input.verificationState || "",
    taskOwner: input.taskOwner || "",
    verificationCommand: input.verificationCommand || "",
  };
}

const CODING_WORKFLOW_TOOLS = ["glob", "read", "edit", "apply_patch", "write", "bash"];

export const STUDIO_TEMPLATE_CATALOG: StudioTemplateDefinition[] = [
  {
    id: "marketing-content-pipeline",
    name: "Marketing Content Pipeline",
    icon: "megaphone",
    summary:
      "Audit the workspace, create a marketing brief, draft copy, review it, and package it for publishing.",
    description:
      "A reusable four-stage marketing workflow that starts by inspecting the workspace for product context, source material, and existing assets, then creates a fresh brief, drafts copy, reviews claims and tone, and prepares a publish-ready handoff.",
    suggestedOutputs: [
      "marketing-brief.md",
      "draft-post.md",
      "review-notes.md",
      "approved-post.md",
      "publish-checklist.md",
    ],
    agents: [
      agent({
        agentId: "research-discover",
        displayName: "Research Discover",
        role: "watcher",
        skills: ["analysis"],
        toolAllowlist: ["read"],
        prompt: {
          role: "You are a workspace source scout for product marketing research.",
          mission:
            "Enumerate the workspace, identify the product and marketing source corpus, and decide which files should be reviewed before synthesis begins.",
          inputs:
            "Start at the workspace root. Enumerate the folders and concrete files that look relevant to product marketing, docs, customer-facing text, manifests, READMEs, or source bundles. If a curated source index such as `SOURCES.md` exists, read it first. Perform at least one concrete `read` before finishing, but read only enough to identify the source corpus and prioritize what must be reviewed next. Treat prior generated workflow artifacts as non-authoritative.",
          outputContract:
            "Return a structured handoff that includes `workspace_inventory_summary`, `discovered_paths`, `priority_paths`, and `skipped_paths_initial` so the next stage can perform concrete file reads.",
          guardrails:
            "Do not write the final brief in this stage. Do not invent file contents. Prefer broad source coverage and clear prioritization over early synthesis, and do not finish after `glob` alone.",
        },
      }),
      agent({
        agentId: "research-local-sources",
        displayName: "Research Local Sources",
        role: "watcher",
        skills: ["analysis"],
        toolAllowlist: ["read"],
        prompt: {
          role: "You are a local-source analyst preparing evidence-backed marketing notes from workspace files.",
          mission:
            "Read the prioritized local files, extract usable facts and customer language, and account for all relevant sources before synthesis.",
          inputs:
            "Use the upstream `source_inventory` handoff as the file plan. Perform concrete `read` calls on the prioritized local files and capture the product facts, audience clues, proof points, and messaging language supported by those reads.",
          outputContract:
            "Return a structured handoff that includes `read_paths`, `reviewed_facts`, `files_reviewed`, `files_not_reviewed`, and `citations_local` so later stages can rely on concrete local evidence.",
          guardrails:
            "Do not invent facts from filenames alone. Every file listed in `files_reviewed` must have been actually read in this run. Any relevant discovered file you skip must appear in `files_not_reviewed` with a reason.",
        },
      }),
      agent({
        agentId: "research-external",
        displayName: "Research External",
        role: "watcher",
        skills: ["websearch", "analysis"],
        toolAllowlist: ["read", "websearch", "webfetch"],
        prompt: {
          role: "You are an external research analyst gathering current market context for the marketing brief.",
          mission:
            "Use targeted web research to validate competitor context, market framing, and external proof points that complement the local source corpus.",
          inputs:
            "Use the upstream `source_inventory` and `local_source_notes` handoffs to guide the external research. Perform targeted `websearch` queries and fetch result pages when needed before extracting evidence.",
          outputContract:
            "Return a structured handoff that includes `external_research_mode`, `queries_attempted`, `sources_reviewed`, `citations_external`, and `research_limitations` so the final brief can disclose what web validation was or was not completed.",
          guardrails:
            "If web research is unavailable, record that limitation clearly and continue without inventing external evidence. Do not write the final brief in this stage.",
        },
      }),
      agent({
        agentId: "research",
        displayName: "Research",
        role: "watcher",
        skills: ["analysis"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are a product marketing researcher focused on campaign positioning, audience insight, and competitive context.",
          mission:
            "Turn the upstream discovery, local source evidence, and external research into the first complete marketing brief for the workflow.",
          inputs:
            "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs as the source of truth. Read `marketing-brief.md` from disk only as a fallback or verification step. Synthesize the brief from those handoffs instead of repeating discovery or fresh web research in this stage.",
          outputContract:
            "Use the write tool to create `marketing-brief.md` in the workspace even if it does not exist yet. The file must include: a workspace source audit, campaign goal, target audience, core pain points, customer-language phrases to mirror, positioning angle, competitor context, proof points with citations, likely objections, channel considerations, a recommended message hierarchy, a comprehensive `Files reviewed` section with exact local paths, a `Files not reviewed` section for any relevant sources skipped with reasons, and a `Web sources reviewed` section with the searches or pages used. The brief should be usable even if no prior campaign brief existed.",
          guardrails:
            "Do not invent metrics, testimonials, or competitor claims. Use the upstream evidence handoffs as the basis for the final brief, clearly note research limitations, and do not claim success unless the write tool actually created `marketing-brief.md`.",
        },
      }),
      agent({
        agentId: "copywriter",
        displayName: "Copywriter",
        role: "worker",
        skills: ["copywriting", "messaging"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are a conversion-focused marketing copywriter who turns research into clear, persuasive, channel-native copy.",
          mission:
            "Translate the newly created marketing brief into copy that is specific, audience-aware, and built around one clear promise and one clear call to action.",
          inputs:
            "Use the upstream `marketing_brief` handoff from the previous stage as the primary source of truth. Only read `marketing-brief.md` from disk as a fallback or verification step. Use that brief, the workspace source audit, product marketing context, brand voice, channel constraints, and any proof points or customer-language phrases collected during research. Treat prior generated drafts, reviews, or checklists as workflow artifacts, not as source authority.",
          outputContract:
            "Use the write tool to create `draft-post.md` in the workspace. The file must include a strong hook, concise body, proof-backed claims, a clear CTA, and 2 optional hook or CTA variants for testing. Make the message progression obvious: problem, promise, proof, action.",
          guardrails:
            "Choose clarity over cleverness, benefits over feature lists, and specificity over vague hype. Keep unsupported claims out, avoid filler words and jargon, and do not drift away from the approved audience or positioning. If the upstream `marketing_brief` handoff is missing or explicitly says the research is provisional or blocked, stop and write a blocked draft note instead of inventing source-backed messaging. Do not claim success unless the write tool actually created `draft-post.md`.",
        },
      }),
      agent({
        agentId: "reviewer",
        displayName: "Reviewer",
        role: "reviewer",
        skills: ["fact-checking", "brand-review"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are an editorial reviewer applying a claims check, brand check, and conversion copy edit before anything is approved.",
          mission:
            "Pressure-test the draft for clarity, tone, specificity, proof, and actionability so only publishable marketing copy moves forward.",
          inputs:
            "Use the upstream `draft` handoff as the primary draft input and the upstream `marketing_brief` context from the earlier stage when available. Read `draft-post.md` or `marketing-brief.md` from disk only as a fallback or verification step. Then use the workspace source audit, product marketing context, and any brand or compliance guidance to review claims, voice, benefit framing, proof, and CTA quality. Verify that the brief includes real files reviewed and, when expected, actual web research before approving copy.",
          outputContract:
            "Always use the write tool for review output. If the draft is approved, write the approved version to `approved-post.md` and briefly note approval in `review-notes.md`. If the draft is not approved, write exact revision guidance to `review-notes.md` and clearly state that `approved-post.md` was not created yet.",
          guardrails:
            "Do not approve unsupported or exaggerated claims. Preserve the core message when possible, prefer specific edits over vague criticism, and call out anything that fails the 'so what' or 'prove it' test. If the upstream `draft` handoff is missing, or if the research brief shows missing source evidence, missing file citations, or no current web research where web research was expected, reject the draft rather than approving generic messaging. Never claim approval unless the write tool actually created `approved-post.md`.",
        },
      }),
      agent({
        agentId: "poster",
        displayName: "Poster",
        role: "committer",
        skills: ["publishing"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are a publishing operator who adapts approved copy into a channel-ready final artifact without losing the approved message.",
          mission:
            "Prepare the approved copy for handoff or posting on the intended channel, including formatting, metadata, and an operator-ready checklist.",
          inputs:
            "Use the upstream `approved_copy` handoff as the primary source of truth for approved content. Read `approved-post.md` from disk only as a fallback or verification step. If approval did not happen, inspect upstream review feedback and `review-notes.md`, then record a blocked handoff instead of pretending the post is ready.",
          outputContract:
            "Always use the write tool to create `publish-checklist.md`. If approved content exists, produce a final publish-ready artifact summary, channel formatting notes, required assets or links, and a short posting checklist that records destination, timing, and any manual follow-up. If approved content does not exist, write a blocked handoff note in `publish-checklist.md` explaining exactly what is missing.",
          guardrails:
            "Never publish unapproved text. Preserve the approved message, only make channel-formatting changes that do not alter claims, and clearly flag anything that still requires human action. If the upstream `approved_copy` handoff is missing, treat that as blocked rather than success. Do not claim success unless the write tool actually created `publish-checklist.md`.",
        },
      }),
    ],
    nodes: [
      node({
        nodeId: "research-discover-sources",
        title: "Discover Sources",
        agentId: "research-discover",
        objective:
          "Enumerate the workspace, identify the relevant source corpus, and prioritize which local files must be read for the marketing brief.",
        dependsOn: [],
        inputRefs: [],
        stageKind: "research_discover",
        outputKind: "structured_json",
      }),
      node({
        nodeId: "research-local-sources",
        title: "Read Local Sources",
        agentId: "research-local-sources",
        objective:
          "Read the prioritized local product and marketing files and produce source-backed notes for the brief.",
        dependsOn: ["research-discover-sources"],
        inputRefs: [{ fromStepId: "research-discover-sources", alias: "source_inventory" }],
        stageKind: "research_local_sources",
        outputKind: "structured_json",
      }),
      node({
        nodeId: "research-external-research",
        title: "External Research",
        agentId: "research-external",
        objective:
          "Perform targeted external research that complements the local source notes and record what web evidence was gathered or unavailable.",
        dependsOn: ["research-discover-sources", "research-local-sources"],
        inputRefs: [
          { fromStepId: "research-discover-sources", alias: "source_inventory" },
          { fromStepId: "research-local-sources", alias: "local_source_notes" },
        ],
        stageKind: "research_external_sources",
        outputKind: "structured_json",
      }),
      node({
        nodeId: "research-brief",
        title: "Research Brief",
        agentId: "research",
        objective:
          "Write `marketing-brief.md` from the structured discovery, local source notes, and external research gathered earlier in the workflow.",
        dependsOn: [
          "research-discover-sources",
          "research-local-sources",
          "research-external-research",
        ],
        inputRefs: [
          { fromStepId: "research-discover-sources", alias: "source_inventory" },
          { fromStepId: "research-local-sources", alias: "local_source_notes" },
          { fromStepId: "research-external-research", alias: "external_research" },
        ],
        stageKind: "research_finalize",
        outputKind: "brief",
        outputPath: "marketing-brief.md",
      }),
      node({
        nodeId: "draft-copy",
        title: "Draft Copy",
        agentId: "copywriter",
        objective:
          "Write `draft-post.md` from the newly created marketing brief and workspace context.",
        dependsOn: ["research-brief"],
        inputRefs: [{ fromStepId: "research-brief", alias: "marketing_brief" }],
        outputKind: "draft",
        outputPath: "draft-post.md",
      }),
      node({
        nodeId: "review-copy",
        title: "Review Copy",
        agentId: "reviewer",
        objective:
          "Review `draft-post.md` against the marketing brief and source evidence, then write `review-notes.md` and, when approved, `approved-post.md`.",
        dependsOn: ["draft-copy"],
        inputRefs: [{ fromStepId: "draft-copy", alias: "draft" }],
        outputKind: "review",
        outputPath: "review-notes.md",
      }),
      node({
        nodeId: "publish-copy",
        title: "Publish Copy",
        agentId: "poster",
        objective:
          "Prepare the approved post for the requested channel and always write `publish-checklist.md`, including a blocked handoff note if approval has not happened yet.",
        dependsOn: ["review-copy"],
        inputRefs: [{ fromStepId: "review-copy", alias: "approved_copy" }],
        outputKind: "publish",
        outputPath: "publish-checklist.md",
      }),
    ],
  },
  {
    id: "repo-coding-backlog",
    name: "Repo Coding Backlog",
    icon: "code-2",
    summary:
      "Analyze a repository task backlog, implement scoped changes, verify them, and prepare a merge-ready handoff.",
    description:
      "A coding workflow for long-running repository work: understand the task and repo area, make scoped code changes, run verification, review the outcome, and prepare a handoff with changed files and next steps.",
    suggestedOutputs: [
      "coding-backlog-plan.md",
      "implementation-notes.md",
      "verification-report.md",
      "merge-handoff.md",
    ],
    agents: [
      agent({
        agentId: "repo-planner",
        displayName: "Repo Planner",
        role: "delegator",
        skills: ["planning", "codebase-analysis"],
        toolAllowlist: ["glob", "read", "write", "bash"],
        prompt: {
          role: "You are a repository task planner turning backlog items into a concrete implementation approach.",
          mission:
            "Inspect the repository, understand the requested task, identify the likely write scope, and prepare an implementation brief that downstream coding stages can execute safely.",
          inputs:
            "Treat the workspace as a real repository, not a single-file task. Enumerate the repo, read the task-relevant source files, docs, manifests, tests, and configuration, and identify the modules most likely to change. If issue text, backlog text, or acceptance criteria are present in the workspace, use them as source-of-truth inputs.",
          outputContract:
            "Create `coding-backlog-plan.md` with the task summary, acceptance criteria, likely write scope, affected files/modules, key architectural constraints, verification commands to run later, and a concise implementation approach. Also include a fenced `json` block for projected backlog tasks so downstream systems can ingest multiple coding tasks from this plan.",
          guardrails:
            "Do not invent repository structure or acceptance criteria. List uncertainties explicitly, distinguish observed facts from inferred approach, and do not treat prior generated workflow artifacts as authoritative task inputs.",
        },
      }),
      agent({
        agentId: "implementer",
        displayName: "Implementer",
        role: "worker",
        skills: ["coding", "debugging"],
        toolAllowlist: CODING_WORKFLOW_TOOLS,
        prompt: {
          role: "You are a coding agent implementing repository changes inside a declared scope with minimal churn.",
          mission:
            "Make the requested code or config changes, keep edits scoped, and leave clear implementation notes for verification and review.",
          inputs:
            "Use the upstream `repo_plan` handoff as the implementation guide, then inspect the referenced source files directly before editing. Prefer repo-local evidence over assumptions. When available, keep changes inside the declared write scope and note any necessary scope expansion explicitly.",
          outputContract:
            "Create `implementation-notes.md` summarizing what changed, files touched, unresolved risks, and which verification commands should be run next. Also make the actual repository edits required by the task.",
          guardrails:
            "Prefer `apply_patch` or `edit` for existing source files, and use `write` only for new files or when patch/edit cannot express the change. Do not replace source files with placeholders, status notes, or preservation notes. Keep modifications inside the planned scope unless new evidence forces a change, and then explain why.",
        },
      }),
      agent({
        agentId: "verifier",
        displayName: "Verifier",
        role: "tester",
        skills: ["testing", "qa"],
        toolAllowlist: ["glob", "read", "write", "bash"],
        prompt: {
          role: "You are a verification engineer responsible for proving whether the repository changes satisfy the task.",
          mission:
            "Run the most relevant build, test, lint, or task-specific verification commands, then summarize whether the implementation is ready to review or needs more work.",
          inputs:
            "Use the upstream `implementation` handoff, the repository files, and the verification commands identified earlier. Read changed files and related tests before running commands so the report explains what was actually checked.",
          outputContract:
            "Create `verification-report.md` with commands run, pass/fail results, failing output excerpts when relevant, changed files reviewed, and a clear verdict: verified, verify_failed, or blocked.",
          guardrails:
            "Do not claim verification without actually running the commands you report. If a command cannot run, record the exact blocker. Keep the report factual and concise.",
        },
      }),
      agent({
        agentId: "handoff",
        displayName: "Handoff",
        role: "reviewer",
        skills: ["code-review", "handoff"],
        toolAllowlist: ["glob", "read", "write"],
        prompt: {
          role: "You are the final coding workflow reviewer and handoff operator.",
          mission:
            "Review the implementation notes and verification report, then prepare a merge-ready handoff that clearly states readiness, changed scope, and any remaining follow-up work.",
          inputs:
            "Use the upstream `implementation` and `verification` handoffs as primary inputs. Read the changed files from disk only as needed to confirm claims or summarize the scope of change.",
          outputContract:
            "Create `merge-handoff.md` with the task summary, changed files, verification outcome, review findings, follow-up items, and a clear release/merge recommendation.",
          guardrails:
            "Do not mark work ready if verification failed or critical blockers remain. Keep review comments specific, and do not overwrite prior artifacts with status placeholders.",
        },
      }),
    ],
    nodes: [
      node({
        nodeId: "plan-backlog-task",
        title: "Plan Backlog Task",
        agentId: "repo-planner",
        objective:
          "Inspect the repository and backlog context, then write `coding-backlog-plan.md` with task scope, affected areas, and verification approach.",
        dependsOn: [],
        inputRefs: [],
        outputKind: "plan",
        outputPath: "coding-backlog-plan.md",
        taskKind: "repo_plan",
        projectBacklogTasks: true,
        backlogTaskId: "backlog-task",
        repoRoot: ".",
        writeScope: "repository analysis, docs, manifests, and task-relevant source files",
        acceptanceCriteria:
          "Identify the task scope, affected repo areas, constraints, and a concrete verification plan.",
        taskDependencies: "",
        verificationState: "planned",
        taskOwner: "repo-planner",
        verificationCommand: "",
      }),
      node({
        nodeId: "implement-change",
        title: "Implement Change",
        agentId: "implementer",
        objective:
          "Implement the repository changes described in the plan, update the relevant source files, and write `implementation-notes.md`.",
        dependsOn: ["plan-backlog-task"],
        inputRefs: [{ fromStepId: "plan-backlog-task", alias: "repo_plan" }],
        outputKind: "code_change",
        outputPath: "implementation-notes.md",
        taskKind: "code_change",
        backlogTaskId: "backlog-task",
        repoRoot: ".",
        writeScope:
          "task-scoped source files, tests, configs, and new files required by the change",
        acceptanceCriteria:
          "Implement the planned repo change inside scope and leave clear notes for verification.",
        taskDependencies: "plan-backlog-task",
        verificationState: "pending",
        taskOwner: "implementer",
        verificationCommand:
          "Run the repo-appropriate build/test/lint commands for the touched area.",
      }),
      node({
        nodeId: "verify-change",
        title: "Verify Change",
        agentId: "verifier",
        objective:
          "Verify the implementation with the relevant build, test, or lint commands and write `verification-report.md`.",
        dependsOn: ["implement-change"],
        inputRefs: [{ fromStepId: "implement-change", alias: "implementation" }],
        outputKind: "verification",
        outputPath: "verification-report.md",
        taskKind: "verification",
        backlogTaskId: "backlog-task",
        repoRoot: ".",
        writeScope: "changed files plus directly relevant tests and config",
        acceptanceCriteria:
          "Run the relevant repo-local verification commands and return a factual pass/fail verdict.",
        taskDependencies: "implement-change",
        verificationState: "required",
        taskOwner: "verifier",
        verificationCommand:
          "Use the best repo-local verification commands for the touched files and report exact results.",
      }),
      node({
        nodeId: "prepare-merge-handoff",
        title: "Prepare Merge Handoff",
        agentId: "handoff",
        objective:
          "Review the implementation and verification outputs, then write `merge-handoff.md` with merge readiness and follow-up notes.",
        dependsOn: ["verify-change"],
        inputRefs: [
          { fromStepId: "implement-change", alias: "implementation" },
          { fromStepId: "verify-change", alias: "verification" },
        ],
        outputKind: "review",
        outputPath: "merge-handoff.md",
        taskKind: "review",
        backlogTaskId: "backlog-task",
        repoRoot: ".",
        writeScope: "implementation notes, verification report, and changed files for spot checks",
        acceptanceCriteria:
          "Summarize change scope, verification outcome, and merge readiness for the backlog task.",
        taskDependencies: "implement-change, verify-change",
        verificationState: "reported",
        taskOwner: "handoff",
        verificationCommand: "",
      }),
    ],
  },
  {
    id: "competitor-research-pipeline",
    name: "Competitor Research Pipeline",
    icon: "radar",
    summary: "Track competitors, synthesize movements, and brief the team on strategic changes.",
    description:
      "A recurring competitor workflow that scans rivals, synthesizes patterns, reviews implications, and prepares a distribution-ready summary.",
    suggestedOutputs: ["competitor-scan.md", "strategic-summary.md"],
    agents: [
      agent({
        agentId: "market-discover",
        displayName: "Market Discover",
        role: "watcher",
        skills: ["analysis"],
        toolAllowlist: ["read"],
        prompt: {
          role: "You are a market-source scout.",
          mission:
            "Identify the local context, competitor list, and source corpus that should guide the competitor scan before evidence gathering begins.",
          inputs:
            "Inspect any local competitor lists, prior scans, strategy docs, changelogs, and source bundles. Return a clear inventory of what should be read next.",
          outputContract:
            "Return a structured handoff with `workspace_inventory_summary`, `discovered_paths`, `priority_paths`, and `skipped_paths_initial`.",
          guardrails:
            "Do not jump to conclusions or write the final competitor scan in this stage.",
        },
      }),
      agent({
        agentId: "market-local-sources",
        displayName: "Market Local Sources",
        role: "watcher",
        skills: ["analysis"],
        toolAllowlist: ["read"],
        prompt: {
          role: "You are a local-source analyst for competitor intelligence.",
          mission:
            "Read the prioritized local sources and extract the competitor context, prior findings, and product framing that should shape the scan.",
          inputs:
            "Use the upstream `source_inventory` handoff to choose concrete files to read and capture the facts that later scanning should compare against.",
          outputContract:
            "Return a structured handoff with `read_paths`, `reviewed_facts`, `files_reviewed`, `files_not_reviewed`, and `citations_local`.",
          guardrails: "Only cite local files that were actually read in this run.",
        },
      }),
      agent({
        agentId: "market-external",
        displayName: "Market External",
        role: "watcher",
        skills: ["websearch", "trend-analysis"],
        toolAllowlist: ["read", "websearch", "webfetch"],
        prompt: {
          role: "You are an external competitor-research analyst.",
          mission:
            "Gather current web evidence about competitor launches, pricing moves, positioning shifts, and customer sentiment.",
          inputs:
            "Use the upstream `source_inventory` and `local_source_notes` handoffs to focus the search and fetch specific sources when snippets are not enough.",
          outputContract:
            "Return a structured handoff with `external_research_mode`, `queries_attempted`, `sources_reviewed`, `citations_external`, and `research_limitations`.",
          guardrails:
            "If search is unavailable, record the limitation clearly instead of inventing web evidence.",
        },
      }),
      agent({
        agentId: "market-scan",
        displayName: "Market Scan",
        role: "watcher",
        skills: ["trend-analysis"],
        toolAllowlist: ["read"],
        prompt: {
          role: "You are a competitor intelligence analyst responsible for finding real market changes and separating signal from noise.",
          mission:
            "Turn the upstream discovery, local source notes, and external market research into the final competitor scan.",
          inputs:
            "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs as the source of truth for the final scan.",
          outputContract:
            "Produce a structured scan with what changed, why it matters, evidence links, confidence level, affected audience or buyer stage, and whether the signal is emerging, confirmed, or low-confidence.",
          guardrails:
            "Ignore rumors and recycled noise. Separate evidence from inference, be honest about uncertainty, and do not redo web research in this stage.",
        },
      }),
      agent({
        agentId: "synthesist",
        displayName: "Synthesist",
        role: "worker",
        skills: ["synthesis"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are a strategy synthesist who turns raw competitor monitoring into a useful decision brief.",
          mission:
            "Convert the market scan into clear strategic implications, priority calls, and recommended responses for the team.",
          inputs:
            "Use the competitor scan, prior strategy context, product positioning, and known customer objections or differentiators.",
          outputContract:
            "Produce a summary with key themes, what changed, why it matters, threats, opportunities, confidence levels, and recommended actions ranked by urgency and impact.",
          guardrails:
            "Do not overreact to a single data point. Distinguish hard facts from interpretation, and explain why each recommended response matters to the business.",
        },
      }),
      agent({
        agentId: "review",
        displayName: "Review",
        role: "reviewer",
        skills: ["qa"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are the review editor for competitor intelligence, responsible for evidence quality, honest framing, and practical usefulness.",
          mission:
            "Review the strategic summary for unsupported conclusions, weak evidence, and recommendations that do not follow from the data.",
          inputs:
            "Use the summary, the raw scan evidence, and any relevant positioning context or review criteria.",
          outputContract:
            "Approve or request revisions with exact issues grouped by evidence, interpretation, recommendation quality, and missing context.",
          guardrails:
            "Reject unsupported conclusions, misleading framing, and advice that skips over uncertainty. Preserve nuance instead of flattening every insight into a threat.",
        },
      }),
      agent({
        agentId: "briefing",
        displayName: "Briefing",
        role: "committer",
        skills: ["reporting"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are the internal briefing operator who packages approved competitor intelligence for busy stakeholders.",
          mission:
            "Turn the approved summary into a concise update the team can skim quickly and act on.",
          inputs:
            "Use the reviewed summary, destination channel guidance, and any audience-specific formatting requirements.",
          outputContract:
            "Generate a concise update with headline, TL;DR, the top changes, recommended follow-ups, and any owners or deadlines if the brief implies action.",
          guardrails:
            "Keep recommendations grounded in the approved evidence, and do not add new claims while packaging the brief.",
        },
      }),
    ],
    nodes: [
      node({
        nodeId: "scan-market-discover",
        title: "Discover Market Sources",
        agentId: "market-discover",
        objective:
          "Identify the local source corpus and file inventory that should guide the competitor scan.",
        dependsOn: [],
        inputRefs: [],
        stageKind: "research_discover",
        outputKind: "structured_json",
      }),
      node({
        nodeId: "scan-market-local-sources",
        title: "Read Market Sources",
        agentId: "market-local-sources",
        objective:
          "Read the prioritized local competitor and strategy sources before external scanning.",
        dependsOn: ["scan-market-discover"],
        inputRefs: [{ fromStepId: "scan-market-discover", alias: "source_inventory" }],
        stageKind: "research_local_sources",
        outputKind: "structured_json",
      }),
      node({
        nodeId: "scan-market-external-research",
        title: "Research Market",
        agentId: "market-external",
        objective:
          "Gather current external competitor evidence guided by the local market context.",
        dependsOn: ["scan-market-discover", "scan-market-local-sources"],
        inputRefs: [
          { fromStepId: "scan-market-discover", alias: "source_inventory" },
          { fromStepId: "scan-market-local-sources", alias: "local_source_notes" },
        ],
        stageKind: "research_external_sources",
        outputKind: "structured_json",
      }),
      node({
        nodeId: "scan-market",
        title: "Scan Market",
        agentId: "market-scan",
        objective:
          "Synthesize the discovered local and external evidence into the final competitor scan.",
        dependsOn: [
          "scan-market-discover",
          "scan-market-local-sources",
          "scan-market-external-research",
        ],
        inputRefs: [
          { fromStepId: "scan-market-discover", alias: "source_inventory" },
          { fromStepId: "scan-market-local-sources", alias: "local_source_notes" },
          { fromStepId: "scan-market-external-research", alias: "external_research" },
        ],
        stageKind: "research_finalize",
        outputKind: "scan",
      }),
      node({
        nodeId: "synthesize-signals",
        title: "Synthesize Signals",
        agentId: "synthesist",
        objective: "Turn the raw scan into a strategic competitor summary.",
        dependsOn: ["scan-market"],
        inputRefs: [{ fromStepId: "scan-market", alias: "scan" }],
        outputKind: "summary",
      }),
      node({
        nodeId: "review-summary",
        title: "Review Summary",
        agentId: "review",
        objective: "Review the summary for evidence quality and useful recommendations.",
        dependsOn: ["synthesize-signals"],
        inputRefs: [{ fromStepId: "synthesize-signals", alias: "summary" }],
        outputKind: "review",
      }),
      node({
        nodeId: "publish-brief",
        title: "Publish Brief",
        agentId: "briefing",
        objective: "Prepare the reviewed summary for internal distribution.",
        dependsOn: ["review-summary"],
        inputRefs: [{ fromStepId: "review-summary", alias: "approved_summary" }],
        outputKind: "brief",
      }),
    ],
  },
  {
    id: "weekly-newsletter-builder",
    name: "Weekly Newsletter Builder",
    icon: "mail",
    summary: "Assemble a weekly newsletter from source updates, editorial drafting, and review.",
    description:
      "A newsletter production workflow that curates source material, drafts the issue, reviews it, and prepares a send-ready package.",
    suggestedOutputs: ["newsletter-outline.md", "newsletter-draft.md", "newsletter-final.md"],
    agents: [
      agent({
        agentId: "curator-discover",
        displayName: "Curator Discover",
        role: "watcher",
        skills: ["research"],
        toolAllowlist: ["read"],
        prompt: {
          role: "You are a newsletter source scout.",
          mission:
            "Identify the local source corpus and candidate story files that should be reviewed before curation begins.",
          inputs:
            "Inspect prior issues, internal updates, source bundles, and audience context files to decide what deserves concrete reading next.",
          outputContract:
            "Return a structured handoff with `workspace_inventory_summary`, `discovered_paths`, `priority_paths`, and `skipped_paths_initial`.",
          guardrails: "Do not curate the final issue in this stage.",
        },
      }),
      agent({
        agentId: "curator-local-sources",
        displayName: "Curator Local Sources",
        role: "watcher",
        skills: ["research", "curation"],
        toolAllowlist: ["read"],
        prompt: {
          role: "You are a local-source curator for the newsletter workflow.",
          mission:
            "Read the prioritized local updates and capture the strongest story candidates and supporting facts.",
          inputs:
            "Use the upstream `source_inventory` handoff to decide what to read and extract the facts that make each item worth including.",
          outputContract:
            "Return a structured handoff with `read_paths`, `reviewed_facts`, `files_reviewed`, `files_not_reviewed`, and `citations_local`.",
          guardrails: "Only elevate items that are supported by actual file reads in this run.",
        },
      }),
      agent({
        agentId: "curator-external",
        displayName: "Curator External",
        role: "watcher",
        skills: ["research", "curation"],
        toolAllowlist: ["read", "websearch", "webfetch"],
        prompt: {
          role: "You are an external-news analyst for newsletter curation.",
          mission:
            "Gather timely external signals that complement the local updates and help determine what belongs in this week’s issue.",
          inputs:
            "Use the upstream `source_inventory` and `local_source_notes` handoffs to guide targeted external searches and page fetches.",
          outputContract:
            "Return a structured handoff with `external_research_mode`, `queries_attempted`, `sources_reviewed`, `citations_external`, and `research_limitations`.",
          guardrails:
            "If search is unavailable, capture that limitation and continue with the evidence already gathered.",
        },
      }),
      agent({
        agentId: "curator",
        displayName: "Curator",
        role: "watcher",
        skills: ["curation"],
        toolAllowlist: ["read"],
        prompt: {
          role: "You are a newsletter curator selecting stories and updates that deserve the audience's limited attention.",
          mission:
            "Choose the strongest mix of timely updates, useful insights, and product-relevant stories from the upstream evidence bundle.",
          inputs:
            "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs. Turn them into the final curated shortlist and section order for the issue.",
          outputContract:
            "Produce a shortlist with item summaries, why each matters now, the intended audience takeaway, and a recommended section order for the issue.",
          guardrails:
            "Prefer relevance, freshness, and distinctiveness over volume. Do not redo discovery or fresh web research in this stage.",
        },
      }),
      agent({
        agentId: "editor",
        displayName: "Editor",
        role: "worker",
        skills: ["writing", "editing"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are the newsletter editor responsible for turning curated inputs into a cohesive, highly readable issue.",
          mission:
            "Write a newsletter that feels timely, useful, and easy to scan while preserving a consistent editorial voice.",
          inputs:
            "Use the curated shortlist, publication tone guidance, prior issue patterns, and any desired structure or CTA rules.",
          outputContract:
            "Produce a full issue draft with subject line options, preview text, section transitions, concise commentary for each item, and a final CTA or reply prompt.",
          guardrails:
            "Keep the issue skimmable, specific, and audience-first. Avoid generic roundup filler, and make every section earn its place.",
        },
      }),
      agent({
        agentId: "copy-review",
        displayName: "Copy Review",
        role: "reviewer",
        skills: ["fact-checking", "copy-editing"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are the newsletter reviewer applying a copy-edit and fact-check pass before send.",
          mission:
            "Review the issue for claim accuracy, clarity, formatting, consistency, and whether each section answers 'why should I care?'",
          inputs:
            "Use the draft, curation notes, proof sources, and any editorial or compliance guidance.",
          outputContract:
            "Approve or return exact edits needed before send, grouped by claims, flow, clarity, formatting, and CTA quality.",
          guardrails:
            "Catch broken references, vague claims, weak transitions, and unsupported statements. Do not let filler or overlong sections through.",
        },
      }),
      agent({
        agentId: "publisher",
        displayName: "Publisher",
        role: "committer",
        skills: ["publishing"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are the newsletter publishing operator responsible for final packaging and send readiness.",
          mission:
            "Prepare the approved issue for the ESP or publishing workflow with all required metadata and preflight checks complete.",
          inputs:
            "Use the approved newsletter draft, publish metadata, audience segment details, and delivery platform requirements.",
          outputContract:
            "Produce the final issue package, send checklist, subject and preview confirmation, and any handoff notes needed for scheduling.",
          guardrails:
            "Do not send unrevised drafts, and flag anything missing that could break layout, tracking, or links.",
        },
      }),
    ],
    nodes: [
      node({
        nodeId: "curate-issue-discover",
        title: "Discover Issue Sources",
        agentId: "curator-discover",
        objective:
          "Identify the local source corpus and candidate files that should feed this week's issue.",
        dependsOn: [],
        inputRefs: [],
        stageKind: "research_discover",
        outputKind: "structured_json",
      }),
      node({
        nodeId: "curate-issue-local-sources",
        title: "Read Issue Sources",
        agentId: "curator-local-sources",
        objective:
          "Read the prioritized local source files and extract the strongest issue candidates.",
        dependsOn: ["curate-issue-discover"],
        inputRefs: [{ fromStepId: "curate-issue-discover", alias: "source_inventory" }],
        stageKind: "research_local_sources",
        outputKind: "structured_json",
      }),
      node({
        nodeId: "curate-issue-external-research",
        title: "Research Issue",
        agentId: "curator-external",
        objective: "Gather timely external signals that should influence this week's issue.",
        dependsOn: ["curate-issue-discover", "curate-issue-local-sources"],
        inputRefs: [
          { fromStepId: "curate-issue-discover", alias: "source_inventory" },
          { fromStepId: "curate-issue-local-sources", alias: "local_source_notes" },
        ],
        stageKind: "research_external_sources",
        outputKind: "structured_json",
      }),
      node({
        nodeId: "curate-issue",
        title: "Curate Issue",
        agentId: "curator",
        objective: "Curate the best items for this week's issue from the staged research handoffs.",
        dependsOn: [
          "curate-issue-discover",
          "curate-issue-local-sources",
          "curate-issue-external-research",
        ],
        inputRefs: [
          { fromStepId: "curate-issue-discover", alias: "source_inventory" },
          { fromStepId: "curate-issue-local-sources", alias: "local_source_notes" },
          { fromStepId: "curate-issue-external-research", alias: "external_research" },
        ],
        stageKind: "research_finalize",
        outputKind: "outline",
      }),
      node({
        nodeId: "draft-issue",
        title: "Draft Issue",
        agentId: "editor",
        objective: "Draft the newsletter issue from the curated outline.",
        dependsOn: ["curate-issue"],
        inputRefs: [{ fromStepId: "curate-issue", alias: "outline" }],
        outputKind: "draft",
      }),
      node({
        nodeId: "review-issue",
        title: "Review Issue",
        agentId: "copy-review",
        objective: "Review the newsletter draft before distribution.",
        dependsOn: ["draft-issue"],
        inputRefs: [{ fromStepId: "draft-issue", alias: "draft" }],
        outputKind: "review",
      }),
      node({
        nodeId: "publish-issue",
        title: "Publish Issue",
        agentId: "publisher",
        objective: "Prepare the issue for the sending platform and final approval.",
        dependsOn: ["review-issue"],
        inputRefs: [{ fromStepId: "review-issue", alias: "approved_issue" }],
        outputKind: "publish",
      }),
    ],
  },
  {
    id: "support-triage-team",
    name: "Support Triage Team",
    icon: "life-buoy",
    summary: "Classify support work, draft responses, review quality, and route action items.",
    description:
      "A support workflow for intake, response drafting, quality review, and escalation or publishing.",
    suggestedOutputs: ["triage-report.md", "response-draft.md", "escalation-summary.md"],
    agents: [
      agent({
        agentId: "intake",
        displayName: "Intake",
        role: "watcher",
        skills: ["triage"],
        toolAllowlist: ["read"],
        prompt: {
          role: "You are the support intake analyst.",
          mission: "Classify incoming issues by severity, category, and owner.",
          inputs: "Use the support thread, customer metadata, and routing rules.",
          outputContract: "Produce a triage summary with next-step recommendations.",
          guardrails: "Escalate safety, billing, and security risks immediately.",
        },
      }),
      agent({
        agentId: "responder",
        displayName: "Responder",
        role: "worker",
        skills: ["customer-support"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are the support response drafter.",
          mission: "Draft a helpful, accurate customer-facing response.",
          inputs: "Use the triage result, product knowledge, and approved response patterns.",
          outputContract: "Produce a response draft and any internal notes required.",
          guardrails: "Do not promise unavailable fixes or timelines.",
        },
      }),
      agent({
        agentId: "qa",
        displayName: "QA",
        role: "reviewer",
        skills: ["quality-review"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are the support QA reviewer.",
          mission: "Check the drafted response for clarity, policy fit, and tone.",
          inputs: "Use the triage summary and drafted response.",
          outputContract: "Approve or provide concrete revision guidance.",
          guardrails: "Catch risky language, policy errors, and missing action items.",
        },
      }),
      agent({
        agentId: "dispatcher",
        displayName: "Dispatcher",
        role: "committer",
        skills: ["operations"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are the support dispatcher.",
          mission: "Finalize the response or route the case to the proper team.",
          inputs: "Use the QA-reviewed response and routing rules.",
          outputContract: "Prepare the final reply or escalation summary with correct owner.",
          guardrails: "Do not close unresolved escalations.",
        },
      }),
    ],
    nodes: [
      node({
        nodeId: "triage-ticket",
        title: "Triage Ticket",
        agentId: "intake",
        objective: "Classify the issue and identify the right handling path.",
        dependsOn: [],
        inputRefs: [],
        outputKind: "triage",
      }),
      node({
        nodeId: "draft-response",
        title: "Draft Response",
        agentId: "responder",
        objective: "Draft the customer-facing response from the triage summary.",
        dependsOn: ["triage-ticket"],
        inputRefs: [{ fromStepId: "triage-ticket", alias: "triage" }],
        outputKind: "draft",
      }),
      node({
        nodeId: "review-response",
        title: "Review Response",
        agentId: "qa",
        objective: "Review the response for quality and policy fit.",
        dependsOn: ["draft-response"],
        inputRefs: [{ fromStepId: "draft-response", alias: "response_draft" }],
        outputKind: "review",
      }),
      node({
        nodeId: "route-case",
        title: "Route Case",
        agentId: "dispatcher",
        objective: "Finalize the approved response or route the case to an owner.",
        dependsOn: ["review-response"],
        inputRefs: [{ fromStepId: "review-response", alias: "approved_response" }],
        outputKind: "dispatch",
      }),
    ],
  },
  {
    id: "sales-prospecting-team",
    name: "Sales Prospecting Team",
    icon: "target",
    summary: "Research accounts, write outreach, review positioning, and prepare sending.",
    description:
      "A prospecting workflow that researches a target account, drafts outreach, reviews fit and claims, and prepares the outbound package.",
    suggestedOutputs: ["account-brief.md", "outreach-draft.md", "send-pack.md"],
    agents: [
      agent({
        agentId: "account-discover",
        displayName: "Account Discover",
        role: "watcher",
        skills: ["account-research"],
        toolAllowlist: ["read"],
        prompt: {
          role: "You are a prospecting source scout.",
          mission:
            "Identify the local account context, CRM notes, and source corpus that should be reviewed before outreach research begins.",
          inputs:
            "Inspect the workspace for account lists, CRM notes, ICP context, and prior research files, then prioritize the concrete sources to read next.",
          outputContract:
            "Return a structured handoff with `workspace_inventory_summary`, `discovered_paths`, `priority_paths`, and `skipped_paths_initial`.",
          guardrails: "Do not write the final account brief in this stage.",
        },
      }),
      agent({
        agentId: "account-local-sources",
        displayName: "Account Local Sources",
        role: "watcher",
        skills: ["account-research"],
        toolAllowlist: ["read"],
        prompt: {
          role: "You are a local account-context analyst.",
          mission:
            "Read the prioritized local account and ICP files and extract the most reliable personalization inputs.",
          inputs:
            "Use the upstream `source_inventory` handoff to choose concrete files to read and capture the facts that will anchor the account brief.",
          outputContract:
            "Return a structured handoff with `read_paths`, `reviewed_facts`, `files_reviewed`, `files_not_reviewed`, and `citations_local`.",
          guardrails: "Only cite files that were actually read in this run.",
        },
      }),
      agent({
        agentId: "account-external",
        displayName: "Account External",
        role: "watcher",
        skills: ["account-research"],
        toolAllowlist: ["read", "websearch", "webfetch"],
        prompt: {
          role: "You are an external account researcher focused on current buying context and public signals.",
          mission:
            "Gather current public context that can strengthen or disprove likely personalization hooks before outreach is drafted.",
          inputs:
            "Use the upstream `source_inventory` and `local_source_notes` handoffs to guide targeted external account research.",
          outputContract:
            "Return a structured handoff with `external_research_mode`, `queries_attempted`, `sources_reviewed`, `citations_external`, and `research_limitations`.",
          guardrails:
            "Do not invent buying signals. If search is unavailable, capture that limitation explicitly.",
        },
      }),
      agent({
        agentId: "account-research",
        displayName: "Account Research",
        role: "watcher",
        skills: ["account-research"],
        toolAllowlist: ["read"],
        prompt: {
          role: "You are an account researcher focused on finding real buying context and usable personalization hooks.",
          mission:
            "Turn the upstream source discovery, local account evidence, and external account research into the final account brief.",
          inputs:
            "Use the upstream `source_inventory`, `local_source_notes`, and `external_research` handoffs as the source of truth.",
          outputContract:
            "Produce a concise account brief with company context, likely priorities, buying signals, possible pain points, messaging angles, and high-confidence personalization hooks labeled by confidence.",
          guardrails:
            "Do not invent buying signals or pretend certainty. Separate observed facts from hypotheses and avoid re-running discovery or fresh web research in this stage.",
        },
      }),
      agent({
        agentId: "outreach-writer",
        displayName: "Outreach Writer",
        role: "worker",
        skills: ["sales-copy"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are an outbound copywriter who writes concise outreach anchored in relevance, not generic personalization theater.",
          mission:
            "Write outreach that connects the account context to a clear value hypothesis and a low-friction next step.",
          inputs:
            "Use the account brief, ICP, offer positioning, proof points, and any approved email or messaging guidelines.",
          outputContract:
            "Produce first-touch outreach with subject line, opening line, body, CTA, and one optional variant that tests a different angle or proof point.",
          guardrails:
            "Keep it personal, specific, and easy to reply to. Avoid generic hype, fake familiarity, and unsupported claims.",
        },
      }),
      agent({
        agentId: "positioning-review",
        displayName: "Positioning Review",
        role: "reviewer",
        skills: ["message-review"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are the outreach reviewer applying a quality and credibility check before anything is sent.",
          mission:
            "Review personalization, claims, tone, and CTA quality so outbound messages feel credible and worth replying to.",
          inputs:
            "Use the account brief, outreach draft, proof points, and any team messaging rules.",
          outputContract:
            "Approve or return revision notes focused on personalization quality, claim support, clarity, and CTA friction.",
          guardrails:
            "Reject fluff, weak personalization, false urgency, and unsupported claims. Prefer direct revision guidance over general criticism.",
        },
      }),
      agent({
        agentId: "send-prep",
        displayName: "Send Prep",
        role: "committer",
        skills: ["ops"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are the send-prep operator.",
          mission: "Prepare approved outreach for the outbound system or rep handoff.",
          inputs: "Use the approved message, account info, and sequencing metadata.",
          outputContract: "Produce a send-ready package with follow-up notes.",
          guardrails: "Do not alter approved claims without review.",
        },
      }),
    ],
    nodes: [
      node({
        nodeId: "research-account-discover",
        title: "Discover Account Sources",
        agentId: "account-discover",
        objective: "Identify the source corpus that should guide account research.",
        dependsOn: [],
        inputRefs: [],
        stageKind: "research_discover",
        outputKind: "structured_json",
      }),
      node({
        nodeId: "research-account-local-sources",
        title: "Read Account Sources",
        agentId: "account-local-sources",
        objective:
          "Read the prioritized local account and ICP files before drafting the account brief.",
        dependsOn: ["research-account-discover"],
        inputRefs: [{ fromStepId: "research-account-discover", alias: "source_inventory" }],
        stageKind: "research_local_sources",
        outputKind: "structured_json",
      }),
      node({
        nodeId: "research-account-external-research",
        title: "Research Account Externally",
        agentId: "account-external",
        objective:
          "Gather targeted external account context and buying signals to support the brief.",
        dependsOn: ["research-account-discover", "research-account-local-sources"],
        inputRefs: [
          { fromStepId: "research-account-discover", alias: "source_inventory" },
          { fromStepId: "research-account-local-sources", alias: "local_source_notes" },
        ],
        stageKind: "research_external_sources",
        outputKind: "structured_json",
      }),
      node({
        nodeId: "research-account",
        title: "Research Account",
        agentId: "account-research",
        objective:
          "Prepare the final account brief from the staged discovery, local evidence, and external research.",
        dependsOn: [
          "research-account-discover",
          "research-account-local-sources",
          "research-account-external-research",
        ],
        inputRefs: [
          { fromStepId: "research-account-discover", alias: "source_inventory" },
          { fromStepId: "research-account-local-sources", alias: "local_source_notes" },
          { fromStepId: "research-account-external-research", alias: "external_research" },
        ],
        stageKind: "research_finalize",
        outputKind: "brief",
      }),
      node({
        nodeId: "draft-outreach",
        title: "Draft Outreach",
        agentId: "outreach-writer",
        objective: "Draft outbound outreach from the account brief.",
        dependsOn: ["research-account"],
        inputRefs: [{ fromStepId: "research-account", alias: "account_brief" }],
        outputKind: "draft",
      }),
      node({
        nodeId: "review-outreach",
        title: "Review Outreach",
        agentId: "positioning-review",
        objective: "Review the outreach for fit, personalization, and quality.",
        dependsOn: ["draft-outreach"],
        inputRefs: [{ fromStepId: "draft-outreach", alias: "outreach_draft" }],
        outputKind: "review",
      }),
      node({
        nodeId: "prepare-send",
        title: "Prepare Send",
        agentId: "send-prep",
        objective: "Prepare the approved outreach for the sending workflow.",
        dependsOn: ["review-outreach"],
        inputRefs: [{ fromStepId: "review-outreach", alias: "approved_outreach" }],
        outputKind: "send_pack",
      }),
    ],
  },
  {
    id: "prd-to-launch-plan-team",
    name: "PRD to Launch Plan Team",
    icon: "rocket",
    summary: "Turn a product brief into an execution plan, review it, and prepare launch handoff.",
    description:
      "A cross-functional planning workflow that reads a PRD, drafts an execution plan, reviews risks, and prepares a launch-ready brief.",
    suggestedOutputs: ["execution-plan.md", "risk-review.md", "launch-brief.md"],
    agents: [
      agent({
        agentId: "planner",
        displayName: "Planner",
        role: "delegator",
        skills: ["planning"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are the implementation planner.",
          mission: "Convert the PRD into a realistic execution plan with milestones and owners.",
          inputs: "Use the PRD, known constraints, and delivery context.",
          outputContract: "Produce a step-by-step plan with dependencies and key open questions.",
          guardrails: "Call out assumptions and missing information explicitly.",
        },
      }),
      agent({
        agentId: "go-to-market",
        displayName: "Go To Market",
        role: "worker",
        skills: ["launch-planning"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are the launch planning writer.",
          mission: "Add launch, messaging, enablement, and rollout considerations to the plan.",
          inputs: "Use the execution plan and product context.",
          outputContract: "Produce the GTM and rollout sections required for launch readiness.",
          guardrails: "Keep dependencies and owner requests concrete.",
        },
      }),
      agent({
        agentId: "risk-review",
        displayName: "Risk Review",
        role: "reviewer",
        skills: ["risk-analysis"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are the delivery risk reviewer.",
          mission: "Review the plan for sequencing risks, unclear owners, and launch gaps.",
          inputs: "Use the implementation plan and GTM additions.",
          outputContract: "Approve or provide explicit risk and gap notes for revision.",
          guardrails: "Prioritize real execution blockers over stylistic concerns.",
        },
      }),
      agent({
        agentId: "launch-ops",
        displayName: "Launch Ops",
        role: "committer",
        skills: ["operations", "handoff"],
        toolAllowlist: ["read", "write"],
        prompt: {
          role: "You are the launch handoff operator.",
          mission: "Prepare the reviewed plan for team handoff and launch tracking.",
          inputs: "Use the approved plan and rollout notes.",
          outputContract: "Produce a launch brief with owners, timeline, and readiness checklist.",
          guardrails: "Do not hide unresolved risks; surface them clearly in the brief.",
        },
      }),
    ],
    nodes: [
      node({
        nodeId: "draft-plan",
        title: "Draft Plan",
        agentId: "planner",
        objective: "Draft the implementation plan from the PRD.",
        dependsOn: [],
        inputRefs: [],
        outputKind: "plan",
      }),
      node({
        nodeId: "add-rollout",
        title: "Add Rollout",
        agentId: "go-to-market",
        objective: "Add GTM and rollout planning details to the execution plan.",
        dependsOn: ["draft-plan"],
        inputRefs: [{ fromStepId: "draft-plan", alias: "implementation_plan" }],
        outputKind: "rollout",
      }),
      node({
        nodeId: "review-risks",
        title: "Review Risks",
        agentId: "risk-review",
        objective: "Review the plan for risk, sequencing, and readiness gaps.",
        dependsOn: ["add-rollout"],
        inputRefs: [{ fromStepId: "add-rollout", alias: "launch_plan" }],
        outputKind: "review",
      }),
      node({
        nodeId: "prepare-launch-brief",
        title: "Prepare Launch Brief",
        agentId: "launch-ops",
        objective: "Prepare the reviewed launch plan for cross-functional handoff.",
        dependsOn: ["review-risks"],
        inputRefs: [{ fromStepId: "review-risks", alias: "approved_plan" }],
        outputKind: "launch_brief",
      }),
    ],
  },
];

export function createWorkflowDraftFromTemplate(
  template: StudioTemplateDefinition,
  workspaceRoot = ""
): StudioWorkflowDraft {
  return {
    automationId: "",
    starterTemplateId: template.id,
    name: template.name,
    description: template.description,
    summary: template.summary,
    icon: template.icon,
    workspaceRoot,
    status: "draft",
    scheduleType: "manual",
    cronExpression: "",
    intervalSeconds: "3600",
    maxParallelAgents: "1",
    useSharedModel: false,
    sharedModelProvider: "",
    sharedModelId: "",
    outputTargets: [...template.suggestedOutputs],
    agents: template.agents.map((entry) => ({
      ...entry,
      skills: [...entry.skills],
      toolAllowlist: [...entry.toolAllowlist],
      toolDenylist: [...entry.toolDenylist],
      mcpAllowedServers: [...entry.mcpAllowedServers],
      prompt: { ...entry.prompt },
    })),
    nodes: template.nodes.map((entry) => ({
      ...entry,
      dependsOn: [...entry.dependsOn],
      inputRefs: entry.inputRefs.map((ref) => ({ ...ref })),
    })),
  };
}
