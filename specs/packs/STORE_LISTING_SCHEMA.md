# Tandem Pack Store Listing Schema

## Scope

Defines marketplace listing payload generated from pack manifest + scanner outputs.
This schema is store/catalog metadata, not the on-disk `tandempack.yaml` itself.

## Listing Schema (JSON)

```json
{
  "schema_version": "1",
  "pack_id": "tpk_01JX8R5S9J6N0M3Y4Q2W7K1C1R",
  "name": "github-pr-workflow",
  "version": "1.0.0",
  "publisher": {
    "publisher_id": "pub_tandem_official",
    "display_name": "Tandem",
    "verification_tier": "official",
    "website": "https://tandem.frumu.ai/"
  },
  "listing": {
    "display_name": "GitHub PR Workflow",
    "description": "Issue triage and PR creation workflow.",
    "categories": ["developer-tools", "automation"],
    "tags": ["github", "workflow", "pr"],
    "license_spdx": "Apache-2.0",
    "icon_url": "https://cdn.example/icon.png",
    "screenshot_urls": ["https://cdn.example/shot-1.png"],
    "changelog_url": "https://cdn.example/changelog.md"
  },
  "distribution": {
    "download_url": "https://cdn.example/github-pr-workflow-1.0.0.zip",
    "sha256": "...",
    "size_bytes": 1234567,
    "signature_status": "valid"
  },
  "capabilities": {
    "required": ["github.create_branch", "github.create_pull_request", "github.list_issues"],
    "optional": ["slack.post_message"],
    "non_portable": []
  },
  "risk_summary": {
    "external_side_effects": true,
    "routine_count": 1,
    "path_scopes": ["workspace/**"],
    "domain_scopes": ["api.github.com", "slack.com"],
    "secrets_required": ["GITHUB_TOKEN"],
    "risk_level": "medium"
  },
  "portability": {
    "connector_agnostic": true,
    "notes": []
  },
  "commerce": {
    "pricing_model": "free",
    "currency": null,
    "price": null,
    "entitlement_required": false
  },
  "timestamps": {
    "published_at": "2026-03-02T00:00:00Z",
    "updated_at": "2026-03-02T00:00:00Z"
  }
}
```

## Required Fields

- core: `schema_version`, `pack_id`, `name`, `version`
- publisher: `publisher_id`, `display_name`, `verification_tier`
- listing: `display_name`, `description`, `categories`, `tags`, `license_spdx`
- distribution: `download_url`, `sha256`, `signature_status`
- capabilities: `required`, `optional`, `non_portable`
- risk summary: full object

## Enum Values

`verification_tier`:

- `unverified`
- `verified`
- `official`

`signature_status`:

- `missing`
- `valid`
- `invalid`
- `untrusted_key`
- `unsupported`

`pricing_model`:

- `free`
- `paid_one_time`
- `paid_subscription`
- `enterprise`

## Entitlement Model (Design-Only)

Marketplace controls download authorization; client installs standard zip payload.
Optional enterprise offline entitlement artifact may be provided as a separate signed token (`tandempack.license`).
