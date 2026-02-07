# Tandem: System Context & Agent Guide

This document provides a comprehensive overview of the Tandem project for AI agents (like yourself) to understand the architecture, security model, and operational guidelines of the system.

## 1. Project Overview

**Tandem** is a local-first, privacy-absolute AI desktop workspace. It allows users to collaborate with an AI "coworker" that has direct access to local project files, but only under strict human supervision.

- **Platform**: Cross-platform (Windows, macOS, Linux) built with Tauri v2.
- **Goal**: Mimic the collaborative experience of an AI coworking tool but with full platform support, zero lock-in, and absolute privacy.

## 2. Technical Architecture

### A. The Three-Layer Stack

1.  **Frontend (React + Vite)**: The "Glass" UI built with Tailwind CSS and Framer Motion. It handles the chat interface, file exploration, settings, and visual permission management.
2.  **Backend (Tauri/Rust)**: The security and orchestration engine. Key responsibilities:
    - **Encrypted Vault**: Storing API keys securely using AES-256-GCM encryption.
    - **LLM Routing**: Abstracting various providers (OpenRouter, Anthropic, OpenAI, Ollama) into a unified interface.
    - **Sidecar Management**: Downloading, verifying, and orchestrating the OpenCode binary.
    - **Permission Proxy**: Intercepting and presenting tool requests to the user.
    - **Operation Journaling**: Recording every file and command operation for audit and undo capability.
3.  **Sidecar (OpenCode)**: A bundled binary that acts as the "Autonomous Brain." It receives tasks and generates tool calls (read_file, write_file, run_command, etc.).

### B. Security & Isolation

- **Zero Telemetry**: No analytics, tracking, or cloud sync.
- **Network Scoping**: Traffic is restricted to local sidecar communication and user-configured LLM endpoints.
- **File Scoping**: The sidecar is strictly confined to directories explicitly granted by the user via the native file picker.

## 3. Agent Modes & Capabilities

Tandem offers specialized "Agent Modes" to handle different types of tasks:

- **Immediate Mode**: The default experience. AI operations are executed one-by-one, each requiring individual user approval via toast notifications.
- **Plan Mode**: The recommended workflow for complex tasks. AI proposes a batch of operations that are **staged** in the Execution Plan panel. Users review all diffs before executing the entire plan at once.
- **Coder Mode**: Optimized for software engineering tasks with specialized system prompts for code generation and refactoring.
- **Ask (General) Mode**: A non-intrusive Q&A mode that ignores file-writing tools, perfect for research and exploration.
- **Explore Mode**: Focused on codebase analysis and semantic search.

## 4. Execution Planning & Staging Area

Tandem implements a "Git-like" staging area for AI operations in **Plan Mode**:

1.  **Interception**: Tool calls (like `write_file`) are intercepted and held in a `DraftStore`.
2.  **Visualization**: Proposed changes appear in the **Execution Plan Panel** at the bottom-right.
3.  **Review**: Users can see side-by-side diffs of proposed changes using the `DiffViewer`.
4.  **Execution**: Staged operations are applied as a batch only after explicit user confirmation.
5.  **Rollback**: The entire batch can be undone via the Operation Journal.

## 5. File System & Context Management

- **File Browser**: A native-feel sidebar allows users to explore their workspace and quickly open files.
- **Context Attachment**: Users can explicitly attach files or entire folders to their chat messages to provide the AI with specific context.
- **Permissions**: Tandem uses a "Visual Permission" model where every read or write request is visible to the user.

## 6. Guide for AI Agents Interacting with Tandem

If you are an agent tasked with modifying or explaining Tandem, keep these principles in mind:

1.  **Respect the Vault**: Never attempt to log or bypass the encryption of API keys.
2.  **Privacy-First**: Suggestions must not introduce telemetry or leak user data.
3.  **Tauri v2 Conventions**: Follow the `capabilities` system and v2 plugin patterns.
4.  **Glass UI Consistency**: Maintain the high-quality, fluid feel (Framer Motion + Tailwind).
5.  **Supervised Pattern**: Always ensure modifications respect the "User is the Boss" philosophy.

## 7. Key Files for Context

- `src-tauri/src/lib.rs`: Main entry point and command definitions.
- `src-tauri/src/llm_router.rs`: Multi-provider routing logic.
- `src-tauri/src/sidecar_manager.rs`: Binary lifecycle and versioning.
- `src/hooks/useStagingArea.ts`: Staging area and planning state.
- `src/components/chat/AgentSelector.tsx`: Agent mode definitions.
- `src/components/files/FileBrowser.tsx`: Workspace exploration UI.
