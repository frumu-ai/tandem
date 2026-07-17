// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn output_target_has_known_file_extension(token: &str) -> bool {
    let extension = Path::new(token)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    matches!(
        extension.as_deref(),
        Some(
            "md" | "markdown"
                | "txt"
                | "json"
                | "jsonl"
                | "yaml"
                | "yml"
                | "csv"
                | "tsv"
                | "toml"
                | "ini"
                | "cfg"
                | "conf"
                | "env"
                | "xml"
                | "html"
                | "htm"
                | "sql"
                | "rs"
                | "ts"
                | "tsx"
                | "js"
                | "jsx"
                | "mjs"
                | "cjs"
                | "py"
                | "go"
                | "java"
                | "kt"
                | "swift"
                | "rb"
                | "php"
                | "c"
                | "h"
                | "cc"
                | "cpp"
                | "hpp"
                | "cs"
                | "sh"
                | "css"
                | "scss"
                | "vue"
                | "svelte"
                | "pdf"
                | "docx"
                | "xlsx"
                | "pptx"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "svg"
                | "webp"
                | "zip"
                | "tar"
                | "gz"
        )
    )
}
