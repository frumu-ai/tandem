use std::fs;
use std::path::{Path, PathBuf};

fn should_skip_dir(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or_default();
    matches!(name, ".git" | "target" | "node_modules" | ".tandem")
}

pub fn search_workspace_files(root: &Path, query: &str, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    let normalized_query = query.trim().to_lowercase();

    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if !should_skip_dir(&path) {
                    stack.push(path);
                }
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if normalized_query.is_empty() || rel_str.to_lowercase().contains(&normalized_query) {
                out.push(rel_str);
                if out.len() >= limit {
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}
