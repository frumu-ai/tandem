# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.7] - 2026-01-21

### Fixed

- **Slides Workflow Feedback Loop:** Refined the presentation guidance to be more flexible, ensuring the AI acknowledges user feedback/improvements during the planning phase instead of jumping immediately to execution.
- **"Add to Chat" Reliability:** Fixed a state management bug in `ChatInput` that prevented HTML files and other external attachments from being correctly added to the chat context.
- **Blur Obstruction:** Removed the `blur(6px)` transition from the `Message` component and streaming indicator, preventing the chat from becoming unreadable during active AI generation.
- **High-Fidelity PDF Export:** Added `@page { margin: 0; size: landscape; }` and `color-adjust` CSS to the HTML slide template to suppress browser headers/footers and preserve professional aesthetics during PDF export.
- **File Link Detection (Chat UI):** Refined the file path detection regex to only match explicit paths (containing slashes or drive letters), preventing normal text from being incorrectly rendered as "jarbled" clickable links.
- **Dynamic Ollama Discovery:** Implemented automatic model discovery for local Ollama instances. The application now dynamically generates the sidecar configuration based on actually installed local models, ensuring a seamless zero-config experience across all platforms.
- **Cross-Platform Config Reliability:** Updated the sidecar manager to correctly handle OpenCode configuration paths on Linux, macOS, and Windows, and bundled a default template in the installer for improved auto-update reliability.
- **Settings Synchronization:** Fixed a bug where changing the model/provider in settings was not immediately reflected in the Chat interface.
- **Model Selector Refinement:** Cleaned up the model dropdown to prioritize OpenCode Zen/Ollama and hide unconfigured providers, reducing clutter.
- **"Allow All" Logic:** Fixed a critical issue where the "Allow All" toggle was ignored by the event handler, implementing robust auto-approval logic for permissions.
- **Chat History Visibility:** Improved session list filtering to strictly handle project path normalization, ensuring only relevant project chats are shown while preventing history loss.

## [0.1.6] - 2026-01-20

### Added

- **High-Fidelity HTML Slides:** Replaced legacy PPTX generation with an interactive 16:9 HTML slideshow system featuring Chart.js integration, keyboard navigation, optimized PDF export via a dedicated Print button, content overflow protection, and strict density limits (max 6 items per slide).
- **Collapsible Tool Outputs:** Large tool outputs (like `todowrite` or file operations) are now collapsed by default in the chat view, reducing visual noise. Users can expand them to see full details.
- **Chart Generation Capabilities:** Updated internal marketing documentation to highlight the new capability of generating interactive visual dashboards directly from research data.
- HTML Canvas/Report feature: render interactive HTML files in a sandboxed iframe with Tailwind, Chart.js, and Font Awesome support.
- "Research" tool category with dedicated instructions for a robust "Search → Select → Fetch" workflow.
- Visibility of AI reasoning/thinking parts in both live streaming and chat history.
- Automatic persistence of the current active session across reloads/refreshes.
- Default "allow" rules for safe tools (`ls`, `read`, `todowrite`, `websearch`, `webfetch`) to reduce permission prompts.

### Fixed

- **[REDACTED] Filtering:** Removed spurious `[REDACTED]` markers that were leaking from OpenCode's internal reasoning output into the chat UI.
- **File Link Detection (Critical Fix):** Completely rewrote the file path regex to reliably detect Unix absolute paths like `/home/user/file.html` in chat messages, making them clickable.
- **Slide Layout & Scaling:** Fixed vertical stacking of slides in the HTML generator and added auto-scaling to fit the viewer's viewport dimensions.
- **Chat Error Handling:** Implemented deduplication for session error messages to prevent repeated bubbles during stream failures.
- **Linux UI Transparency:** Fixed an issue where the project switcher dropdown was unreadable on Linux due to incorrect glass effect rendering.
- **Session Loading:** Resolved a bug where the application would start with a blank screen instead of loading the previously selected chat session.
- **External Link Handling:** Fixed permission issues preventing "Open in Browser" from working for generated files.
- **HTML Preview:** Links within generated HTML reports now correctly open in the system default browser.
- **Tool Selector Cleanup:** Temporarily disabled the unimplemented "Diagrams" and "Tables" categories from the specialized tools selector to improve UX.
- Robust cancellation: Stop button now reliably terminates backend AI processes using a fallback API mechanism.
- Tool visibility: All tool calls (including technical ones) are now visible throughout the session per user request.
- Fixed chat "freezing" by ensuring intermediate reasoning and tool steps are always streamed to the UI.
- Replaced hardcoded version numbers with dynamic values in `MatrixLoader`, `Settings`, and the initial splash screen.
- Improved error handling in the sidecar manager when primary cancellation endpoints are unavailable.
- Resolved ESLint warnings in `Message.tsx` and `Chat.tsx`.

### Changed

- Updated `create_session` and `rewind_to_message` to include default safe-tool permissions.
- Modified `sidecar.rs` to treat "reasoning" parts as visible content.

## [0.1.5] - 2026-01-20

### Added

- Compact theme selector dropdown with theme details and color swatches.
- Active provider/model badge next to the tool selector in chat.
- Allow-all toggle for tool permissions on new chats.

### Fixed

- Linux `.deb` auto-update now downloads the `.deb` artifact (instead of the AppImage), preventing `update is not a valid deb package`.
- Taskbar no longer overlays the About and Settings screens.
- OpenCode sidecar API compatibility after upstream route changes (provider/model listing and prompt submission).
- Streaming event parsing for newer OpenCode SSE payload shapes.
- Structured provider errors now surface in the chat UI instead of failing silently; improved extraction of specific reasons (e.g., credit limits) from nested responses.
- Permission prompts now render correctly for updated tool event payloads.
- Provider key status refreshes immediately after saving or deleting API keys.
- Technical tool calls (edit, write, ls, etc.) are now handled as transient background tasks and auto-cleanup from chat on success.
- Final AI responses now render reliably at the end of a session, with an automatic backfill mechanism if the stream is interrupted.
- Reduced terminal log spam by downgrading verbose background activity and summarizing large event payloads.
- Fixed a TypeScript error where the `tool` property was missing from the `tool_end` event payload.

### Changed

- Update checking and install progress is now shown at the top of Settings.

## [0.1.4] - 2026-01-20

### Added

- Auto-update functionality with GitHub releases
- Sidecar binary management and updates
- Vault encryption for API keys
- About page with update checker

### Changed

- Improved sidecar process management on Windows
- Enhanced error handling for file operations

### Fixed

- File locking issues during sidecar updates on Windows
- ESLint warnings in React components

## [0.1.0] - 2026-01-18

### Added

- Initial release
- Chat interface with OpenCode AI engine
- Session management and history
- Multi-provider support (Anthropic, OpenAI, OpenRouter)
- Zero-trust security with local encryption
- Project-based organization
- Real-time streaming responses

[Unreleased]: https://github.com/frumu-ai/tandem/compare/v0.1.6...HEAD
[0.1.6]: https://github.com/frumu-ai/tandem/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/frumu-ai/tandem/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/frumu-ai/tandem/compare/v0.1.0...v0.1.4
[0.1.0]: https://github.com/frumu-ai/tandem/releases/tag/v0.1.0
