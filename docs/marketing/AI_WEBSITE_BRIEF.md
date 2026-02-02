# Tandem Product Brief for Website Content

**Target Audience:** AI Agent / Content generator for [frumu.ai](https://frumu.ai/)
**Product Name:** Tandem
**Product URL:** [https://tandem.frumu.ai/](https://tandem.frumu.ai/)

## Executive Summary

Tandem is a **local-first, privacy-focused AI workspace** designed to bring the power of AI coding tools (like Cursor/Claude Dev) to _general_ file management and project work. It runs entirely on the user's desktop (Windows, macOS, Linux), ensuring that sensitive data never leaves their machine unless explicitly sent to an LLM provider of their choice.

## Core Value Proposition

1.  **Privacy & sovereignty:** Users own their data. No hidden telemetry, no cloud storage of their files, no vendor lock-in.
2.  **Safety (Zero-Trust):** The AI is treated as an "untrusted contractor." Every file read/write is gated by a robust permission system.
3.  **Agentic Power:** Beyond simple chat, Tandem features autonomous agents ("Ralph Loop", "Plan Mode") that can plan complex architectures or iterate on tasks until completion.

## "Why It's Cool" (Differentiators)

- **Vector Memory on Desktop:** It uses `sqlite-vec` to create a semantic memory of your project locally. It "remembers" your codebase without uploading it to a vector cloud.
- **The "Ralph Loop":** Most AI tools chat once and stop. Tandem has a "Ralph Mode" that autonomously iterates, self-corrects, and validates its own work until the task is done, tracking file changes via Git.
- **Bring Your Own Key (BYOK):** It's provider-agnostic. Use OpenRouter, Anthropic, OpenAI, or even **local models (Ollama)** for specific tasks.
- **Not Just for Code:** While built with coding tech, it's designed for _any_ text-based workflow—marketing plans, legal docs, research summaries.

## Key Features Breakdown

### 1. Workflows & Agents

- **Chat Mode:** Standard context-aware chat with your files.
- **Plan Mode:** The AI drafts a comprehensive Markdown plan (`.md`) first. The user reviews the plan, and only then does the AI execute the changes.
- **Ralph Loop:** An autonomous agent loop that executes, verifies, and iterates until a completion token (`<promise>COMPLETE</promise>`) is met.
- **Canvas:** Renders interactive HTML/JS dashboards and reports directly in the chat stream.

### 2. Security & Architecture

- **Zero-Trust File System:** The AI cannot touch files outside the user-selected workspace.
- **Encrypted Vault:** API keys are stored in the OS native keychain (AES-256-GCM), never plain text.
- **Tauri + Rust:** Built on a hyper-performant, secure Rust backend (Tauri v2) with a React frontend. fast, light, and native.

### 3. Extensibility

- **Skills System:** Users can import "Skills" (markdown files with instructions) to teach the AI new capabilities or domain-specific knowledge.
- **Memory:** Semantic search retrieval augmented generation (RAG) running locally.

## Visual Identity

- **Aesthetic:** "Glassmorphism," clean, modern, dark-mode first.
- **Typography:** Rubik 900 (Bold/Industrial) for headers, Inter for UI.
- **Vibe:** Professional, secure, futuristic but grounded.

## Technical Keywords (for SEO/Context)

- Local-first AI
- Privacy-focused LLM client
- RAG (Retrieval Augmented Generation)
- Autonomous AI Agents
- Tauri App
- Rust
- Ollama Client
- OpenCode Sidecar
