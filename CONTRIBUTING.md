# Contributing to Tandem

Thank you for your interest in contributing to Tandem! This document provides guidelines and instructions for contributing.

## Code of Conduct

Please be respectful and constructive in all interactions. We're building something together.

## Getting Started

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
