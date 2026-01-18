# Specialists & Non-Technical User Features

> Future feature ideas for expanding Tandem beyond developer-focused use cases.

## Overview

Tandem's local-first architecture provides unique advantages for non-technical users:

- **Privacy**: Documents never leave the user's computer
- **No upload limits**: Works with large files/folders that cloud AI tools reject
- **Offline capable**: Can analyze documents without internet (with local models like Ollama)

This document captures ideas for making Tandem accessible and valuable to average users who want AI assistance with documents, analysis, and business tasks.

---

## Specialist System

### Concept

Instead of one generic AI chat, users choose from purpose-built "specialists" - AI personas configured for specific tasks with appropriate tools, prompts, and UI.

### Proposed Specialists

| Specialist            | Purpose                                      | Key Tools                   |
| --------------------- | -------------------------------------------- | --------------------------- |
| **Document Analyst**  | Summarize documents, extract key points, Q&A | Read files, search          |
| **Data Extractor**    | Structured output, tables, CSV generation    | Read, write CSV             |
| **Writing Assistant** | Style matching, editing, drafts              | Read examples, write docs   |
| **File Organizer**    | Safe file ops, naming conventions            | List, rename, move (staged) |
| **Research Helper**   | Citations, cross-referencing                 | Read, web search            |

### Configuration Structure

Each specialist defined as YAML/JSON config:

```yaml
# Example: specialists/document-analyst.yaml
name: "Document Analyst"
icon: "file-text"
description: "Summarize documents, extract key points, answer questions"

system_prompt: |
  You are a document analysis specialist. Your role is to help users 
  understand their documents by:
  - Summarizing key points clearly
  - Answering questions about content
  - Comparing multiple documents
  - Extracting specific information

  Always cite which document/page your information comes from.
  Use simple, non-technical language.

tools:
  enabled: [read_file, list_directory, search]
  disabled: [write_file, execute_command]

ui:
  show_terminal: false
  show_code_highlighting: false
  welcome_message: "Drop your documents here or select a folder to analyze"
  suggested_prompts:
    - "Summarize the main points"
    - "What are the key dates mentioned?"
    - "Compare these two documents"
```

---

## UX Improvements for Non-Technical Users

### Onboarding Wizard

- "What do you want to do today?" instead of empty chat
- Guide users to select appropriate specialist
- Help them connect their first folder

### Staged File Operations

- Preview all changes before applying
- Clear diff view showing what will change
- One-click undo for entire operations
- (Infrastructure already exists via StagingStore)

### Progress Indicators

- "Reading 5 files..."
- "Analyzing content..."
- "Generating summary..."
- Show which files are being processed

### Output Formatting

- Render tables nicely in the UI
- "Copy as Excel" / "Copy as CSV" buttons
- Export to formatted documents
- Charts and visualizations for data

### Strong Undo Support

- Undo any file operation
- Session-level undo history
- "Undo everything from last 5 minutes"

### Suggested Follow-ups

- After each response, show contextual action buttons
- "Would you like me to..." suggestions
- Quick refinement options

---

## Cloud Storage Integration (Future)

### Option A: Native Google Drive Integration

**Pros:**

- Seamless UX
- Direct file access

**Cons:**

- OAuth2 complexity
- API rate limits
- Significant development effort (~2-3 weeks)

**Technical requirements:**

- OAuth2 flow in Tauri (browser redirect handling)
- Google Drive API client (Rust crates available)
- File caching/sync strategy
- Permission management UI

### Option B: Recommend Google Drive Desktop (Recommended for v1)

Users install Google Drive for Desktop, which syncs to a local folder. Tandem accesses that local folder like any other directory.

**Pros:**

- Zero integration work
- Works immediately
- User controls sync settings

**Cons:**

- Requires separate app installation
- Not as seamless

---

## Open Questions

- [ ] Should specialists be built-in only, or can users create/share their own?
- [ ] Monetization angle: premium specialists?
- [ ] Different model defaults per specialist (cheaper models for simple tasks)?
- [ ] How to handle specialist-specific conversation history?
- [ ] Should specialists have memory/learning across sessions?

---

## Implementation Priority

1. **Phase 1**: Simplified UI mode (hide dev features)
2. **Phase 2**: Specialist picker + 2-3 built-in specialists
3. **Phase 3**: Custom specialist creation
4. **Phase 4**: Cloud storage integrations (if user demand)

---

_Last updated: January 2026_
