# Feature Request: Agent-Ready Web Markdown Extraction (Local, Rust)

**Owner:** Tandem Engine  
**Status:** Feature request (TODO)  
**Priority:** High (quality + cost + UX win for research/tools)  
**Motivation:** Replace “HTML soup” with clean, structured Markdown for agents, without relying on Cloudflare’s paid Markdown-for-Agents feature.

---

## Problem

When agents fetch web pages today, they often receive:
- heavy HTML (nav, ads, cookie banners, scripts)
- low-signal content that wastes tokens
- inconsistent structure that hurts extraction/summarization

Cloudflare offers “Markdown for Agents” but it’s gated behind Pro/Business plans. We want the same (or better) capability built into **tandem-engine** as a **proprietary/local-first** feature that works on **any site**.

---

## Current Tandem Reality (Web Fetch)

The existing `webfetch` tool in **tandem-tools** is a thin HTTP GET:
- Direct `reqwest::get(url)` with no custom timeouts, no size caps, no SSRF protections, and no redirect policy.
- No content-type detection or HTML cleanup; it returns raw response text.
- Return shape today: `ToolResult { output: <string>, metadata: { truncated: boolean } }` with **20,000 chars max**.
- No Markdown conversion, no main-content extraction, no metadata (title/canonical/links).

Separately, `websearch` uses an MCP endpoint (Exa), but **webfetch is not MCP-backed** yet.

Implication: the new feature must either (a) replace the current `webfetch` implementation, or (b) add a new tool that returns a structured `WebDocument` and update tool selection and permissions accordingly.

---

## Goals

1. Convert arbitrary HTML pages into clean Markdown suitable for LLM consumption.
2. Extract main content (article/docs) and drop boilerplate (nav/footers/sidebars).
3. Return both:
   - `markdown` (structured: headings, lists, code blocks)
   - `text` (plain, compact fallback)
4. Provide stable metadata for citations and follow-up fetches:
   - title
   - canonical URL (if available)
   - outbound links
   - publish date (best-effort)
5. Enforce hard limits (bytes, timeouts, redirects) and safe handling of compressed responses.

---

## Non-Goals (initial)

- Full JS-rendering / headless browser execution (no remote JS)
- Pixel-perfect table reconstruction for every site
- Perfect paywalled/blocked page handling beyond “best-effort”

---

## Proposed API and Tool Contract

### WebDocument shape (suggested)

(Use indented blocks in this doc to avoid triple-backtick fences.)

    {
      "url": "https://example.com/page",
      "final_url": "https://example.com/page?ref=1",
      "title": "Example Title",
      "content_type": "text/html",
      "markdown": "# Example Title\n\n…",
      "text": "Example Title\n\n…",
      "links": [
        {"text": "Some Link", "href": "https://example.com/other"}
      ],
      "meta": {
        "canonical": "https://example.com/page",
        "lang": "en",
        "published_at": "2026-02-14T10:00:00Z"
      },
      "stats": {
        "bytes_in": 123456,
        "bytes_out": 23456,
        "tokens_estimate": 1234,
        "elapsed_ms": 321
      }
    }

### Tool surface

Suggested args:
- `mode: "auto" | "article" | "full"`
  - `auto`: try main-content extraction; fall back to full-page stripped HTML
  - `article`: aggressive readability-style extraction
  - `full`: keep more sections; still remove scripts/styles and normalize
- `return: "markdown" | "text" | "both"` (default: `both`)
- `max_bytes`, `timeout_ms`, `max_redirects` (engine defaults if omitted)

### Backward-compat / replacement path for current `webfetch`

To stay compatible with Tandem’s tool system:
- Option 1 (preferred): keep the tool name `webfetch` but change `output` to be a JSON string matching `WebDocument` and include a compact `text` fallback at the top-level for legacy consumers.
- Option 2: introduce a new tool `webfetch_document` (or `webfetch_markdown`) returning `WebDocument`, and keep `webfetch` returning raw text for legacy behavior.

Document the return shape in tool schema/metadata so the UI and agents can rely on deterministic fields.

---

## Architecture / Pipeline

1. Fetch
   - HTTP GET with timeouts, redirect policy, response size caps, and safe decompression caps.
   - Cache by URL + (ETag/Last-Modified) with short TTL to avoid re-fetch spam.

2. Detect
   - If `Content-Type` is `text/markdown` or `text/plain`: normalize and return.
   - If HTML: proceed.

3. Clean + Extract
   - Remove `script`, `style`, `noscript`, and common boilerplate tags (`nav`, `footer`, `aside`) where safe.
   - Prefer `main` / `article` / best text-density container (heuristic).
   - Phase 2 adds readability-grade extraction.

4. Convert to Markdown
   - Headings → `#`, `##`, `###`
   - Lists → `-` / `1.`
   - Code/pre → fenced code blocks in output (engine output may use triple-backticks; this doc avoids them only for export stability)
   - Links → `[text](url)`
   - Images → optional (default: drop or keep only alt + URL)

5. Post-process
   - Normalize whitespace, collapse repeated blank lines.
   - Ensure stable ordering for extracted links/headings (deterministic output).
   - Extract title/canonical/lang/publish-time best-effort.

---

## Security & Performance Requirements (Best-in-Class)

### Security
- SSRF protection:
  - block private/loopback/link-local/metadata IP ranges
  - validate DNS resolution; re-check after redirects
  - prohibit cross-scheme redirects and disallow redirects to blocked IP space
- Strict scheme allowlist: `http`/`https` only (reject `file`, `ftp`, `data`, `gopher`)
- TLS policy:
  - enforce modern TLS
  - reject invalid certs by default (no silent downgrade)
- Content-type enforcement:
  - treat mismatched types as errors unless explicitly allowed
- Decompression safety:
  - cap compressed + decompressed bytes (zip-bomb defense)
- Parser safety:
  - cap node count, depth, and text length (parse-bomb defense)

### Performance
- Aggressive caps:
  - per-host timeouts
  - max redirects
  - max bytes in/out
  - max DOM nodes
- Connection pooling + concurrency limits:
  - global and per-host backpressure
- Streaming + early-exit:
  - allow extraction without full DOM when possible (future optimization)
- Deterministic output:
  - stable ordering supports caching and golden tests

---

## Implementation Phases

### Phase 1 (MVP)
- Harden fetch (timeouts/caps/redirects/SSRF baseline)
- Simple boilerplate stripping
- “Best container by text density” heuristic
- Convert cleaned DOM to Markdown
- Return `markdown + text + title + links`

Success criteria: most docs/blog posts become readable and 3–10x smaller than raw HTML.

### Phase 2 (Quality upgrade)
- Add readability-style extraction for `mode=article`
  - Option A: native Rust readability implementation
  - Option B: embed a battle-tested Readability algorithm (JS) executed locally via an embedded JS engine (no remote scripts)
- Better metadata extraction (canonical, publish time, author)
- Better table handling (basic Markdown tables for simple cases)

### Phase 3 (Ecosystem + UX polish)
- Chunking helpers (by headings/semantic blocks) to produce LLM-ready chunks
- Token estimation / compression ratio metrics in logs/UI
- Optional modes (include images / code-only / outline-only)

---

## Testing Plan

1. Golden file tests
   - Small HTML fixtures with expected Markdown output
   - Cover headings, lists, code blocks, links, tables, weird nesting

2. Integration tests (network)
   - Deterministic local test server with:
     - redirects
     - gzip/br
     - large content rejected
     - wrong content-type
     - missing content-length

3. Regression set
   - Curate ~20 real-world pages (docs, blogs, news, wiki)
   - Snapshot expected structure (not necessarily exact text)

---

## Observability

Emit structured events (ties into event streams):
- `web.fetch.start` / `web.fetch.end`
- `web.extract.start` / `web.extract.end`
- `web.convert.start` / `web.convert.end`

Include: URL, bytes in/out, elapsed_ms, mode, success/failure, failure reason.

---

## Acceptance Criteria

- Given a typical blog post HTML:
  - output Markdown is readable with correct headings
  - boilerplate reduced significantly
  - code blocks preserved
- Engine never hangs on large/slow pages; caps enforced
- Tool callers can request `mode=article|full` and `return=markdown|both`
- Works regardless of whether a site is behind Cloudflare

---

## Notes / Rationale

Cloudflare’s feature validates the value (token savings + structure), but gating makes it unreliable as a baseline. Tandem should own this capability to keep “research and build with AI” smooth, cheap, and consistent for everyone.
