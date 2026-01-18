# Tandem Marketing Materials & Launch Checklist

## 📊 Feature Comparison Table

Use this table in blog posts, README updates, or Product Hunt descriptions.

| Feature                | Tandem (Local-First)        | Claude Cowork            | SaaS AI (ChatGPT/Claude) | Other AI Agents         |
| :--------------------- | :-------------------------- | :----------------------- | :----------------------- | :---------------------- |
| **Data Privacy**       | 🛡️ Absolute (Local only)    | 🔐 Local (Mac only)      | ☁️ Cloud-stored          | ❓ Varies               |
| **Telemetry**          | 🚫 Zero                     | ❓ Unknown               | 📈 High (Tracking)       | 📈 Moderate             |
| **Cost**               | 💰 Pay-per-use (Pennies)    | 💸 Pro Subscription      | 💸 $20/mo Subscription   | 💸 Subscription/Paid    |
| **Platform Support**   | 💻 Win / Mac / Linux        | 🍎 macOS only            | 🌐 Web / Mac only        | 💻 Varies               |
| **Execution Safety**   | 📋 Plan Mode + Diff Review  | ✅ Permission-based      | 🚫 None                  | ⚠️ Automated (Risky)    |
| **Local Models**       | 🦙 Ollama Support           | 🚫 No                    | 🚫 No                    | ⚠️ Limited              |
| **Open Source**        | ✅ Yes (MIT)                | 🚫 No                    | 🚫 No                    | ❓ Varies               |
| **API Keys**           | 🏦 Encrypted Vault          | 🔐 Claude Account        | 🔐 SaaS Managed          | 📝 Often Plaintext      |
| **Provider Choice**    | 🔓 Any (BYOK)               | 🚫 Claude only           | 🚫 Provider-locked       | ⚠️ Limited              |

---

## 🚀 Product Hunt Launch Checklist

### 1. Preparation (T-Minus 7 Days)
- [ ] **High-Quality Screenshots:**
    - Dark mode "Glass UI" showing a full conversation.
    - Close-up of the **Execution Plan Panel** with a diff open.
    - **Settings Page** showing various providers (Ollama, OpenRouter).
    - **Visual Permission Toast** in action.
- [ ] **Teaser Video (30-60s):**
    - Show selecting a workspace folder.
    - Show toggling "Plan Mode".
    - Show the AI proposing changes and the user clicking "Execute".
- [ ] **Compelling Tagline:**
    - "Developer superpowers for everyone."
    - "The tools that changed how programmers work. Now for the rest of us."
    - "What Cursor did for developers, Tandem does for everyone."

### 2. Assets & Content
- [ ] **Maker Comment:** Prepare a story about *why* you built Tandem:
    - "We loved Claude Cowork but Windows/Linux users were left out."
    - "We wanted to credit the amazing OpenCode project with a GUI that anyone could use."
    - "Privacy + provider freedom = no vendor lock-in."
- [ ] **First Reviewers:** Reach out to friends/beta testers to leave honest reviews on launch day.
- [ ] **Social Media Graphics:** Square and 16:9 versions for X and LinkedIn.
- [ ] **Key Talking Points:**
    - **The Democratization Angle**: "What Cursor/Claude Code did for developers, Tandem does for everyone"
    - **Beyond Chat**: Not "upload a PDF and ask questions" - work with entire folders of files with full read/write capabilities
    - Position Plan Mode as the killer feature (batch review, just like developers get in their tools)
    - Credit OpenCode prominently

### 3. Launch Day (12:01 AM PST)
- [ ] **Post on X:** Use the "Hook" template from `marketing_posts.md`. Pin the tweet.
- [ ] **Post on Reddit:** 
    - Share to `r/LocalLLaMA` and `r/selfhosted` using the privacy-focused template.
    - Share to `r/rust` and `r/tauri` using the technical deep-dive template.
- [ ] **Hacker News:** Submit as "Show HN".
- [ ] **Monitor Comments:** Respond quickly to questions on all platforms.
- [ ] **Engagement Strategy:**
    - Acknowledge comparisons to Claude Cowork positively.
    - Emphasize you're not competing—you're bringing it to more platforms.
    - Credit OpenCode in every response that mentions "the agent."

### 4. Post-Launch
- [ ] **GitHub Update:** Add "Featured on Product Hunt" badge to README.
- [ ] **Follow-up:** Share launch results and "thank you" post on social media.

---

## 💡 Quick Hooks for Ads/Banners

- "Developer superpowers for everyone"
- "The tools that changed how programmers work. Now for the rest of us."
- "What Cursor did for developers, Tandem does for everyone"
- "AI that works with your files, not just your chats"
- "Claude Cowork for everyone (Windows, Linux, Mac)."
- "Built on OpenCode: Industrial AI agents in a local-first GUI."
- "Stop the $20/month subscription bleed."
- "Chat with your files, keep your secrets."
- "The AI agent that asks for permission."
- "Your code, your keys, your machine."
- "Plan Mode: Review every change before it happens."

---

## 🎯 Platform-Specific Content Tips

### Reddit Best Practices
- **r/LocalLLaMA**: Lead with Ollama support and local model performance.
- **r/selfhosted**: Emphasize zero telemetry and data sovereignty.
- **r/productivity**: Focus on use cases (documents, notes, research) rather than tech.
- **r/rust / r/tauri**: Deep technical dive, share architecture decisions.

### Hacker News Best Practices
- Be honest about limitations (e.g., "Still rough around the edges").
- Respond to every comment in the first 2 hours.
- If someone asks "Why not just use OpenCode CLI?": Answer with "Great for devs, but Tandem adds a GUI for non-technical users + visual permissions + batch operations."

### X (Twitter) Best Practices
- Use visuals (GIFs of the diff viewer, permission toasts).
- Tag @OpenCodeAI if they have a Twitter presence.
- Use #buildinpublic to attract indie maker community.
- Thread format works well—Post 1 (hook) → Post 2 (demo) → Post 3 (link).
