# Tandem Marketing Post Templates

Use these templates as a starting point for promoting Tandem on various platforms.

---

## 📍 Platform: Reddit (r/LocalLLaMA, r/selfhosted, r/privacy)

**Title Options:**
- [Show Reddit] Tandem: A private, local-first AI coworker for Windows, Linux, and MacOS
- Tired of $20/month AI subscriptions? Try Tandem – open-source, BYOK, and zero telemetry.
- I built a "Claude Cowork" alternative that runs 100% on your machine.

**Body:**
Hi everyone,

I’ve been working on **Tandem**, a local-first AI workspace designed for people who want the power of AI agents without the privacy trade-offs or monthly subscriptions.

Think of it as a supervised AI coworker that lives on your desktop. It's built on top of the excellent **OpenCode** engine, giving you industrial-grade agent capabilities in a secure, cross-platform GUI.

**Key Features:**
- 🛡️ **Zero Telemetry:** No analytics, no tracking. Your data stays on your machine.
- 🔓 **Provider Freedom:** Use OpenRouter, Anthropic, OpenAI, or run 100% locally with Ollama.
- 💰 **Pay-As-You-Go:** Connect your own API keys. Stop paying for subscriptions you don't use.
- 📋 **Plan Mode:** The killer feature—review multi-step AI operations in a side-by-side diff viewer before they touch your files. Batch approve everything at once.
- ⏪ **Full Undo:** Everything is journaled. If the AI makes a mistake, roll it back in one click.
- 🪟 **Cross-Platform:** Beautiful "Glass UI" built with Tauri v2, available for Windows, Linux, and MacOS.

We were inspired by the **Claude Cowork** research preview from Anthropic. We loved the collaborative feel but wanted something that was open-source, provider-agnostic, and worked for everyone—not just macOS users.

**Check it out here:** [https://github.com/frumu-ai/tandem](https://github.com/frumu-ai/tandem)

Would love to hear your thoughts on the "Supervised Agent" approach!

---

## 📍 Platform: Reddit (r/productivity)

**Title Options:**
- [Tool] I built an AI assistant that works with your entire folder of documents (privacy-focused, runs locally)
- Organize 100+ files into one workspace and chat with all of them at once
- AI coworker for your documents—zero cloud uploads, zero subscriptions

**Body:**
Hey r/productivity,

I wanted to share **Tandem**, a tool I built to help organize and work with large collections of documents without uploading anything to the cloud.

**The Problem:**
If you work with lots of files (research papers, meeting notes, contracts, book chapters), it's hard to keep context across everything. Tools like ChatGPT make you upload files one-by-one and your data goes to their servers.

**How Tandem Works:**
1. Point it at a folder on your computer (your "workspace").
2. Ask questions like "What are the common themes in these 50 documents?" or "Find inconsistencies between these contracts."
3. The AI can read/write files, but **only with your permission**. You review every change before it happens.

**Why it's different:**
- ✅ **Privacy:** Your files never leave your computer.
- ✅ **Cost:** Use your own API key with OpenRouter (pay pennies per query) or run 100% locally with Ollama.
- ✅ **Plan Mode:** See all proposed changes in a visual diff viewer before approving.

Built on the open-source OpenCode engine with a desktop UI that works on Windows, Mac, and Linux.

**Repo:** [https://github.com/frumu-ai/tandem](https://github.com/frumu-ai/tandem)

Happy to answer questions!

---

## 📍 Platform: Reddit (r/rust, r/tauri, r/opensource)

**Title Options:**
- [Show Reddit] Tandem: A Tauri v2 desktop app wrapping OpenCode with a secure permission proxy
- Built a local-first AI workspace with Rust + Tauri v2 + OpenCode
- Open-source AI agent GUI with AES-256-GCM vault and operation journaling

**Body:**
Hi everyone,

I've been working on **Tandem**, an open-source desktop application that provides a secure GUI for the OpenCode AI agent.

**Technical Stack:**
- **Frontend:** React + Vite + Tailwind (the "Glass UI")
- **Backend:** Rust via Tauri v2
- **Security:** AES-256-GCM encrypted vault for API keys, permission proxy for all file/command operations
- **Sidecar:** OpenCode binary orchestration with process lifecycle management

**Architecture Highlights:**
- **Supervised Agent Pattern:** Every write operation goes through a permission proxy. Users can approve individually (Immediate Mode) or batch-review in a staging area (Plan Mode).
- **Operation Journal:** Full undo support via journaled operations. If the AI makes a mistake, rollback in one click.
- **Provider Agnostic:** Routes to OpenRouter, Anthropic, OpenAI, or Ollama via a unified interface.
- **Cross-Platform:** Single codebase for Windows, macOS, and Linux using Tauri v2's capabilities system.

**Why we built it:**
We loved Anthropic's Claude Cowork but wanted something open-source and cross-platform. The goal was to wrap OpenCode's powerful CLI in a desktop app that non-technical users could trust (hence the heavy focus on visual permissions and safety).

**Repo:** [https://github.com/frumu-ai/tandem](https://github.com/frumu-ai/tandem)

Feedback on the architecture and security model is very welcome!

---

## 📍 Platform: Hacker News (Show HN)

**Title:** Show HN: Tandem – A local-first, privacy-absolute AI workspace built with Tauri v2

**Body:**
Tandem is an open-source desktop application that provides a secure environment for AI agents to interact with local files.

**The Architecture:**
- **Frontend:** React + Vite + Tailwind (The "Glass" UI).
- **Backend:** Rust (Tauri v2) handling the secure vault (AES-256-GCM), provider routing, and permission proxy.
- **Sidecar:** Orchestrates the OpenCode agent binary locally.

**Why we built it:**
We were heavily inspired by **Anthropic's Claude Cowork** research preview. We wanted to bring that high-context, collaborative AI experience to all platforms (Windows/Linux/Mac) while maintaining 100% data sovereignty. 

Existing AI tools often require uploading entire codebases or sensitive documents to the cloud. Tandem keeps the "thinking" between you and your provider, with zero telemetry or tracking. We use a "Supervised Agent" pattern where every file write or command execution requires visual approval or is staged in a batch "Execution Plan" for review.

**Repo:** [https://github.com/frumu-ai/tandem](https://github.com/frumu-ai/tandem)

---

## 📍 Platform: X (Twitter)

**Post 1 (The Hook):**
Stop paying the "Privacy Tax" for AI. 🛡️

Introducing Tandem: The open-source, cross-platform alternative to Claude Cowork. Built on OpenCode, running 100% on your machine.

✅ Zero Telemetry
✅ BYO API Keys (OpenRouter, Anthropic, Ollama)
✅ Win / Linux / Mac
✅ Full Undo & Planning

[Link to Repo] #buildinpublic #AI #Privacy #OpenCode

**Post 2 (The Visuals - Quote Tweet with Video/GIF):**
The "Plan Mode" in Tandem is a game-changer. 🚀

Powered by OpenCode's native Plan agent—see all proposed file changes in a diff viewer, remove what you don't want, then execute the entire batch.

No more watching an AI go rogue on your codebase.

[GIF of Diff Viewer / Execution Plan Panel]

**Post 3 (The Economics):**
Subscription bleed is real. 💸

Why pay $20/month for ChatGPT/Claude when you can use Tandem with OpenRouter and pay pennies for what you actually use?

Or go 100% local with Ollama. 🦙

Built on OpenCode. Available for Windows, Linux, Mac.

#SelfHosted #OpenSource #TandemAI

**Post 4 (The Claude Cowork Comparison):**
Remember when Claude Cowork dropped and Windows/Linux users were left out? 😅

We built Tandem to fix that. Same collaborative AI vibe, but:
✅ Open-source (MIT)
✅ All platforms
✅ Built on OpenCode
✅ BYOK (Ollama, OpenRouter, etc)

[Link] #AI #OpenSource

---

## 📍 Platform: X (Twitter) - Democratization Angle

**Post 1:**
In 2024, AI coding tools changed everything for developers.

Cursor, Claude Code, Copilot - they can read your entire codebase, understand context, and make changes across hundreds of files.

But why should only programmers have these superpowers?

**Post 2:**
Writers need to work with 50-chapter manuscripts.
Researchers need to synthesize 200 papers.
Analysts need to cross-reference quarterly reports.

They deserve the SAME tools.

**Post 3:**
That's why we built Tandem.

Same capabilities that transformed programming:
- Folder-wide context (not just one file at a time)
- Multi-step operations with review
- Full undo for everything
- Your files never leave your computer

For everyone. Not just devs.

[Link] #buildinpublic #AI

---

## 📍 Platform: Reddit (r/productivity, r/ChatGPT, r/artificial)

**Title:** Developers have had AI superpowers for a year. Why shouldn't everyone else?

**Body:**
If you're a programmer, you've probably heard of tools like Cursor, Claude Code, or Copilot. They're transformative - AI that understands your entire project, makes changes across hundreds of files, and lets you review everything before it happens.

But here's what struck me: non-programmers have the EXACT same needs.

- A researcher with 200 PDFs needs cross-document synthesis
- A writer with a 50-chapter manuscript needs consistency checking  
- An analyst with quarterly reports needs pattern recognition

Why should these people be stuck with "upload one file at a time" tools like ChatGPT?

That's why we built Tandem. It's essentially the same capabilities that changed how developers work, but wrapped in an interface anyone can use:

- Point it at a folder (your "workspace")
- AI reads and understands everything
- Ask for changes - it proposes them
- You review in a visual diff view
- Approve what you want, reject what you don't

It's open-source, runs locally (your files never leave your computer), and you can use whatever AI provider you want.

**Repo:** [https://github.com/frumu-ai/tandem](https://github.com/frumu-ai/tandem)

Happy to answer questions!

