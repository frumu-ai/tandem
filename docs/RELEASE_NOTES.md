# Tandem v0.1.7 Release Notes

## Highlights

- **Model Selection UX**: The provider/model selector has been streamlined to prioritize OpenCode Zen and Ollama, while automatically hiding unconfigured providers to reduce list noise.
- **Instant Settings Sync**: Changing your preferred model or provider in Settings now updates the Chat interface immediately without requiring a refresh.
- **Mid-Stream Permissions**: The "Allow All" button is now unblocked during generation, allowing you to quickly authorize complex tasks (like multi-file operations) while the AI is streaming, preventing repetitive approval loops.
- **Robust Session Visibility**: Removed strict project path filtering that could hide chat sessions when using the sidecar in manual/developer modes. Your history is now always visible.
- **HTML Slide Improvements**: Enhanced PDF export fidelity for slides and improved the flexible feedback loop during presentation planning.

## Notes

- This update focuses heavily on refining the "first-run" and "configured" experience, removing friction when selecting models and authorizing actions.

---

# Tandem v0.1.6 Release Notes

## Highlights

- **Canvas Feature**: Securely render LLM-generated HTML reports and dashboards in a sandboxed iframe. Built-in support for Tailwind CSS, Chart.js, and Font Awesome via CDN allows the AI to create rich, interactive visual artifacts directly in the preview.
- **Improved Web Research**: Introduced a dedicated "Research" tool category. The AI now follows a smarter "Search → Select → Fetch" workflow, reducing failures and avoiding anti-bot blocks on popular sites.
- **Reasoning & Tool Visibility**: AI "thinking" steps (reasoning) are now visible in real-time and saved in chat history. Technical tool calls are no longer hidden, providing full transparency into the AI's internal logic and progress.
- **Robust Stop Command**: Fixed a critical issue where the "Stop" button failed to halt the backend process. Implemented a fallback cancellation mechanism to ensure provider API calls are terminated immediately, preventing credit waste.
- **Session Persistence**: Your active chat session now survives application reloads and rebuilds. No more losing your place or accidentally creating "New Chat" sessions on refresh.
- **Dynamic Versioning**: Hardcoded version numbers have been removed. The loading screen, splash screen, and settings now dynamically display the actual version of Tandem and the OpenCode sidecar.
- **Optimized Permissions**: Safe, non-destructive tools (listing files, reading code, searching the web) are now allowed by default. This reduces permission fatigue while keeping destructive actions (writing, deleting, commands) securely gated.

## Notes

- The Canvas feature is most reliable in **Plan Mode**, allowing you to review the proposed report structure before the AI writes the HTML file.
- If a web fetch fails due to bot detection, the AI is now instructed to pivot to alternative sources rather than retrying the same blocked URL.

---

# Tandem v0.1.5 Release Notes

## Highlights

- **OpenCode API Compatibility**: Sidecar requests now align with the latest OpenCode routes, restoring provider/model listings and prompt delivery after upstream changes.
- **Error Surfacing**: Structured provider errors (like OpenRouter credit limits) are now displayed directly in the chat UI with improved extraction of specific error reasons.
- **Permission Prompts**: Tool permission requests are displayed correctly again after upstream event schema changes.
- **Transient Tool UI**: Technical background tasks (file reads, edits, etc.) now appear briefly in the chat and clear automatically on success, keeping the conversation focused.
- **Reliable Responses**: Improved the synchronization of final message parts and added a history backfill mechanism to ensure responses appear correctly without needing to switch sessions.
- **Log Spam Reduction**: Terminal output is now significantly quieter, summarizing large data payloads and hiding routine background activity.
- **Settings Update UX**: Update checking and download/install progress is now surfaced at the top of Settings.
- **UI Fixes + Polish**: The taskbar no longer overlays the About/Settings screens, the theme picker is now a compact dropdown with previews, and the active provider/model badge appears next to the tool selector.
- **Allow-All Mode**: New chats can opt into an “Allow all tools” mode to skip per-tool approval prompts.

## Notes

- If you see OpenRouter credit-limit errors, add credits or lower max output tokens for the selected model.
- AppImage installs are unchanged and continue to update via the AppImage artifact.

---

# Tandem v0.1.1 Release Notes

## Highlights

- **Enhanced Image Handling**: Implemented automatic 1024px resizing and JPEG compression, reducing image token size by up to 90%.
- **Linux Platform Support**: Resolved clipboard image pasting issues on Linux using a native Tauri plugin fallback.
- **Improved Reliability**: Images now persist across session reloads and are compatible with all vision-capable models via Markdown inlining.

## Complete Feature List

### Image Pipeline (Optimization update)

- **Smart Compression**: Pasted images are automatically converted to JPEG (0.8 quality) to minimize base64 payload size.
- **Auto-Resizing**: Images larger than 1024px are capped while preserving aspect ratio, preventing "max tokens" errors in long chats.
- **Responsive Previews**: Added CSS constraints for inlined images to ensure they display correctly within the chat UI.

### Platform Compatibility

- **Linux Clipboard**: Integration with `@tauri-apps/plugin-clipboard-manager` to handle screenshots and images when standard web events fail.
- **Markdown Inlining**: Switched image attachments to standard Markdown data URLs for universal model compatibility and persistence.

---

# Tandem v0.1.0 Release Notes

## Highlights

- **Advanced Presentation Engine**: Added high-fidelity PPTX export with theme-aware styling and explicit coordinate mapping.
- **Brand Evolution**: Refreshed the entire application with Rubik 900 typography and a polished, centered boot sequence.
- **AI-Powered Layouts**: Enhanced LLM tool guidance to include spatial positioning for automated slide generation.

## Complete Feature List

### Presentation Engine (Major Update)

- **Explicit Styling**: Exported PPTX files now preserve theme colors (Dark, Corporate, Minimal, Light) and layouts.
- **Positioning Engine**: Implemented EMU (English Metric Unit) mapping to ensure elements are perfectly positioned without overlap.
- **Themed Backgrounds**: Slides now include high-fidelity background shapes matching the selected Tandem theme.
- **Enhanced XML Generation**: Refactored the Rust backend to generate standards-compliant OOXML for maximum compatibility with PowerPoint and Google Slides.

### Branding + UI

- **Rubik 900 Font**: Migrated primary brand elements to Google's Rubik font (Weight 900) for a bolder, more modern look.
- **Centered Splash Screen**: Improved the initialization sequence with precise horizontal centering and smoother transitions.
- **Matrix Loader Refinement**: Synchronized the MatrixRain effect with the new brand typography.

### Developer Experience

- **Tool Guidance v2**: Updated the internal AI instruction set to proactively use spatial coordinates when designing artifacts.
- **Local Build Support**: Added documentation for code signing keys to enable independent local builds.

---

# Tandem v1.0.0 Release Notes

## Highlights

- Local-first AI workspace for chat, planning, and execution across your files
- Rich file viewer with specialized previews and safe write controls
- Permissioned tooling with undo support (including git-backed rollback)
- New presentation workflow with preview and PPTX export

## Complete Feature List

### Core Chat + Planning

- Multi-turn chat with streaming responses
- Plan Mode for reviewable, step-by-step execution
- Task execution controls for plan approval and run
- Session-based chat history
- Agent selection for different workflows

### Workspace + Files

- Work with any directory on disk
- Supports local Google Drive directories
- File browser with search and preview
- Rich viewer for text, code, markdown, images, and PDFs
- Presentation preview for `.tandem.ppt.json` files
- File-to-chat attachments and drag/drop support
- Safe file operations gated by permission prompts

### Safety + Control

- Permission system for read/write/command execution
- Clear confirmation UX before sensitive actions
- Undo support when git is available (rollback of file changes)
- Error reporting surfaced in the UI

### Providers + Models

- Multiple provider support (local and hosted)
- Model selector grouped by provider
- Context length visibility per model

### Updates + Distribution

- Auto-update support via Tauri updater
- Cross-platform desktop app (Windows/macOS/Linux)

### Presentation Workflow

- Two-phase flow: outline planning (reviewable) then JSON execution
- Uses `.tandem.ppt.json` as the source of truth
- Slides JSON schema shared across frontend and backend
- Plan Mode integration for approval before generation
- Immediate Mode support for direct generation when Plan Mode is off

### Presentation Preview

- Dedicated preview experience for `.tandem.ppt.json` files
- Theme support: light, dark, corporate, minimal
- Layouts: title, content, section, blank
- Slide navigation via arrows, buttons, and thumbnail strip
- Keyboard navigation for left/right arrows
- Speaker notes toggle

### Export

- One-click export to `.pptx`
- Tauri command `export_presentation` generates binary PPTX
- Rust backend uses `ppt-rs` for generation

### Chat + Controls

- Context toolbar below the chat input for Agent, Tools, and Model selectors
- Tool category picker with enabled badge count
- Model selector grouped by provider with context length display
- Tool guidance injected per message based on enabled categories

### File Handling

- Automatic detection of `.tandem.ppt.json` files
- Preview routing that prioritizes presentation files before generic preview
- File browser integration to open presentation previews directly

### Developer + System Notes

- New presentation types in TypeScript (`Presentation`, `Slide`, `SlideElement`)
- Tauri API wrapper `getToolGuidance()` for dynamic instructions
- Tauri command registration for presentation features

## Known Limitations

- No image embedding in exported slides yet
- Basic layout options only; advanced positioning is not included

## Next Up

- Image and chart support
- More layout templates and theme customization
- PDF export and batch export
