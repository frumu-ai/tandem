# Tandem: System Context & Agent Guide

This document provides a comprehensive overview of the Tandem project for AI agents (like yourself) to understand the architecture, security model, and operational guidelines of the system.

## 1. Project Overview

**Tandem** is a local-first, privacy-absolute AI desktop workspace. It allows users to collaborate with an AI "coworker" that has direct access to local project files, but only under strict human supervision.

- **Platform**: Cross-platform (Windows, macOS, Linux) built with Tauri v2.
- **Goal**: Mimic the collaborative experience of "Claude Cowork" but with full platform support, zero lock-in, and absolute privacy.

## 2. Technical Architecture

### A. The Three-Layer Stack

1.  **Frontend (React + Vite)**: The "Glass" UI where the user chats, manages settings, and approves permissions. It communicates with the Tauri backend via `invoke` commands.
2.  **Backend (Tauri/Rust)**: The security engine. It handles:
    - **Encrypted Vault**: Storing API keys using AES-256-GCM (Stronghold).
    - **LLM Routing**: Abstracting different providers (Anthropic, OpenAI, etc.) into a unified local interface.
    - **Permission Proxy**: Intercepting agent requests and presenting them to the user for approval.
3.  **Sidecar (OpenCode)**: A bundled binary that acts as the "Autonomous Brain." It receives high-level tasks and generates specific tool calls (read file, edit file, run command).

### B. Security & Isolation

- **Zero Telemetry**: No analytics, no tracking, no cloud synchronization.
- **Network Scoping**: Traffic is restricted to the local sidecar and the user-configured LLM endpoints.
- **File Scoping**: The sidecar is strictly confined to directories explicitly granted by the user via the native file picker.

## 3. The Supervised Agent Pattern

Tandem uses a "Supervised Agent" architecture. The AI agent is treated as an untrusted but capable contractor:

- **Visibility**: The agent can see files in the workspace to gain context.
- **Intervention**: Any operation that modifies the system (writing a file, running a shell command) triggers a **Visual Permission Toast** in the UI.
- **Human-in-the-Loop**: The operation only executes if the user clicks "Approve."

## 4. LLM & Model Routing

Tandem is **provider-agnostic**. It uses a "Bring Your Own Key" (BYOK) model:

- **Cloud Providers**: OpenRouter (recommended), Anthropic, OpenAI.
- **Local Providers**: Ollama (for 100% air-gapped usage).
- **Custom**: Any OpenAI-compatible API endpoint.

The backend (`llm_router.rs`) handles formatting requests into the specific dialect required by each provider (e.g., converting OpenAI messages to Anthropic's system/messages format).

## 5. File System & Workspace

- **Workspaces**: Users add folders as "Projects."
- **Active Project**: Only one project is "active" at a time, meaning the agent's context is limited to that folder.
- **Exclusions**: System files and sensitive patterns (like `.env`) are typically hidden or blocked from agent access by default.

## 6. Guide for AI Agents Interacting with Tandem

If you are an agent tasked with modifying or explaining Tandem, keep these principles in mind:

1.  **Respect the Vault**: Never attempt to log, print, or bypass the encryption of API keys. They stay in the Rust `Stronghold` vault.
2.  **Privacy-First**: If you suggest a feature, ensure it doesn't introduce telemetry or external dependencies that leak data.
3.  **Tauri v2 Conventions**: Tandem uses Tauri v2. Use `capabilities` for permissions and follow the v2 plugin ecosystem patterns.
4.  **Local Context is King**: The user's primary value is "Chatting with their files." Ensure file-reading and context-gathering logic is robust and respects the scoped directory.
5.  **UI Fluidity**: The "Glass UI" (Framer Motion + Tailwind) is a core part of the experience. Modifications to the frontend should maintain this high-quality, native feel.

## 7. Key Files for Context

- `src-tauri/src/lib.rs`: Main entry point and command definitions.
- `src-tauri/src/llm_router.rs`: Logic for multi-provider support.
- `src-tauri/src/vault.rs`: Secure storage implementation.
- `src/lib/tauri.ts`: Frontend wrapper for backend commands.
- `src/hooks/useAppState.ts`: Global state management for the workspace.
