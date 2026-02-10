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
