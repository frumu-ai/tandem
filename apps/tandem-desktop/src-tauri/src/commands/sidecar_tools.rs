// ============================================================================
// Sidecar Binary Management
// ============================================================================

/// Check the sidecar binary status (installed, version, updates available)
#[tauri::command]
pub async fn check_sidecar_status(app: AppHandle) -> Result<SidecarStatus> {
    sidecar_manager::check_sidecar_status(&app).await
}

/// Fast sidecar status check — local binary only, no network calls.
/// Used on the critical startup path to avoid blocking on GitHub API.
#[tauri::command]
pub async fn check_sidecar_status_fast(app: AppHandle) -> Result<SidecarStatus> {
    sidecar_manager::check_sidecar_status_fast(&app).await
}

/// Download/update the sidecar binary
#[tauri::command]
pub async fn download_sidecar(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    // Stop the sidecar first to release the binary file lock
    tracing::info!("Stopping sidecar before download");
    let _ = state.sidecar.stop().await;

    // Give the process extra time to fully terminate and release file handles
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    sidecar_manager::download_sidecar(app).await
}

// ============================================================================
// Tool Definitions (for conditional tool injection)
// ============================================================================

/// Tool guidance for LLM context injection
/// Instead of custom OpenCode tools, we provide structured instructions
/// for using existing tools (like 'write') to create specialized files
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolGuidance {
    pub category: String,
    pub instructions: String,
    pub json_schema: serde_json::Value,
    pub example: String,
}

/// Get tool guidance based on enabled categories
/// This injects structured instructions for the LLM to follow
#[tauri::command]
pub fn get_tool_guidance(categories: Vec<String>) -> Vec<ToolGuidance> {
    let mut guidance = Vec::new();

    for category in &categories {
        // Fix: borrow categories instead of moving
        match category.as_str() {
            "presentations" => {
                guidance.push(ToolGuidance {
                    category: "presentations".to_string(),
                    instructions: r#"# High-Fidelity 16:9 HTML Slideshows

Use this capability to create premium, interactive presentations that look like modern dashboards.

## TWO-PHASE WORKFLOW:

### PHASE 1: PLANNING
1. **Outline**: Present a structured outline (Title, Theme, Slide-by-slide layout).
2. **Review**: Allow the user to request changes to colors, layout, or content.
3. **Approval**: Once the user approves the outline, proceed to Phase 2.

### PHASE 2: IMPLEMENTATION
1. **Apply Feedback**: Incorporate all requested refinements from the planning phase.
2. **Generate Code**: Use the `write` tool to create the `{filename}.slides.html` file.
3. **Summary**: Briefly confirm that the file has been generated with the requested styles.

## TECHNICAL REQUIREMENTS:

### 1. Slide Stacking (Critical)
- **Absolute Stacking**: All `.slide` elements must be stacked on top of each other.
- **Visibility**: Only the `.active` slide should be visible; all others MUST be `display: none !important`.
- **Content Containment**: Add `overflow: hidden` to `.slide` to prevent content spill.

### 2. Layout & Scaling
- **16:9 aspect ratio** (1920x1080).
- **Safe Margins**: 100px padding for all content.
- **Scale to Fit**: Multi-directional scaling for the entire deck.

### 3. Content Density Limits (STRICT)
- **Max List Items**: 6 per slide.
- **Max Columns**: 2 per slide.
- **Vertical Space**: Leave 200px empty at the bottom.

### 4. High-Fidelity PDF Export
- Add an "Export to PDF" button that triggers `window.print()`.
- **CSS Requirements for Clean PDFs**:
  - `@page { margin: 0; size: landscape; }` (Crucial: Removes headers/footers).
  - `html, body { -webkit-print-color-adjust: exact !important; print-color-adjust: exact !important; }` (Preserves background colors/gradients).
  - Hide all navigation buttons and counters via `.no-print { display: none !important; }`.

## SLIDESHOW HTML TEMPLATE:
```html
<!DOCTYPE html>
<html>
<head>
    <script src="https://cdn.tailwindcss.com"></script>
    <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
    <link href="https://cdnjs.cloudflare.com/ajax/libs/font-awesome/6.0.0/css/all.min.css" rel="stylesheet">
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;600;700&display=swap" rel="stylesheet">
    <style>
        @page { margin: 0; size: landscape; }
        body, html {
            margin: 0; padding: 0; width: 100%; height: 100%; overflow: hidden; background: #020617;
            -webkit-print-color-adjust: exact !important; print-color-adjust: exact !important;
        }
        #viewport { width: 100vw; height: 100vh; display: flex; align-items: center; justify-content: center; }
        #deck {
            width: 1920px; height: 1080px;
            position: relative;
            transform-origin: center;
        }
        .slide {
            position: absolute; inset: 0;
            display: none;
            padding: 100px;
            flex-direction: column;
            overflow: hidden;
        }
        .slide.active { display: flex; }
        @media print {
            body { background: white; overflow: visible; height: auto; }
            #viewport, #deck { width: 100%; height: auto; transform: none !important; display: block; }
            .slide { position: relative; display: block !important; break-after: page; width: 100%; height: auto; aspect-ratio: 16/9; page-break-after: always; overflow: visible; }
            .no-print { display: none !important; }
        }
    </style>
</head>
<body>
    <div id="viewport">
        <div id="deck">
            <!-- SLIDE 1 -->
            <div class="slide active bg-slate-900 text-white">
                <h1 class="text-9xl font-bold italic tracking-tighter">TITLE</h1>
            </div>
            <!-- MORE SLIDES -->
        </div>
    </div>
    <!-- Nav buttons -->
    <div class="no-print fixed bottom-8 right-8 flex gap-4 items-center bg-black/40 backdrop-blur-xl p-4 rounded-2xl border border-white/10">
        <button onclick="window.print()" class="w-12 h-12 flex items-center justify-center rounded-xl bg-emerald-600/20 hover:bg-emerald-600/30 text-emerald-400" title="Export to PDF"><i class="fas fa-file-pdf"></i></button>
        <div class="w-px h-8 bg-white/10"></div>
        <button onclick="prev()" class="w-12 h-12 flex items-center justify-center rounded-xl bg-white/10 hover:bg-white/20"><i class="fas fa-chevron-left"></i></button>
        <span id="counter" class="text-white font-mono min-w-[60px] text-center">1 / X</span>
        <button onclick="next()" class="w-12 h-12 flex items-center justify-center rounded-xl bg-white/10 hover:bg-white/20"><i class="fas fa-chevron-right"></i></button>
    </div>
    <script>
        let current = 0;
        const slides = document.querySelectorAll('.slide');
        function update() {
            slides.forEach((s, i) => s.classList.toggle('active', i === current));
            document.getElementById('counter').innerText = `${current + 1} / ${slides.length}`;
        }
        function next() { current = (current + 1) % slides.length; update(); }
        function prev() { current = (current - 1 + slides.length) % slides.length; update(); }
        window.onkeydown = (e) => {
            if (['ArrowRight', 'Space', 'ArrowDown'].includes(e.code)) next();
            if (['ArrowLeft', 'ArrowUp'].includes(e.code)) prev();
        };
        function fit() {
            const scale = Math.min(window.innerWidth / 1920, window.innerHeight / 1080);
            document.getElementById('deck').style.transform = `scale(${scale})`;
        }
        window.onresize = fit; fit();
    </script>
</body>
</html>
```
"#.to_string(),
                    json_schema: serde_json::json!({
                        "file_type": "HTML Slideshow",
                        "scaling": "Auto-fit viewport",
                        "navigation": "Arrows, Space, Click",
                        "pdf_export": "Print button with optimized layout"
                    }),
                    example: "Generate 'strategic_path.slides.html' with 6 absolutely stacked slides, overflow:hidden, and a Print to PDF button.".to_string(),
                });
            }
            "canvas" => {
                guidance.push(ToolGuidance {
                    category: "canvas".to_string(),
                    instructions: r#"# HTML Canvas / Report Creation

Use this capability when the user asks for "reports", "visualizations", "dashboards", or "canvases".

You can create rich, interactive HTML files that render directly in Tandem's preview.

## Requirements:
1. Create a SINGLE standalone HTML file (e.g., `report.html`, `dashboard.html`).
2. Use **Tailwind CSS** via CDN for styling.
3. Use **Chart.js** via CDN for charts.
4. Use **Font Awesome** via CDN for icons.
5. Use **Google Fonts** (Inter) for typography.
6. The HTML must be self-contained (CSS/JS inside `<style>` and `<script>` tags).

## Template:

```html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Report Title</title>
    <script src="https://cdn.tailwindcss.com"></script>
    <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
    <link href="https://cdnjs.cloudflare.com/ajax/libs/font-awesome/6.0.0/css/all.min.css" rel="stylesheet">
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;600;700&display=swap" rel="stylesheet">
    <style>
        body { font-family: 'Inter', sans-serif; }
    </style>
</head>
<body class="bg-slate-50 text-slate-900">
    <div class="max-w-7xl mx-auto p-8">
        <!-- Content Here -->
        <canvas id="myChart"></canvas>
    </div>
    <script>
        // Chart.js logic here
    </script>
</body>
</html>
```

## Workflow:
1. **Plan:** Propose the structure/content of the report (Plan Mode).
2. **Execute:** Use the `write` tool to create the HTML file.
"#.to_string(),
                    json_schema: serde_json::json!({
                        "file_type": "HTML",
                        "libraries": ["Tailwind CSS", "Chart.js", "Font Awesome"],
                        "structure": "Single file, self-contained"
                    }),
                    example: "Use the `write` tool to create `quarterly_report.html` with Tailwind and Chart.js code.".to_string(),
                });
            }
            "python" => {
                guidance.push(ToolGuidance {
                    category: "python".to_string(),
                    instructions: r#"# Workspace Python (Venv-Only)

Tandem enforces a workspace-scoped Python virtual environment at `.tandem/.venv`.

## Rules (STRICT)

1. Do NOT run `python`, `python3`, `py`, or `pip install` directly.
2. If you need Python packages, instruct the user to open **Python Setup (Workspace Venv)** and click **Create venv in workspace**.
3. Only run Python via the workspace venv interpreter, e.g.:
   - Windows: `.tandem\.venv\Scripts\python.exe ...`
   - macOS/Linux: `.tandem/.venv/bin/python3 ...`
4. Install dependencies using:
   - `-m pip install -r requirements.txt` (preferred) or `-m pip install <pkgs>`

## If the venv is missing

Explain that Tandem will block Python until the venv exists, and the wizard may open automatically when a blocked command is attempted.
"#.to_string(),
                    json_schema: serde_json::json!({
                        "venv_root": ".tandem/.venv",
                        "allowed_python": "workspace venv interpreter only",
                        "install": "venv python -m pip install -r requirements.txt"
                    }),
                    example: "Use `.tandem/.venv/bin/python3 -m pip install -r requirements.txt` then run `.tandem/.venv/bin/python3 script.py`.".to_string(),
                });
            }
            "research" => {
                guidance.push(ToolGuidance {
                    category: "research".to_string(),
                    instructions: r#"# Web Research & Browsing

Use this capability for finding information, verifying facts, or gathering data from the web.

## Best Practices:
1. **Search First:** Always start with `websearch` to find valid, up-to-date URLs.
2. **Avoid Dead Links:** Do not `webfetch` URLs that likely don't exist or are deep links without verifying them first.
3. **Handle Blocking:** Many sites (e.g., Statista, Airbnb, LinkedIn) block bots.
   - If `webfetch` returns 403/404/Timeout:
     - Do NOT retry the exact same URL immediately.
     - Try searching for the specific information on a different site.
     - Try fetching the root domain or a generic page if appropriate.
4. **Prefer Text:** `webfetch` works best on content-heavy pages (docs, blogs, articles). It may fail on heavy SPAs.
5. **Use HTML Only When Needed:** Use `webfetch_html` only when raw DOM/HTML is explicitly required.

## Workflow:
1. **Search:** `websearch` query: "latest real estate trends asia 2025"
2. **Select:** Pick 1-2 promising URLs from the search results.
3. **Fetch:** `webfetch` url: "..."
4. **Fallback:** If fetch fails, go back to step 1 with a refined query or try the next URL.
5. **Raw HTML (optional):** `webfetch_html` url: "..."
"#.to_string(),
                    json_schema: serde_json::json!({
                        "strategy": "Search -> Select -> Fetch -> Fallback",
                        "error_handling": "Stop retrying failing URLs; use alternatives",
                        "limitations": "Some sites block automated access"
                    }),
                    example: "Search for 'rust tauri docs', then fetch the official documentation page.".to_string(),
                });
            }
            "diagrams" => {
                // Future: Mermaid diagram guidance
                tracing::debug!("Diagrams tool category not yet implemented");
            }
            "spreadsheets" => {
                // Future: Table/CSV guidance
                tracing::debug!("Spreadsheets tool category not yet implemented");
            }
            _ => {
                tracing::debug!("Unknown tool category: {}", category);
            }
        }
    }

    tracing::debug!(
        "Returning {} tool guidance items for categories: {:?}",
        guidance.len(),
        categories
    );
    guidance
}

// ============================================================================
