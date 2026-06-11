mod config_docs;
mod source;

use crate::error::{RepoIntelligenceError, Result};
use crate::model::{ExtractedFacts, FileManifestEntry};
use std::path::{Path, PathBuf};

pub fn extract_repo_facts(
    root: impl AsRef<Path>,
    files: &[FileManifestEntry],
) -> Result<ExtractedFacts> {
    let root = root.as_ref();
    let mut facts = ExtractedFacts::default();
    for file in files {
        facts.extend(extract_file_from_root(root, file)?);
    }
    Ok(facts)
}

pub fn extract_file_facts(path: &str, body: &str) -> ExtractedFacts {
    let mut facts = source::extract_source_facts(path, body);
    facts.extend(config_docs::extract_config_doc_facts(path, body));
    facts
}

fn extract_file_from_root(root: &Path, file: &FileManifestEntry) -> Result<ExtractedFacts> {
    let path = root.join(&file.path);
    let bytes = std::fs::read(&path).map_err(|source| RepoIntelligenceError::ReadFile {
        path: PathBuf::from(path),
        source,
    })?;
    let Ok(body) = String::from_utf8(bytes) else {
        return Ok(ExtractedFacts::default());
    };
    Ok(extract_file_facts(&file.path, &body))
}
