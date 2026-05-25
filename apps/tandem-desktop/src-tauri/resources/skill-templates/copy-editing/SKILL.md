---
name: copy-editing
description: Improve existing marketing copy through structured editing passes for clarity, voice, proof, specificity, and conversion impact. Use after drafting copy for pages, emails, and campaign assets. Produce deterministic review artifacts under scripts/marketing/<slug>/.
---

# Copy Editing

Apply a repeatable editing system without changing core intent.

## Required Inputs

- Campaign slug
- Source draft path or pasted draft
- Intended channel and audience
- Primary conversion action

## Workflow

1. Read `scripts/marketing/_shared/product-marketing-context.md` if present. If missing, initialize from `references/product-marketing-context-template.md` included with this skill.
2. Establish editing goals and non-negotiables.
3. Apply sequential sweeps: clarity, tone, "so what", proof, specificity, emotion, final polish.
4. Track each material edit with a reason code.
5. Validate that edits did not break core message or CTA.

## Output Files

- `scripts/marketing/<slug>/12-copy-edit-review.md`
- `scripts/marketing/<slug>/13-final-polished-copy.md`
- `scripts/marketing/<slug>/13-edit-log.csv`

## QA Checklist

- Each major edit includes rationale.
- Final copy preserves original intent while increasing clarity.
- Proof and specificity gaps are reduced or flagged.
- Tone matches context doc rules.

## Failure Modes

- Over-rewrite risk: preserve message architecture and only refine.
- Voice drift: compare against context terms and tone rules.
- Conversion drift: re-check CTA prominence and user action path.

## Next Skill Routing

- Run `social-content` for distribution derivatives.
- Run `email-sequence` for lifecycle adaptations.
