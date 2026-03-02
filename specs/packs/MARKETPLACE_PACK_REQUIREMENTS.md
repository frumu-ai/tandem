# Tandem Pack Marketplace Requirements

## Scope

This document extends Tandem Pack v1 with marketplace-ready metadata and validation rules.
All requirements are additive and backward-compatible with local zip installs.

## Marker and Detection Rule (Unchanged)

A zip is considered a Tandem Pack only when it contains `tandempack.yaml` at the zip root.
No other heuristic can trigger install.

## Core Identity and Versioning

Marketplace-ready packs use core identity fields at manifest top-level:

- `manifest_schema_version` (required)
- `pack_id` (required, immutable)
- `name` (required)
- `version` (required semver)

`pack_id` is stable across all versions of the same pack line, including internal and marketplace packs.

## Marketplace Metadata Block

Marketplace listing metadata is nested under `marketplace` and required for marketplace publishing, optional for local installs.

```yaml
marketplace:
  publisher:
    publisher_id: "pub_tandem_official"
    display_name: "Tandem"
    verification_tier: "official"
    website: "https://tandem.frumu.ai/"
    support: "support@tandem.ai"
  listing:
    display_name: "GitHub PR Workflow"
    description: "Issue triage and pull request creation workflow."
    categories: ["developer-tools", "automation"]
    tags: ["github", "workflow", "pr"]
    license_spdx: "Apache-2.0"
    icon: "resources/marketplace/icon.png"
    screenshots:
      - "resources/marketplace/shot-1.png"
    changelog: "resources/marketplace/CHANGELOG.md"
```

## Required vs Optional

Required for local install:

- `manifest_schema_version`
- `pack_id`
- `name`
- `version`
- `type`
- `engine.requires`
- `entrypoints`
- `capabilities.required`
- `contents`

Required for marketplace publication (in addition):

- `marketplace.publisher.publisher_id`
- `marketplace.publisher.display_name`
- `marketplace.publisher.verification_tier`
- `marketplace.listing.display_name`
- `marketplace.listing.description`
- `marketplace.listing.categories`
- `marketplace.listing.tags`
- `marketplace.listing.license_spdx`

Optional:

- icon/screenshot/changelog references
- signature file (`tandempack.sig`) depending on publisher tier policy

## Explicit Contents Validation

`contents` must explicitly list all installable entities so installer can validate completeness before extraction/registration.

```yaml
contents:
  skill_modules:
    - id: tandem.skill.github.core
      path: skill_modules/tandem.skill.github.core.yaml
  agent_presets:
    - id: tandem.agent.github_worker
      path: agent_presets/tandem.agent.github_worker.yaml
  automation_presets:
    - id: tandem.automation.github_pr_triage
      path: automation_presets/tandem.automation.github_pr_triage.yaml
  agents:
    - id: github_worker
      path: agents/github_worker.md
  missions:
    - id: pr_triage
      path: missions/pr_triage.yaml
  routines:
    - id: daily_sync
      path: routines/daily_sync.yaml
```

## Portability Rules

- Pack behavior contracts must reference capability IDs, not provider tool names.
- Provider-specific dependencies are allowed only when explicitly declared in `capabilities.non_portable`.
- Marketplace scans must flag non-portable dependencies in listing/risk output.

## Security and Validation Rules

Marketplace must reject packs when any of the following fail:

- marker file missing at root
- invalid manifest schema/version
- unsafe archive (traversal, symlink, zip bomb, bounds violations)
- embedded secrets detected
- required marketplace metadata missing
- invalid SPDX expression
- referenced assets missing

## Permission and Risk Summary Requirements

Client and marketplace must compute and display before install:

- required/optional/non-portable capabilities
- routine triggers and side-effect potential
- policy scopes (tool/path/domain where declared)
- required secrets placeholders

## Routine Enablement Policy

Pack routines install **disabled by default**.
Auto-enable is allowed only when:

- publisher/source is trusted, and
- user/org policy explicitly allows auto-enable.

## Marketplace-Ready Manifest Example

```yaml
manifest_schema_version: 1
pack_id: "tpk_01JX8R5S9J6N0M3Y4Q2W7K1C1R"
name: github-pr-workflow
version: 1.0.0
type: workflow
engine:
  requires: ">=0.9.0 <2.0.0"

marketplace:
  publisher:
    publisher_id: "pub_tandem_official"
    display_name: "Tandem"
    verification_tier: "official"
    website: "https://tandem.frumu.ai/"
    support: "support@tandem.ai"
  listing:
    display_name: "GitHub PR Workflow"
    description: "Issue triage and PR creation workflow with optional Slack notifications."
    categories: ["developer-tools", "automation"]
    tags: ["github", "workflow", "pr"]
    license_spdx: "Apache-2.0"
    icon: "resources/marketplace/icon.png"
    screenshots: ["resources/marketplace/shot-1.png"]
    changelog: "resources/marketplace/CHANGELOG.md"

entrypoints:
  automation_presets: ["tandem.automation.github_pr_triage"]
  missions: ["pr_triage"]
  routines: ["daily_sync"]

capabilities:
  required:
    - github.create_branch
    - github.create_pull_request
    - github.list_issues
  optional:
    - slack.post_message
  non_portable: []

secrets:
  required:
    - key: GITHUB_TOKEN
      description: "Token with repository write scope"

contents:
  automation_presets:
    - id: tandem.automation.github_pr_triage
      path: automation_presets/tandem.automation.github_pr_triage.yaml
  agent_presets:
    - id: tandem.agent.github_worker
      path: agent_presets/tandem.agent.github_worker.yaml
  missions:
    - id: pr_triage
      path: missions/pr_triage.yaml
  routines:
    - id: daily_sync
      path: routines/daily_sync.yaml
```
