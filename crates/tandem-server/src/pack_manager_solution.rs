// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Self-contained, pinned solution artifacts through the existing PackManager.
//! These bytes are reusable product content, not customer state or activation.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::ensure;
use tandem_solutions::{
    parse_blueprint, SolutionBlueprint, MAX_ARTIFACT_BYTES, MAX_BLUEPRINT_BYTES,
};

use super::*;

#[derive(Debug)]
pub struct SolutionPackArtifacts {
    pub blueprint: SolutionBlueprint,
    /// Exact verified bytes, keyed by blueprint component ID for ResolutionInput.
    pub artifacts: BTreeMap<String, Vec<u8>>,
}

struct Snapshot {
    files: BTreeMap<String, Vec<u8>>,
    solution: SolutionPackArtifacts,
}

fn portable_path(value: &str) -> anyhow::Result<String> {
    ensure!(
        !value.contains(['\\', ':'])
            && value
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != ".."),
        "solution entry must be a portable relative file path"
    );
    safe_relative_pack_path(value)?;
    Ok(value.into())
}

fn snapshot(root: &Path) -> anyhow::Result<Snapshot> {
    reject_symlink_path(root, "solution pack")?;
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    let mut total = 0usize;
    let mut entries_seen = 0usize;
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            entries_seen += 1;
            ensure!(
                entries_seen <= MAX_FILES * MAX_PATH_DEPTH,
                "solution tree exceeds entry limit"
            );
            ensure!(
                entry.path().strip_prefix(root)?.components().count() <= MAX_PATH_DEPTH,
                "solution path exceeds depth limit"
            );
            let kind = entry.file_type()?;
            ensure!(!kind.is_symlink(), "solution pack contains a symbolic link");
            if kind.is_dir() {
                stack.push(entry.path());
                continue;
            }
            ensure!(kind.is_file(), "solution pack contains a non-regular file");
            ensure!(
                files.len() < MAX_FILES,
                "solution pack exceeds file count limit"
            );
            let path = entry.path();
            let relative = path
                .strip_prefix(root)?
                .to_str()
                .ok_or_else(|| anyhow!("solution pack path must be UTF-8"))?
                .replace('\\', "/");
            portable_path(&relative)?;
            let mut bytes = Vec::new();
            File::open(&path)?
                .take(MAX_FILE_BYTES + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 <= MAX_FILE_BYTES,
                "solution file exceeds size limit"
            );
            total = total
                .checked_add(bytes.len())
                .ok_or_else(|| anyhow!("solution size overflow"))?;
            ensure!(
                total as u64 <= MAX_EXTRACTED_BYTES,
                "solution exceeds extracted size limit"
            );
            ensure!(
                files.insert(relative, bytes).is_none(),
                "duplicate solution file path"
            );
        }
    }
    let marker = files
        .get(MARKER_FILE)
        .ok_or_else(|| anyhow!("solution manifest missing"))?;
    ensure!(
        marker.len() <= MAX_BLUEPRINT_BYTES,
        "solution manifest exceeds size limit"
    );
    let manifest: PackManifest = serde_yaml::from_slice(marker)?;
    ensure!(manifest.pack_type == "solution", "expected solution pack");
    let blueprint_path = portable_path(
        manifest
            .entrypoints
            .get("solution")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("solution pack requires entrypoints.solution"))?,
    )?;
    ensure!(
        blueprint_path != MARKER_FILE && blueprint_path != PACK_SIGNATURE_FILE,
        "solution blueprint must be a separate file"
    );
    let blueprint_bytes = files
        .get(&blueprint_path)
        .ok_or_else(|| anyhow!("solution blueprint missing"))?;
    ensure!(
        blueprint_bytes.len() <= MAX_BLUEPRINT_BYTES,
        "solution blueprint exceeds size limit"
    );
    let blueprint = parse_blueprint(std::str::from_utf8(blueprint_bytes)?)?;
    let pack_id = manifest.pack_id.as_deref().unwrap_or(&manifest.name);
    ensure!(
        blueprint.solution.id == pack_id && blueprint.solution.version == manifest.version,
        "solution identity must match its pack manifest"
    );
    let mut allowed: BTreeSet<String> = [
        MARKER_FILE.into(),
        PACK_SIGNATURE_FILE.into(),
        blueprint_path.clone(),
    ]
    .into();
    let mut artifacts = BTreeMap::new();
    for (id, component) in &blueprint.components {
        ensure!(
            component.artifact.pack_id == pack_id && component.artifact.version == manifest.version,
            "solution v1 requires a self-contained locked artifact closure"
        );
        let path = portable_path(&component.artifact.path)?;
        ensure!(
            path != MARKER_FILE && path != PACK_SIGNATURE_FILE && path != blueprint_path,
            "solution component must reference an artifact file"
        );
        let bytes = files
            .get(&path)
            .ok_or_else(|| anyhow!("solution component {id} artifact missing"))?;
        ensure!(
            bytes.len() <= MAX_ARTIFACT_BYTES,
            "solution artifact exceeds size limit"
        );
        ensure!(
            format!("{:x}", Sha256::digest(bytes)) == component.artifact.sha256,
            "solution component {id} artifact digest mismatch"
        );
        allowed.insert(path);
        artifacts.insert(id.clone(), bytes.clone());
    }
    ensure!(
        files.keys().all(|path| allowed.contains(path)),
        "solution contains undeclared files; keep customer state outside reusable packs"
    );
    // Reuse the pack scanner's patterns, with no size or .example exemptions
    // for the allowlisted solution payload. Do not log matched credential bytes.
    for (path, bytes) in &files {
        if path == PACK_SIGNATURE_FILE {
            continue;
        }
        ensure!(
            !SECRET_SCAN_PATTERNS.iter().any(|needle| bytes
                .windows(needle.len())
                .any(|window| window == needle.as_bytes())),
            "embedded_secret_detected in solution file {path}"
        );
    }
    Ok(Snapshot {
        files,
        solution: SolutionPackArtifacts {
            blueprint,
            artifacts,
        },
    })
}

fn verify_snapshot(snapshot: &Snapshot) -> anyhow::Result<String> {
    let signature = snapshot
        .files
        .get(PACK_SIGNATURE_FILE)
        .ok_or_else(|| anyhow!("solution artifact loading requires a trusted signature"))?;
    // Verify exactly the in-memory bytes returned/exported, not a second tree
    // read which could change between signature verification and artifact use.
    let mut hasher = Sha256::new();
    let mut ordered = snapshot.files.iter().collect::<Vec<_>>();
    // Match pack_content_digest's existing Path ordering exactly. Changing
    // this to flat string ordering would change some nested-file signatures.
    ordered.sort_by(|(left, _), (right, _)| Path::new(left).cmp(Path::new(right)));
    for (path, bytes) in ordered {
        if path == PACK_SIGNATURE_FILE {
            continue;
        }
        hasher.update((path.len() as u64).to_be_bytes());
        hasher.update(path.as_bytes());
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    let digest = hasher.finalize();
    let hexadecimal = format!("{digest:x}");
    verify_pack_signature_bytes(signature, digest.into())?;
    Ok(hexadecimal)
}

pub(super) fn verified_digest(root: &Path, manifest: &PackManifest) -> anyhow::Result<String> {
    let snapshot = snapshot(root)?;
    ensure!(
        snapshot.solution.blueprint.solution.id
            == manifest.pack_id.as_deref().unwrap_or(&manifest.name)
            && snapshot.solution.blueprint.solution.version == manifest.version,
        "solution differs from selected install manifest"
    );
    verify_snapshot(&snapshot)
}

fn verify_installed(snapshot: &Snapshot, record: &PackInstallRecord) -> anyhow::Result<()> {
    let digest = verify_snapshot(snapshot)?;
    ensure!(record.solution_content_sha256.as_deref() == Some(digest.as_str()),
        "solution content differs from install receipt or lacks a validated receipt; reinstall a verified archive");
    ensure!(
        snapshot.solution.blueprint.solution.id == record.pack_id
            && snapshot.solution.blueprint.solution.version == record.version,
        "solution no longer matches its installed identity"
    );
    Ok(())
}

pub(super) fn validate(root: &Path) -> anyhow::Result<()> {
    snapshot(root)?;
    Ok(())
}

pub(super) fn inspection(root: &Path, record: &PackInstallRecord) -> anyhow::Result<Value> {
    let snapshot = snapshot(root)?;
    verify_installed(&snapshot, record)?;
    Ok(serde_json::json!({
        "blueprint": snapshot.solution.blueprint,
        "runtime_materialized": false,
        "activation_required": true,
    }))
}

pub(super) fn export(root: &Path, output: &Path, record: &PackInstallRecord) -> anyhow::Result<()> {
    let snapshot = snapshot(root)?;
    verify_installed(&snapshot, record)?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for (path, bytes) in &snapshot.files {
        writer.start_file(path, options)?;
        std::io::Write::write_all(&mut writer, bytes)?;
    }
    writer.finish()?;
    Ok(())
}

impl PackManager {
    /// Service must separately authorize pack access and installation scope.
    /// Always returns a freshly trusted, self-contained immutable byte snapshot.
    pub async fn solution_artifacts(
        &self,
        selector: &str,
    ) -> anyhow::Result<SolutionPackArtifacts> {
        let index = self.read_index().await?;
        let record =
            select_record(&index, Some(selector), None).ok_or_else(|| anyhow!("pack not found"))?;
        ensure!(record.pack_type == "solution", "pack is not a solution");
        let lock = self.pack_lock(&record.name).await;
        let _guard = lock.lock().await;
        let root = self.validated_record_install_path(&record)?;
        let snapshot = snapshot(&root)?;
        verify_installed(&snapshot, &record)?;
        Ok(snapshot.solution)
    }
}
