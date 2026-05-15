# Security Policy

## Tandem Security Model

Tandem is built with a **zero-trust, security-first** architecture. This document outlines our security model and how to report vulnerabilities.

## Security Features

### 1. Encrypted Credential Storage

- API keys are encrypted using AES-256-GCM via SecureKeyStore
- Keys are never stored in plaintext or exposed to the frontend
- Master password derived from user input + machine ID

### 2. Sandboxed File Access

- AI can only access explicitly granted workspace folders
- Sensitive paths are permanently denied (`.env`, `.ssh`, `.gnupg`, `*.pem`, `*.key`)
- All file operations are validated before execution

### 3. Network Isolation

- Strict Content Security Policy (CSP)
- Only allowlisted endpoints can be contacted:
  - `127.0.0.1` (local sidecar)
  - `openrouter.ai`
  - `api.anthropic.com`
  - `api.openai.com`
  - User-configured custom endpoints

### 4. Supervised Agent Pattern

- AI sidecar is treated as untrusted
- All operations go through the Tool Proxy
- User approval required for write/delete operations
- Full operation journal with undo capability

### 5. Zero Telemetry

- No analytics or tracking
- No "call home" functionality
- All data stays on your device

### 6. Channel Attack Surface Controls

Slack, Discord, and Telegram adapters are treated as semi-trusted network
surfaces. Tandem applies defense-in-depth before a channel action can affect a
run, session, workspace, or configuration:

- Interaction endpoints validate the platform user and reject missing or
  unauthorized identities instead of defaulting to an anonymous actor.
- Approval and rework buttons require an `Approve`-or-higher channel user
  capability, backed by explicit enrollment records or the configured channel
  security profile.
- Slash commands are tiered as read, act, approve, or reconfigure, and the
  dispatcher refuses commands above the channel/user tier before execution.
- Reconfigure-tier slash commands require a fresh step-up confirmation from a
  second surface, currently a desktop-issued PIN typed into the chat within 5
  minutes.
- Channel-origin prompts and approval decisions are rate-limited per user, and
  outbound replies pass through redaction for common secrets and paths outside
  the pinned workspace.
- Channel-created sessions are pinned to a workspace boundary so file tools
  cannot read or write outside the session's assigned workspace.
- Audit streaming can export approval decisions, tool execution ledger events,
  and channel capability changes for external monitoring.

## Reporting a Vulnerability

We take security seriously. If you discover a security vulnerability, please report it responsibly.

### How to Report

1. **DO NOT** create a public GitHub issue for security vulnerabilities
2. Email security concerns to: [info@frumu.ai]
3. Include:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (if any)

### What to Expect

- Acknowledgment within 48 hours
- Regular updates on our progress
- Credit in security advisories (if desired)
- We aim to fix critical vulnerabilities within 7 days

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Security Best Practices for Users

1. **Keep Tandem updated** - We regularly release security patches
2. **Use strong API keys** - Rotate keys periodically
3. **Limit workspace scope** - Only grant access to folders you need
4. **Review permissions** - Check what operations you're approving
5. **Use local models** - For sensitive work, consider Ollama

## Architecture Security

```
┌─────────────────────────────────────────────────────────────┐
│ TRUST BOUNDARY 1: WebView Sandbox                           │
│ - No direct filesystem access                               │
│ - No direct network (except Tauri IPC)                      │
│ - CSP blocks external scripts                               │
├─────────────────────────────────────────────────────────────┤
│ TRUST BOUNDARY 2: Tauri Capabilities                        │
│ - IPC commands require explicit permission                  │
│ - Sensitive paths permanently denied                        │
├─────────────────────────────────────────────────────────────┤
│ TRUST BOUNDARY 3: Tool Proxy                                │
│ - ALL operations validated before execution                 │
│ - Path traversal attacks blocked                            │
│ - Rate limiting on operations                               │
├─────────────────────────────────────────────────────────────┤
│ TRUST BOUNDARY 4: Sidecar Process                           │
│ - Runs with minimal privileges                              │
│ - No direct file/network access                             │
│ - Communicates only via localhost IPC                       │
│ - Receives time-limited session tokens, not raw API keys    │
└─────────────────────────────────────────────────────────────┘
```

## Acknowledgments

We thank the security researchers who have helped improve Tandem's security.

---

_Last updated: January 2026_
