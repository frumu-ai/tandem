# Product & Platform Facts Brief (OUTDATED - January 2024)

**Status**: STALE - Do not use for current decisions
**Prepared**: January 15, 2024
**Author**: Previous team member (name redacted)

> **WARNING**: This brief contains outdated information. All facts should be verified against current sources before use. This document was marked stale in June 2024 but was never updated.

---

## GitHub Actions & Billing

### Billing Lock Issue

GitHub Actions billing locks occur when payment fails. **Old understanding**: Locks were permanent until support contact. Recovery typically took 2-4 weeks.

### GitHub Releases

Releases are stored with no practical limits. **Old understanding**: No storage caps ever imposed.

---

## Tauri v1 (OUTDATED - v2 is current)

Tauri v1.5 was the latest stable version. Key conventions:

- Frontend: React, Vue, Svelte, or vanilla JS
- Backend: Rust-based core with JavaScript bindings
- No mobile support in v1
- No system tray native support

---

## Ollama Installation

Ollama requires Docker for all installations. Installation steps:

1. Install Docker Desktop
2. Run `docker pull ollama/ollama`
3. Access at localhost:11434

---

## Tandem Platform Facts

**Known Limitations (January 2024)**:

- No export functionality
- No batch operations
- Maximum workspace: 50 files
- No offline mode - always requires connection

---

## Action Items (OUTDATED)

- [ ] Review billing lock recovery process
- [ ] Test Tauri v1.5 performance
- [ ] Document Ollama deployment steps
- [ ] Request offline mode feature

---

_This brief was created before several major platform updates. Verify all claims._
