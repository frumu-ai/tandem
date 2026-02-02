# Tandem v0.1.14 Release Notes

## Highlights

- **Task Completion Relaibility**: We've tightened the feedback loop between the AI's work and the UI. Now, when Ralph Loop or Plan Mode executes a task, it is explicitly instructed to mark that task as "completed" in your list using the `todowrite` tool. This fixes the annoying desync where the AI would finish the work but leave the checkbox empty.
- **Smarter Execution Prompts**: The automated prompts used during plan execution have been refined to ensure the AI understands exactly how to report its progress back to the interface.

## Complete Change List

### Core Intelligence

- **Prompt Engineering**: Updated `ralph/service.rs` and `Chat.tsx` to include strict directives for task status updates. The AI is now mandated to call `todowrite` with `status="completed"` immediately after finishing a task item.

- **Ralph Loop (Iterative Task Agent)**: Meet Ralph—a new mode that puts the AI in a robust "do-loop." Give it a complex task, and it will iterate, verify, and refine its work until it meets a strict completion promise. It's like having a tireless junior developer who checks their own work.
- **Long-Term Memory**: Tandem now remembers! We've integrated a semantic memory system using `sqlite-vec` that allows the AI to recall context from previous sessions and project documents. This means smarter, more context-aware assistance that grows with your project.
- **Semantic Context Retrieval**: Questions about your project now tap into a vector database of your codebase, providing accurate, relevant context even for large repositories that don't fit in a standard prompt.

## Complete Feature List

### Core Intelligence

- **Vector Memory Store**: Implemented a local, zero-trust vector database (`sqlite-vec`) to store and retrieve semantic embeddings of your codebase and conversation history.
- **Memory Context Injection**: The AI now automatically receives relevant context snippets based on your current query, reducing hallucinations and "I don't know" responses about your own code.

### Workflow

- **Ralph Loop Mode**: New "Loop" toggle in the chat control bar enables iterative task execution. The AI loops until `<promise>COMPLETE</promise>` is detected, with configurable min/max iterations and struggle detection.
- **Iterative Execution Loop**: Automates the [Plan -> Execute -> Verify] cycle. It loops until the task is verifiably complete, with built-in pause/resume controls for human oversight.
- **Plan Mode Integration**: Ralph works hand-in-hand with Plan Mode, respecting your review process while automating the repetitive parts of implementation.
- **Context Injection**: Add mid-loop context via the Ralph panel to guide the AI when it gets stuck or needs clarification.
- **Struggle Detection**: Automatically detects when the AI is stuck (no file changes for 3+ iterations or repeated errors) and injects helpful hints.

### Ralph Loop Technical Details

- **Workspace-local Storage**: Loop state, history, and context stored in `.opencode/tandem/ralph/` directory
- **Git Integration**: Tracks file changes via `git status` and `git diff` between iterations
- **Completion Detection**: Uses regex pattern matching for `<promise>COMPLETE</promise>` token
- **Safety Features**: Max iteration limit (50), graceful cancellation, error handling with state persistence
- **Tauri Commands**: Full API with `ralph_start`, `ralph_pause`, `ralph_resume`, `ralph_cancel`, `ralph_add_context`, `ralph_status`, `ralph_history`

### Technical improvements

- **SQLite Extension Fix**: Resolved a critical build issue with `sqlite-vec` by properly registering the C-extension initialization, ensuring stable memory database connections.
- **Documentation**: Added comprehensive documentation for the Ralph Loop architecture and implementation details in `docs/ralph-loop/`.

---

# Tandem v0.1.13 Release Notes

## Highlights

- **Native Planning Mode**: A powerful new agent mode for drafting architecture and implementation plans. No more jumping into code blindly—the AI now proposes a markdown-based plan for you to review and approve first.
- **Real-time Plan Synchronization**: Your UI is now a live reflection of your plan files. Changes made by the AI (or you) to plan files are immediately detected and reflected in the chat, thanks to a new backend watcher.
- **Clarification Buttons**: Interactive follow-up questions from the AI now appear with clickable suggestion buttons, making it easier to provide quick feedback on technical decisions.
- **Backend Stability**: Fixed the `get_workspace_path` compilation error that plagued the recent development cycle.

## Complete Feature List

### Planning

- **Plan Agent Skill**: Optimized prompt engineering to force structured, question-free plan generation.
- **Plan Viewer & Selector**: New UI components for navigating and reviewing plans created during the session.
- **Synchronized Backend Watcher**: Tauri-backed filesystem notifications for the plans directory.
- **Interactive Questions**: Support for `ask_followup_question` with structured options in the chat interface.

### Technical improvements

- **Whitespace Enforcement**: Tightened tool call parsing to eliminate errors caused by leading/trailing spaces in tool names.
- **Strict Context Injection**: Dynamic system directives that prime the AI with the correct plan file path and immediate goals.
- **Code Health**: Cleaned up various linting and formatting warnings across the Rust and TypeScript codebases.

---

# Tandem v0.1.12 Release Notes

## Highlights

- **File Viewer Fixed**: Resolved "Failed to load directory" error that broke the Files tab. The overly restrictive path security checks from v0.1.11 were causing Windows path normalization issues and have been removed.
- **Permission Spam Fix**: Stopped repeated approval prompts for the same tool request, even when "Allow all" is enabled.
- **Reliable Auto-Approval**: Auto-approve now responds to the correct permission request ID and no longer creates duplicate prompts.
- **Cleaner Session Switching**: Pending approvals reset cleanly when switching sessions to prevent stale prompts.

## Complete Fix List

### File Access

- **Directory Loading**: Removed path allowlist checks from `read_directory`, `read_file_content`, and `read_binary_file` that were causing the file viewer to fail. The canonical path normalization on Windows (e.g., `\\?\C:\...`) wasn't matching the stored allowed paths, resulting in "Failed to load directory" errors.

### Permissions

- **Deduplicated Requests**: Permission prompts are now keyed to a single request ID to prevent duplicates.
- **Allow All Alignment**: Auto-approval now uses the permission request ID instead of tool start IDs.
- **Session Reset**: Pending permissions are cleared when the active session changes.

---

# Tandem v0.1.11 Release Notes

## Highlights

- **Version Metadata Repair**: Corrected mismatched version numbers across files (some 0.1.8/0.1.9 while the built version was 0.1.10) to restore reliable auto-update detection. If you're on v0.1.9, you should now be able to update to v0.1.11 and receive all the Skills Management features from v0.1.10.
- **Safer File Access**: Tightened file browser access to the active workspace with stronger denylist enforcement on Windows.
- **Quieter Streaming**: Removed verbose debug logs to keep the UI responsive during long streaming sessions.

## Complete Fix List

### Security & Stability

- **Workspace Allowlist Enforcement**: File browsing, text reads, and binary reads now require the path to be inside the active workspace.
- **Denylist Normalization**: Windows path separators are normalized to ensure patterns like `.env`, `.key`, and `.pem` are consistently blocked.
- **Binary Read Guardrails**: Large binary files are now blocked by default with a safe size limit.

### Performance

- **Streaming Log Reduction**: Removed high-volume stream and provider logs that could slow down the UI during generation.

## Notes

- This is a patch release to fix the v0.1.10 update detection issue. All features from v0.1.10 (Skills Management, automatic sidecar restart, etc.) are included.
- If you're manually installing, you can skip directly to v0.1.11.

---

# Tandem v0.1.10 Release Notes

## Highlights

- **Skills Management System**: Introduced a complete skills management interface allowing you to import, discover, and organize OpenCode-compatible skills. Skills extend the AI's capabilities with specialized instructions for specific workflows like code review, documentation, and more.
- **Robust YAML Parsing**: Fixed critical compatibility issues with SKILL.md files containing special characters (colons, quotes) in descriptions. The parser now automatically handles edge cases for seamless skill imports.
- **Smart Project Context**: The Skills panel intelligently displays your active project name and automatically adjusts UI based on workspace state, preventing errors when no project is selected.
- **Resource Discovery**: Added direct links to popular skill repositories (Awesome Claude Skills with 100+ curated skills, SkillHub with 7,000+ community contributions, and official documentation) to help you discover and import useful skills quickly.

## Complete Feature List

### Skills Management

- **Visual Skills Panel**: New dedicated section in Settings for managing skills with clear separation between Project and Global skills
- **Import Workflow**: Paste SKILL.md content directly into the interface, choose installation location (project-specific or global), and save with one click
- **Automatic Discovery**: Skills are automatically detected from both `.opencode/skill/` (project) and `~/.config/opencode/skills/` (global) directories
- **Blank Skill Template**: Quick-start option to create a new skill from scratch with proper YAML frontmatter structure
- **Delete Support**: Remove unwanted skills directly from the UI
- **Seamless Restart**: After importing a skill, the AI engine automatically restarts with a polished full-screen overlay featuring animated icons and progress indicators - no manual intervention required

### UX Improvements

- **Active Project Indicator**: Bold, color-highlighted display of the current project name with human-readable folder name
- **Context-Aware UI**: Project option automatically disabled when no workspace is selected, with clear "(no project selected)" messaging
- **Auto-Refresh**: Skills list updates immediately after importing without requiring manual refresh
- **Professional Styling**: Cleaned up button text and visual hierarchy for a more polished appearance

### Technical Fixes

- **YAML Frontmatter Parsing**: Automatically quotes description values containing colons, preventing parse failures with complex skill descriptions
- **Content Preservation**: Skills are now saved with their original formatting intact instead of being reconstructed, eliminating corruption issues
- **External Link Support**: All skill resource links use Tauri's native `openUrl()` for proper desktop app integration
- **Type Safety**: Resolved TypeScript prop type errors in SkillsPanel component
- **Enhanced Logging**: Added detailed debug output for skill discovery and parsing to aid troubleshooting

## Notes

- **Skill Location**: Project skills (`.opencode/skill/`) are workspace-specific and can be version-controlled. Global skills (`~/.config/opencode/skills/`) are available across all projects.
- **YAML Format**: While SKILL.md uses YAML frontmatter (industry standard), you no longer need to worry about quoting special characters - the parser handles this automatically.
- **Skill Discovery**: The AI automatically uses installed skills when relevant to the conversation - no manual selection needed.

---

# Tandem v0.1.9 Release Notes

## Highlights

- **macOS Styling Polish**: Refined the global glass effect and UI variables for a more sophisticated, premium look and feel on macOS.
- **Improved HTML Previews**: Added `baseHref` support to the sandboxed HTML preview component. Relative paths for images, scripts, and stylesheets now resolve correctly, ensuring generated reports look exactly as intended.

---

# Tandem v0.1.7 Release Notes

## Highlights

- **Dynamic Ollama Support**: Implemented zero-config model discovery for Ollama. Tandem now automatically detects your local models and configures the sidecar on-the-fly, so "Ollama (Local)" works instantly for all users.
- **Improved Chat Rendering**: Fixed a bug where normal text was being incorrectly identified as file paths. The chat UI is now significantly more robust and no longer produces "jarbled" clickable words.
- **Model Selection UX**: The provider/model selector has been streamlined to prioritize OpenCode Zen and Ollama, while automatically hiding unconfigured providers to reduce list noise.
- **Instant Settings Sync**: Changing your preferred model or provider in Settings now updates the Chat interface immediately without requiring a refresh.
- **Mid-Stream Permissions**: The "Allow All" button is now unblocked during generation, AND the auto-approval logic has been fixed to reliably intercept and approve tool requests without prompting.
- **Robust Session Visibility**: Session filtering is now smarter about path normalization, ensuring your chat history is complete for the active project while correctly hiding unrelated sessions.
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

# Tandem v0.1.4 Release Notes

## Highlights

- **Auto-Update**: Built-in updater for seamless upgrades via GitHub Releases.
- **About Page**: New dedicated view for version info and update management.
- **Key Security**: API keys are now encrypted at rest using system-native credential stores (Keytar).
- **Sidecar Management**: Improved process lifecycle handling and binary updates.

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
