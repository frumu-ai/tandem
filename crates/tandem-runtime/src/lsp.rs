use std::path::{Path, PathBuf};
use std::sync::Arc;

use regex::Regex;
use serde::Serialize;

#[derive(Clone)]
pub struct LspManager {
    workspace_root: Arc<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspDiagnostic {
    pub severity: String,
    pub message: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspLocation {
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspSymbol {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line: usize,
}

impl LspManager {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: Arc::new(workspace_root.into()),
        }
    }

    pub fn diagnostics(&self, rel_path: &str) -> Vec<LspDiagnostic> {
        let path = self.absolute_path(rel_path);
        let Ok(content) = std::fs::read_to_string(&path) else {
            return vec![LspDiagnostic {
                severity: "error".to_string(),
                message: "File not found".to_string(),
                path: rel_path.to_string(),
                line: 1,
                column: 1,
            }];
        };

        let mut diagnostics = Vec::new();
        let mut brace_balance = 0i64;
        for (idx, line) in content.lines().enumerate() {
            for ch in line.chars() {
                if ch == '{' {
                    brace_balance += 1;
                } else if ch == '}' {
                    brace_balance -= 1;
                }
            }
            if line.contains("TODO") {
                diagnostics.push(LspDiagnostic {
                    severity: "hint".to_string(),
                    message: "TODO marker".to_string(),
                    path: rel_path.to_string(),
                    line: idx + 1,
                    column: line.find("TODO").unwrap_or(0) + 1,
                });
            }
        }
        if brace_balance != 0 {
            diagnostics.push(LspDiagnostic {
                severity: "warning".to_string(),
                message: "Unbalanced braces detected".to_string(),
                path: rel_path.to_string(),
                line: 1,
                column: 1,
            });
        }
        diagnostics
    }

    pub fn symbols(&self, q: Option<&str>) -> Vec<LspSymbol> {
        let mut out = Vec::new();
        let rust_fn = Regex::new(r"^\s*(pub\s+)?(async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)").ok();
        let rust_struct = Regex::new(r"^\s*(struct|enum|trait)\s+([A-Za-z_][A-Za-z0-9_]*)").ok();
        let ts_fn =
            Regex::new(r"^\s*(export\s+)?(async\s+)?function\s+([A-Za-z_][A-Za-z0-9_]*)").ok();

        for entry in ignore::WalkBuilder::new(self.workspace_root.as_path())
            .build()
            .flatten()
        {
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            let ext = path.extension().and_then(|v| v.to_str()).unwrap_or("");
            if !matches!(ext, "rs" | "ts" | "tsx" | "js" | "jsx" | "py") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(path) else {
                continue;
            };
            for (idx, line) in content.lines().enumerate() {
                if let Some(re) = &rust_fn {
                    if let Some(c) = re.captures(line) {
                        let name = c[3].to_string();
                        if symbol_matches(&name, q) {
                            out.push(LspSymbol {
                                name,
                                kind: "function".to_string(),
                                path: relativize(self.workspace_root.as_path(), path),
                                line: idx + 1,
                            });
                        }
                    }
                }
                if let Some(re) = &rust_struct {
                    if let Some(c) = re.captures(line) {
                        let kind = c[1].to_string();
                        let name = c[2].to_string();
                        if symbol_matches(&name, q) {
                            out.push(LspSymbol {
                                name,
                                kind,
                                path: relativize(self.workspace_root.as_path(), path),
                                line: idx + 1,
                            });
                        }
                    }
                }
                if let Some(re) = &ts_fn {
                    if let Some(c) = re.captures(line) {
                        let name = c[3].to_string();
                        if symbol_matches(&name, q) {
                            out.push(LspSymbol {
                                name,
                                kind: "function".to_string(),
                                path: relativize(self.workspace_root.as_path(), path),
                                line: idx + 1,
                            });
                        }
                    }
                }
            }
            if out.len() >= 500 {
                break;
            }
        }
        out
    }

    pub fn goto_definition(&self, symbol: &str) -> Option<LspLocation> {
        self.symbols(Some(symbol))
            .into_iter()
            .find(|s| s.name == symbol)
            .map(|s| LspLocation {
                path: s.path,
                line: s.line,
                column: 1,
                preview: format!("{} {}", s.kind, s.name),
            })
    }

    pub fn references(&self, symbol: &str) -> Vec<LspLocation> {
        let escaped = regex::escape(symbol);
        let re = Regex::new(&format!(r"\b{}\b", escaped)).ok();
        let Some(re) = re else {
            return Vec::new();
        };
        let mut refs = Vec::new();
        for entry in ignore::WalkBuilder::new(self.workspace_root.as_path())
            .build()
            .flatten()
        {
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            let Ok(content) = std::fs::read_to_string(path) else {
                continue;
            };
            for (idx, line) in content.lines().enumerate() {
                if let Some(m) = re.find(line) {
                    refs.push(LspLocation {
                        path: relativize(self.workspace_root.as_path(), path),
                        line: idx + 1,
                        column: m.start() + 1,
                        preview: line.trim().to_string(),
                    });
                    if refs.len() >= 200 {
                        return refs;
                    }
                }
            }
        }
        refs
    }

    pub fn hover(&self, symbol: &str) -> Option<String> {
        let def = self.goto_definition(symbol)?;
        Some(format!(
            "{}:{}:{} => {}",
            def.path, def.line, def.column, def.preview
        ))
    }

    pub fn call_hierarchy(&self, symbol: &str) -> serde_json::Value {
        let definition = self.goto_definition(symbol);
        let references = self.references(symbol);
        serde_json::json!({
            "symbol": symbol,
            "definition": definition,
            "incomingCalls": references.into_iter().take(50).collect::<Vec<_>>(),
            "outgoingCalls": []
        })
    }

    fn absolute_path(&self, rel_path: &str) -> PathBuf {
        let p = PathBuf::from(rel_path);
        if p.is_absolute() {
            p
        } else {
            self.workspace_root.join(p)
        }
    }
}

fn relativize(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

fn symbol_matches(name: &str, q: Option<&str>) -> bool {
    match q {
        None => true,
        Some(q) => name.to_lowercase().contains(&q.to_lowercase()),
    }
}
