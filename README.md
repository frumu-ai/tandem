# Tandem

[![CI](https://github.com/frumu-ai/tandem/actions/workflows/ci.yml/badge.svg)](https://github.com/frumu-ai/tandem/actions/workflows/ci.yml)
[![Release](https://github.com/frumu-ai/tandem/actions/workflows/release.yml/badge.svg)](https://github.com/frumu-ai/tandem/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A local-first, privacy-focused AI workspace. Your AI coworker that runs entirely on your machine.

Inspired by [Claude Cowork](https://claude.com/blog/cowork-research-preview), but open source and provider-agnostic.

## Features

- **Zero telemetry** - No data leaves your machine except to your chosen LLM provider
- **Provider freedom** - Use OpenRouter, Anthropic, OpenAI, Ollama, or any OpenAI-compatible API
- **Secure by design** - API keys stored in encrypted vault, never in plaintext
- **Cross-platform** - Windows, macOS, and Linux from day one
- **Visual permissions** - Approve every file access and action
- **Full undo** - Rollback any AI operation with operation journaling

## Quick Start

### Prerequisites

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://rustup.rs/) 1.75+
- [pnpm](https://pnpm.io/) (recommended) or npm

**Platform-specific:**

| Platform | Additional Requirements                                                        |
| -------- | ------------------------------------------------------------------------------ |
| Windows  | [Build Tools for Visual Studio](https://visualstudio.microsoft.com/downloads/) |
| macOS    | Xcode Command Line Tools: `xcode-select --install`                             |
| Linux    | `webkit2gtk-4.1`, `libappindicator3`, `librsvg2`                               |

### Installation

1. **Clone the repository**

   ```bash
   git clone https://github.com/frumu-ai/tandem.git
   cd tandem
   ```

2. **Install dependencies**

   ```bash
   pnpm install
   ```

3. **Download the sidecar binary**

   ```bash
   pnpm run download-sidecar
   ```

   This fetches the OpenCode binary for your platform.

4. **Run in development mode**
   ```bash
   pnpm tauri dev
   ```

### Building for Production

```bash
# Build for current platform
pnpm tauri build

# Output locations:
# Windows: src-tauri/target/release/bundle/msi/
# macOS:   src-tauri/target/release/bundle/dmg/
# Linux:   src-tauri/target/release/bundle/appimage/
```

## Configuration

### Setting Up Your LLM Provider

1. Launch Tandem
2. Click the **Settings** icon (gear) in the sidebar
3. Choose your provider:
   - **OpenRouter** (recommended) - Get key at [openrouter.ai](https://openrouter.ai/keys)
   - **Anthropic** - Get key at [console.anthropic.com](https://console.anthropic.com/settings/keys)
   - **OpenAI** - Get key at [platform.openai.com](https://platform.openai.com/api-keys)
   - **Ollama** - No key needed, just run Ollama locally
4. Enter your API key (stored securely in encrypted vault)

### Granting Folder Access

Tandem can only access folders you explicitly grant:

1. Click **Select Workspace** in Settings
2. Choose a folder via the native file picker
3. Tandem can now read/write files in that folder only

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Tandem Desktop App                        │
├─────────────────┬───────────────────┬───────────────────────┤
│  React Frontend │   Tauri Core      │  OpenCode Sidecar     │
│  (WebView)      │   (Rust)          │  (AI Agent)           │
├─────────────────┴───────────────────┴───────────────────────┤
│                Stronghold Encrypted Vault                    │
└─────────────────────────────────────────────────────────────┘
```

### Supervised Agent Pattern

Tandem treats the AI as an "untrusted contractor":

- All operations go through a **Tool Proxy**
- Write operations require **user approval**
- Full **operation journal** with undo capability
- **Circuit breaker** for resilience

## Security

- **API keys**: Encrypted with AES-256-GCM in Stronghold vault
- **File access**: Scoped to user-selected directories only
- **Network**: Only connects to localhost + allowlisted LLM endpoints
- **No telemetry**: Zero analytics, zero tracking, zero call home

See [SECURITY.md](SECURITY.md) for our full security model.

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md).

```bash
# Run lints
pnpm lint

# Run tests
pnpm test
cargo test

# Format code
pnpm format
cargo fmt
```

## Project Structure

```
tandem/
├── src/                    # React frontend
│   ├── components/         # UI components
│   ├── hooks/              # React hooks
│   └── lib/                # Utilities
├── src-tauri/              # Rust backend
│   ├── src/                # Rust source
│   ├── capabilities/       # Permission config
│   └── binaries/           # Sidecar (gitignored)
├── scripts/                # Build scripts
└── docs/                   # Documentation
```

## Roadmap

- [x] Phase 1: Security Foundation
- [x] Phase 2: Sidecar Integration
- [x] Phase 3: Glass UI
- [x] Phase 4: BYOK Provider Routing
- [x] Phase 5: Agent Capabilities
- [ ] Browser integration
- [ ] Connectors & Skills
- [ ] Multi-workspace support

## License

[MIT](LICENSE) - Use it however you want.

## Acknowledgments

- [Anthropic](https://anthropic.com) for the Cowork inspiration
- [Tauri](https://tauri.app) for the secure desktop framework
- [OpenCode](https://opencode.ai/) for the awesome opensource cli
- The open source community

---

**Tandem** - Your local-first AI coworker.

---

_Note: This codebase communicates with the OpenCode sidecar binary for AI agent capabilities and routes to various LLM providers (OpenRouter, Anthropic, OpenAI, Ollama, or custom APIs). All communication stays local except for LLM provider API calls._
