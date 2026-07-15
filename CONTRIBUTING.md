# Contributing to Tandem

Thank you for your interest in contributing to Tandem! This document provides guidelines and instructions for contributing.

## Code of Conduct

Please be respectful and constructive in all interactions. We're building something together.

## Licensing of Contributions and Developer Certificate of Origin

This is a mixed-license repository (see [docs/LICENSING.md](docs/LICENSING.md)).
By submitting a contribution you agree to the following terms, and CI enforces
the sign-off that records that agreement.

### Sign-off required (DCO)

Every commit must be signed off:

```bash
git commit -s
```

This adds a `Signed-off-by: Your Name <your@email>` trailer certifying the
[Developer Certificate of Origin 1.1](https://developercertificate.org/) —
that you wrote the contribution or otherwise have the right to submit it
under the license indicated in the file or package you are changing — **and**
that you agree to the contribution licensing terms in this section. Commits
without a matching sign-off fail the `DCO Sign-Off` CI check
(`scripts/check-dco.mjs`).

### Inbound license

Unless you explicitly state otherwise, your contribution is licensed under
the license of the package it modifies, as declared by that package's
manifest and package-local `LICENSE` file (`MIT OR Apache-2.0` for the
permissive packages, `BUSL-1.1` for the source-available packages).

### Additional terms for the source-available (BUSL-1.1) packages

The following packages are commercially licensed by Frumu LTD under the
Business Source License 1.1:

- `crates/tandem-plan-compiler`
- `crates/tandem-governance-engine`
- `crates/tandem-enterprise-server`
- `crates/tandem-incident-monitor`

By submitting a contribution that modifies any of these packages, you
additionally grant Frumu LTD a perpetual, worldwide, non-exclusive,
royalty-free, irrevocable right to use, reproduce, modify, distribute, and
sublicense your contribution under licenses of Frumu LTD's choosing,
including commercial licenses and future changes to the package license
parameters. You retain copyright in your contribution.

If you are not able or willing to grant these rights (for example, because
your employer owns your work and has not approved), please do not submit
changes to these packages — open an issue describing the change instead.

### Prerequisites

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://rustup.rs/) 1.75+
- [pnpm](https://pnpm.io/) (recommended) or npm

**Platform-specific:**

| Platform | Additional Requirements                                                        |
| -------- | ------------------------------------------------------------------------------ |
| Windows  | [Build Tools for Visual Studio](https://visualstudio.microsoft.com/downloads/) |
| macOS    | Xcode Command Line Tools (`xcode-select --install`)                            |
| Linux    | `webkit2gtk-4.1`, `libappindicator3`, `librsvg2`                               |

### Setup

1. Fork the repository
2. Clone your fork:
   ```bash
   git clone https://github.com/YOUR_USERNAME/tandem.git
   cd tandem
   ```
3. Install dependencies:
   ```bash
   pnpm install
   ```
4. Create the sidecar placeholder:
   ```bash
   pnpm run download-sidecar
   ```
5. Run in development mode:
   ```bash
   pnpm tauri dev
   ```

## Development Workflow

### Branch Naming

- `feature/description` - New features
- `fix/description` - Bug fixes
- `docs/description` - Documentation
- `refactor/description` - Code refactoring

### Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add OpenRouter provider support
fix: resolve permission dialog not showing
docs: update README installation steps
refactor: extract tool proxy into separate module
```

## Python Packs Standard

Some Starter Packs include Python templates. To keep installs safe and reproducible, Tandem enforces a workspace-scoped venv at `.opencode/.venv`.

Guidelines:

- Do not recommend global `pip install ...` in pack docs.
- For packs that include Python templates:
  - Prefer shipping a `requirements.txt` when dependencies are stable and modest.
  - Document install/run commands using the workspace venv.
- Keep dependencies minimal. Avoid pinning unless necessary. If pinning is required, explain why in the PR description.

Venv-safe commands to document:

### Windows

```bash
cd "<workspace>"
.opencode\.venv\Scripts\python.exe -m pip install -r requirements.txt
```

### macOS / Linux

```bash
cd "<workspace>"
.opencode/.venv/bin/python3 -m pip install -r requirements.txt
```

### Code Style

**TypeScript/React:**

```bash
pnpm lint        # Check for issues
pnpm format      # Auto-format code
```

**Rust:**

```bash
cargo fmt        # Format code
cargo clippy     # Lint
cargo test       # Run tests
```

### Testing

- Write tests for new features
- Ensure existing tests pass before submitting PR
- Test on multiple platforms if possible

## Pull Request Process

1. Create a feature branch from `main`
2. Make your changes
3. Run lints and tests
4. Update documentation if needed
5. Submit a PR with a clear description
6. Address review feedback

### PR Checklist

- [ ] Code follows project style guidelines
- [ ] Tests pass locally
- [ ] Documentation updated (if applicable)
- [ ] No secrets or sensitive data committed
- [ ] Commit messages follow conventions

### Generated / Automated Optimization PRs

Automated tools (e.g. Bolt/Jules) can open large numbers of small,
overlapping "performance" PRs — typically micro-optimizations like replacing
`[...arr].sort(...)[0]` with a `maxBy`/`minBy` helper, or consolidating
`useMemo` hooks. Left untriaged, these accumulate into a pile of stale,
duplicate, conflicting branches. To keep this from recurring:

**When a generated optimization PR is acceptable**

- It makes **one** focused change with a clear, measurable benefit.
- It is **rebased on current `main`** with **green CI** (not an old base SHA).
- It touches only the files relevant to the optimization — no unrelated
  server/Rust edits, lockfile churn, generated-catalog changes, `.patch`
  files, or tool-config files (e.g. `.jules/bolt.md`) bundled into a
  "performance" PR.
- A human owner has reviewed it. Generated PRs are **not** auto-merged.

**Prefer consolidation over a train of one-offs**

- If several generated branches make the *same* change to the *same* files,
  do **not** merge them one-by-one by age. Cherry-pick the worthwhile change
  **once** into a single intentional PR and close the rest as superseded.

**Auto-close criteria** (close with a concise comment explaining why)

- Duplicates or supersedes another open PR making the same change.
- Failing CI or based on a stale `main` with no recent updates.
- Bundles unrelated or server-touching changes into a micro-optimization PR
  (treat large or server-touching generated PRs as suspect until reviewed).

Keep this repo-hygiene triage separate from active feature/security work.

## Architecture Overview

```
tandem/
├── src/                    # React frontend
│   ├── assets/             # Static assets
│   ├── components/         # UI components
│   │   ├── ui/             # Base components (Button, Input, etc.)
│   │   ├── chat/           # Chat interface
│   │   ├── settings/       # Settings panel
│   │   ├── permissions/    # Permission dialogs
│   │   ├── orchestrate/    # Multi-agent orchestration UI
│   │   ├── ralph/          # Ralph loop UI
│   │   └── skills/         # Skills management UI
│   ├── contexts/           # React contexts and providers
│   ├── hooks/              # React hooks
│   ├── lib/                # Utilities and Tauri bindings
│   └── types/              # Shared TypeScript types
├── src-tauri/             # Rust backend
│   ├── src/
│   │   ├── commands.rs    # Tauri IPC commands
│   │   ├── sidecar.rs     # Sidecar lifecycle
│   │   ├── tool_proxy.rs  # File operation proxy
│   │   ├── llm_router.rs  # Provider routing
│   │   └── keystore.rs    # Secure storage
│   │   ├── memory/         # Vector memory and retrieval
│   │   ├── orchestrator/   # Multi-agent orchestration logic
│   │   ├── packs.rs        # Workspace pack handling
│   │   ├── presentation/   # PPTX export pipeline
│   │   ├── ralph/          # Ralph loop implementation
│   │   ├── skill_templates.rs # Skill template library
│   │   └── skills.rs       # Skills registry and loader
│   └── capabilities/      # Permission configuration
└── scripts/               # Build scripts
```

## Adding Skills (Developer Guide)

Tandem supports "skills" in two ways:

1. **Skill templates (starter skills)**: bundled with the app, listed in the UI as quick-install options.
2. **Installed skills**: user-installed skills (folder-scoped or global) that Tandem discovers at runtime.

This section explains how to add or update both safely.

### 1) Skill Templates (Bundled Starter Skills)

Skill templates live at:

- `src-tauri/resources/skill-templates/<skill-id>/SKILL.md`

These templates are listed via the Tauri command `skills_list_templates` (see `src-tauri/src/commands.rs`) which reads from `src-tauri/resources/skill-templates/` (see `src-tauri/src/skill_templates.rs`).

#### Create A New Skill Template

1. Create a new folder:
   - `src-tauri/resources/skill-templates/<skill-id>/`
2. Add a `SKILL.md` with YAML frontmatter and a body.

**Required YAML frontmatter fields**

```yaml
---
name: my-skill
description: What this skill does (short, user-facing)
---
```

**Optional YAML frontmatter fields**

```yaml
---
name: my-skill
description: What this skill does
requires: [python, node, bash] # Optional. Used only for UI "runtime" pills.
license: Optional. Human-readable or pointer to a LICENSE file.
compatibility: Optional. Notes like "Node 18+" etc.
metadata:
  author: Your Name
  category: writing
---
```

Notes:

- `name` must follow OpenCode rules (enforced in `src-tauri/src/skills.rs`): `^[a-z0-9]+(-[a-z0-9]+)*$` and 1-64 chars.
- `requires` is only a hint shown in the starter skill cards (bottom-right pills). It does not enforce anything.
- Tandem does not bundle Python/Node/etc. If you add `requires`, make sure the instructions remain useful for users who may not have that runtime installed.

#### Skill Template Content Guidelines

- Keep the first few paragraphs action-oriented: what it does, when to use it, what it produces.
- Prefer checklists and step-by-step workflows.
- Avoid requiring access to arbitrary filesystem paths outside what Tandem typically operates on.
- If you reference scripts inside a pack, be explicit about where they live and how to run them.

### 2) Installed Skills (User-Installed)

Installed skills are discovered from:

- **Folder**: `<workspace>/.opencode/skill/<skill-id>/SKILL.md`
- **Global**: `~/.config/opencode/skills/<skill-id>/SKILL.md`

The UI for importing/deleting lives under Extensions -> Skills, and the backend commands are:

- `list_skills` / `import_skill` / `delete_skill` in `src-tauri/src/commands.rs`

### Validation / QA Checklist

When you add a new skill template:

1. Run the app: `pnpm tauri dev`
2. Go to Extensions -> Skills and confirm the starter skill appears and installs correctly.
3. Confirm the Installed skills list updates after install (folder/global as expected).
4. If you added `requires`, confirm the runtime pills render on the starter skill card.
5. Watch logs for warnings like "Skipping invalid skill template ... Failed to parse frontmatter".

## Adding Themes (Developer Guide)

Themes are CSS variable palettes applied at runtime. The theme id is stored in user settings and applied to `document.documentElement`.

### 1) Add the Theme ID

Update the union in `src/types/theme.ts` with a new id (kebab-case with underscores, matching existing ids).

### 2) Add the Theme Definition

Add a new entry to `THEMES` in `src/lib/themes.ts`:

- id, name, description
- cssVars for all required tokens (colors, glass, fonts)

Use an existing theme as a template to ensure full coverage.

### 3) Light Theme Support (If Applicable)

If your theme is light, update `src/index.css` to opt into a light color scheme:

```
html[data-theme="your_theme_id"] {
  color-scheme: light;
}
```

### 4) Verify in the UI

1. Run the app: `pnpm tauri dev`
2. Open Settings -> Theme
3. Select your new theme and confirm it updates:
   - Background, surface, text, borders
   - Glass surfaces and hover states
   - Contrast and accessibility

### 5) Validation Checklist

- Theme appears in the Theme picker list
- All UI tokens render (no fallback colors)
- No unreadable text or low-contrast buttons
- Light themes set `color-scheme: light`

## Key Principles

1. **Security First** - All changes must maintain our zero-trust model
2. **Privacy Absolute** - No telemetry, no data collection
3. **User Control** - Users approve all significant operations
4. **Transparency** - Clear about what the AI is doing

## Areas for Contribution

- **Features** - New capabilities and integrations
- **UI/UX** - Improve the interface and experience
- **Documentation** - Tutorials, guides, API docs
- **Testing** - Increase test coverage
- **Performance** - Optimize speed and resource usage
- **Accessibility** - Make Tandem usable by everyone
- **Orchestration & Skills** - Improve multi-agent flows and skill templates
- **Memory & Artifacts** - Enhance vector memory, reports, and presentation outputs

## Questions?

- Open a [Discussion](https://github.com/YOUR_USERNAME/tandem/discussions) for questions
- Check existing [Issues](https://github.com/YOUR_USERNAME/tandem/issues) before creating new ones

---

Thank you for contributing to Tandem!
