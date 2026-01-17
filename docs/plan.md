Act as a Senior Principal Software Architect and Security Lead. I want you to plan the build of a local-first, AI-powered desktop workspace application with a "Zero-Trust" approach to data handling.

## 1. Context & Reference Material
Please use the following resources as your "Ground Truth" for design and functionality, but apply a strict security overlay.

* **UX/Behavior Inspiration:** [Claude Cowork Research Preview](https://claude.com/blog/cowork-research-preview)
    * *Goal:* Mimic the "collaborative" feel where the AI has visibility into the work surface, but ensure this visibility is strictly local.
* **Technical Reference (Example Only):** [different-ai/openwork GitHub Repo](https://github.com/different-ai/openwork)
    * *Note:* This is an example of another project connecting OpenCode to a desktop UI. Tandem will have its **own unique architecture** using a Supervised Agent pattern that is architecturally superior - not a copy of openwork.

## 2. Project Goal
Build a native desktop application (Mac/Windows/Linux) that acts as an autonomous "AI Employee." The user selects a local working directory, and the AI can read/write files to complete tasks. **CRITICAL:** The application must be "Local-First" and "Privacy-Absolute." No telemetry, no hidden data collection, and API keys must be handled with banking-grade security.

## 3. Security & Privacy Standards (Non-Negotiable)
1.  **Zero Telemetry:** The app must NOT include any analytics (e.g., Google Analytics, Posthog) or "call home" functionality.
2.  **Secure Key Storage:** API Keys (OpenAI/Anthropic) must **NEVER** be stored in `localStorage`, `IndexedDB`, or plain text files. They must be stored in the OS Native Keychain (using `tauri-plugin-store` with encryption or native platform secure enclave bindings).
3.  **Strict Scoping:** The AI Sidecar must only have access to the specific directory the user explicitly grants. It cannot read outside that sandbox.
4.  **Network Isolation:** The app should only be allowed to communicate with:
    * The local Sidecar (localhost).
    * The specific LLM API endpoints (e.g., `api.openai.com`) user explicitly configures.
    * No other external traffic is permitted by default.

## 4. Technical Stack Constraints (Strict)
* **Framework:** Tauri v2 (Latest Release) — *Must use v2 `capabilities` system to enforce strict permissions.*
* **Backend/Core:** Rust (for the Tauri process).
* **Frontend:** React + TypeScript + Vite.
* **UI System:** Tailwind CSS + Shadcn/UI + Framer Motion.
* **AI Engine:** Bundled external AI agent binary (Sidecar pattern).

## 5. Key Features to Architect
1.  **Secure Sidecar Orchestration:** Spawn the AI engine locally. Ensure the port it listens on is not exposed to the wider network (bind to `127.0.0.1` strictly).
2.  **Visual Permission Model:** When the AI wants to read a file or browse a website, the UI must show a "Permission Request" toast that the user must Approve (like MacOS permissions).
3.  **"Glass" UI:** Fluid, optimistic UI updates that feel native and high-quality.
4.  **Streaming & Events:** Secure local streaming of the AI's thought process.

## 6. Your Output Requirements
Please generate a comprehensive **Technical Design Document (TDD)**:

### A. Security Architecture
- Detail how `tauri-plugin-stronghold` or OS Keychain will be used.
- Define the `Content-Security-Policy` (CSP) headers for the frontend.
- Explain how we prevent the sidecar process from becoming an orphan if the app crashes.

### B. Architecture Diagram
- A textual description or Mermaid chart showing the isolation layers: Frontend -> Tauri Core -> Secure Enclave -> Sidecar -> File System.

### C. The "Sidecar" Implementation Strategy
- Technical steps to bundle the binary in `tauri.conf.json` (v2).
- Configuration for the `capabilities` file to strictly limit what the frontend can invoke.

### D. Step-by-Step Build Plan (MVP)
- **Phase 1:** Security Foundation (Tauri v2 + Secure Storage implementation).
- **Phase 2:** Sidecar Integration (Spawning the "Brain" locally).
- **Phase 3:** The UI Layer (Shadcn + Visual Permissions).
- **Phase 4:** BYOK Integration (Connecting the pipes).

Start by confirming you understand the "Security-First" requirement and how it changes standard Tauri architecture.