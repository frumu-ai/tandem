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
