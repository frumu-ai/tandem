// Runtime engine release artifacts are authenticated before any archive
// content is extracted, made executable, or activated.

use super::{GitHubAsset, GitHubRelease, ENGINE_REPO};
use crate::error::{Result, TandemError};
use flate2::read::GzDecoder;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tandem_enterprise_contract::{
    current_artifact_architecture, current_artifact_platform, verify_artifact_manifest,
    ArtifactDigestVerifier, ArtifactKind, ArtifactManifestEntry, ArtifactManifestExpectation,
    ARTIFACT_MANIFEST_FILENAME, ARTIFACT_MANIFEST_SIGNATURE_FILENAME, MAX_ARTIFACT_MANIFEST_BYTES,
    MAX_ARTIFACT_SIGNATURE_BYTES,
};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

const ENGINE_RELEASE_WORKFLOWS: &[&str] = &[
    ".github/workflows/release.yml",
    ".github/workflows/engine-release.yml",
];
const MAX_ENGINE_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ENGINE_BINARY_BYTES: u64 = 1024 * 1024 * 1024;
const USER_AGENT: &str = "tandem-desktop-engine-installer";

#[derive(Debug)]
struct PendingArtifactPath {
    path: PathBuf,
    armed: bool,
}

impl PendingArtifactPath {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PendingArtifactPath {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[derive(Debug)]
pub(super) struct PreparedEngineInstall {
    staged: PendingArtifactPath,
    downloaded_bytes: u64,
    expected_version: String,
}

impl PreparedEngineInstall {
    pub(super) fn downloaded_bytes(&self) -> u64 {
        self.downloaded_bytes
    }
}

pub(super) fn release_client() -> Result<reqwest::Client> {
    let redirect = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 4 {
            return attempt.stop();
        }
        if trusted_release_redirect(attempt.url()) {
            attempt.follow()
        } else {
            attempt.stop()
        }
    });
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(5 * 60))
        .redirect(redirect)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|_| sidecar_error("artifact_http_client_create_failed"))
}

fn trusted_release_redirect(url: &reqwest::Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    url.host_str().is_some_and(|host| {
        host == "github.com" || host == "api.github.com" || host.ends_with(".githubusercontent.com")
    })
}

pub(super) async fn prepare_verified_engine<F>(
    client: &reqwest::Client,
    release: &GitHubRelease,
    asset_name: &str,
    binaries_dir: &Path,
    progress: F,
) -> Result<PreparedEngineInstall>
where
    F: Fn(u64, u64),
{
    let expected_release = release.tag_name.as_str();
    let expected_version = super::normalize_version_label(expected_release);
    let manifest_asset = exact_release_asset(release, ARTIFACT_MANIFEST_FILENAME)?;
    let signature_asset = exact_release_asset(release, ARTIFACT_MANIFEST_SIGNATURE_FILENAME)?;
    let manifest_bytes =
        download_small_release_asset(client, manifest_asset, MAX_ARTIFACT_MANIFEST_BYTES).await?;
    let signature_bytes =
        download_small_release_asset(client, signature_asset, MAX_ARTIFACT_SIGNATURE_BYTES).await?;
    let verified = verify_artifact_manifest(
        &manifest_bytes,
        &signature_bytes,
        ArtifactManifestExpectation {
            source_repository: ENGINE_REPO,
            release: expected_release,
            version: expected_version,
            allowed_workflows: ENGINE_RELEASE_WORKFLOWS,
        },
    )
    .map_err(|error| sidecar_error(error.to_string()))?;

    let platform = current_artifact_platform().map_err(|error| sidecar_error(error.to_string()))?;
    let architecture =
        current_artifact_architecture().map_err(|error| sidecar_error(error.to_string()))?;
    let manifest_entry = verified
        .artifact(ArtifactKind::Engine, platform, architecture, asset_name)
        .map_err(|error| sidecar_error(error.to_string()))?
        .clone();
    let asset = exact_release_asset(release, asset_name)?;
    if asset.size != manifest_entry.length {
        return Err(sidecar_error("artifact_metadata_length_mismatch"));
    }

    std::fs::create_dir_all(binaries_dir)
        .map_err(|_| sidecar_error("artifact_install_directory_create_failed"))?;
    let archive =
        download_verified_archive(client, asset, &manifest_entry, binaries_dir, progress).await?;
    let archive_path = archive.path().to_path_buf();
    let parent = binaries_dir.to_path_buf();
    let asset_name = asset_name.to_string();
    let staged = tokio::task::spawn_blocking(move || {
        extract_verified_archive(&archive_path, &parent, &asset_name)
    })
    .await
    .map_err(|_| sidecar_error("artifact_extract_task_failed"))??;
    drop(archive);

    Ok(PreparedEngineInstall {
        staged,
        downloaded_bytes: manifest_entry.length,
        expected_version: expected_version.to_string(),
    })
}

fn exact_release_asset<'a>(release: &'a GitHubRelease, name: &str) -> Result<&'a GitHubAsset> {
    let mut matches = release.assets.iter().filter(|asset| asset.name == name);
    let asset = matches
        .next()
        .ok_or_else(|| sidecar_error("release_required_asset_missing"))?;
    if matches.next().is_some() {
        return Err(sidecar_error("release_duplicate_asset"));
    }
    let trusted_url = reqwest::Url::parse(&asset.browser_download_url)
        .ok()
        .is_some_and(|url| trusted_release_redirect(&url));
    if !trusted_url {
        return Err(sidecar_error("release_asset_url_untrusted"));
    }
    Ok(asset)
}

async fn download_small_release_asset(
    client: &reqwest::Client,
    asset: &GitHubAsset,
    limit: usize,
) -> Result<Vec<u8>> {
    if asset.size == 0 || asset.size > limit as u64 {
        return Err(sidecar_error("release_metadata_asset_size_invalid"));
    }
    let response = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(|_| sidecar_error("release_metadata_asset_request_failed"))?;
    if !response.status().is_success() {
        return Err(sidecar_error("release_metadata_asset_download_failed"));
    }
    bounded_response_bytes(response, limit, asset.size).await
}

async fn bounded_response_bytes(
    mut response: reqwest::Response,
    limit: usize,
    expected_length: u64,
) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64 || length != expected_length)
    {
        return Err(sidecar_error("release_response_length_invalid"));
    }
    let mut body = Vec::with_capacity(expected_length.min(limit as u64) as usize);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| sidecar_error("release_response_read_failed"))?
    {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(sidecar_error("release_response_too_large"));
        }
        body.extend_from_slice(&chunk);
    }
    if body.len() as u64 != expected_length {
        return Err(sidecar_error("release_response_length_invalid"));
    }
    Ok(body)
}

async fn download_verified_archive<F>(
    client: &reqwest::Client,
    asset: &GitHubAsset,
    manifest_entry: &ArtifactManifestEntry,
    parent: &Path,
    progress: F,
) -> Result<PendingArtifactPath>
where
    F: Fn(u64, u64),
{
    let mut verifier = ArtifactDigestVerifier::new(manifest_entry, MAX_ENGINE_ARCHIVE_BYTES)
        .map_err(|error| sidecar_error(error.to_string()))?;
    let mut response = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(|_| sidecar_error("artifact_download_request_failed"))?;
    if !response.status().is_success() {
        return Err(sidecar_error("artifact_download_failed"));
    }
    if response
        .content_length()
        .is_some_and(|length| length != verifier.expected_length())
    {
        return Err(sidecar_error("artifact_length_mismatch"));
    }

    let (pending, mut file) = create_pending_file(parent, "archive").await?;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| sidecar_error("artifact_download_read_failed"))?
    {
        verifier
            .update(&chunk)
            .map_err(|error| sidecar_error(error.to_string()))?;
        file.write_all(&chunk)
            .await
            .map_err(|_| sidecar_error("artifact_write_failed"))?;
        progress(verifier.observed_length(), verifier.expected_length());
    }
    file.flush()
        .await
        .map_err(|_| sidecar_error("artifact_flush_failed"))?;
    file.sync_all()
        .await
        .map_err(|_| sidecar_error("artifact_sync_failed"))?;
    drop(file);
    verifier
        .finalize()
        .map_err(|error| sidecar_error(error.to_string()))?;
    Ok(pending)
}

async fn create_pending_file(
    parent: &Path,
    label: &str,
) -> Result<(PendingArtifactPath, tokio::fs::File)> {
    for _ in 0..8 {
        let path = parent.join(format!(".tandem-engine-{}-{label}", Uuid::new_v4()));
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
        {
            Ok(file) => return Ok((PendingArtifactPath::new(path), file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(sidecar_error("artifact_pending_file_create_failed")),
        }
    }
    Err(sidecar_error("artifact_pending_file_collision"))
}

fn extract_verified_archive(
    archive_path: &Path,
    parent: &Path,
    asset_name: &str,
) -> Result<PendingArtifactPath> {
    let (pending, mut output) = create_blocking_pending_file(parent, "staged")?;
    let extracted = if asset_name.ends_with(".zip") {
        extract_single_zip(archive_path, &mut output)?
    } else if asset_name.ends_with(".tar.gz") {
        extract_single_tar(archive_path, &mut output)?
    } else {
        return Err(sidecar_error("artifact_archive_format_unsupported"));
    };
    if extracted == 0 || extracted > MAX_ENGINE_BINARY_BYTES {
        return Err(sidecar_error("artifact_binary_size_invalid"));
    }
    output
        .sync_all()
        .map_err(|_| sidecar_error("artifact_staged_binary_sync_failed"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = output
            .metadata()
            .map_err(|_| sidecar_error("artifact_staged_binary_metadata_failed"))?
            .permissions();
        permissions.set_mode(0o755);
        output
            .set_permissions(permissions)
            .map_err(|_| sidecar_error("artifact_staged_binary_permissions_failed"))?;
    }
    drop(output);
    Ok(pending)
}

fn create_blocking_pending_file(
    parent: &Path,
    label: &str,
) -> Result<(PendingArtifactPath, std::fs::File)> {
    for _ in 0..8 {
        let path = parent.join(format!(".tandem-engine-{}-{label}", Uuid::new_v4()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((PendingArtifactPath::new(path), file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(sidecar_error("artifact_staged_file_create_failed")),
        }
    }
    Err(sidecar_error("artifact_staged_file_collision"))
}

fn extract_single_zip(archive_path: &Path, output: &mut std::fs::File) -> Result<u64> {
    let archive_file =
        std::fs::File::open(archive_path).map_err(|_| sidecar_error("artifact_zip_open_failed"))?;
    let mut archive =
        zip::ZipArchive::new(archive_file).map_err(|_| sidecar_error("artifact_zip_invalid"))?;
    if archive.len() != 1 {
        return Err(sidecar_error("artifact_zip_entry_count_invalid"));
    }
    let mut entry = archive
        .by_index(0)
        .map_err(|_| sidecar_error("artifact_zip_entry_invalid"))?;
    if entry.is_dir()
        || entry.name() != super::get_binary_name()
        || entry.size() == 0
        || entry.size() > MAX_ENGINE_BINARY_BYTES
        || entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
    {
        return Err(sidecar_error("artifact_zip_entry_rejected"));
    }
    copy_binary_bounded(&mut entry, output)
}

fn extract_single_tar(archive_path: &Path, output: &mut std::fs::File) -> Result<u64> {
    let archive_file =
        std::fs::File::open(archive_path).map_err(|_| sidecar_error("artifact_tar_open_failed"))?;
    let decoder = GzDecoder::new(archive_file);
    let mut archive = tar::Archive::new(decoder);
    let mut entries = archive
        .entries()
        .map_err(|_| sidecar_error("artifact_tar_invalid"))?;
    let mut entry = entries
        .next()
        .ok_or_else(|| sidecar_error("artifact_tar_empty"))?
        .map_err(|_| sidecar_error("artifact_tar_entry_invalid"))?;
    let path = entry
        .path()
        .map_err(|_| sidecar_error("artifact_tar_path_invalid"))?
        .into_owned();
    let size = entry
        .header()
        .size()
        .map_err(|_| sidecar_error("artifact_tar_size_invalid"))?;
    if path != Path::new(super::get_binary_name())
        || !entry.header().entry_type().is_file()
        || size == 0
        || size > MAX_ENGINE_BINARY_BYTES
    {
        return Err(sidecar_error("artifact_tar_entry_rejected"));
    }
    let copied = copy_binary_bounded(&mut entry, output)?;
    if entries.next().is_some() {
        return Err(sidecar_error("artifact_tar_entry_count_invalid"));
    }
    Ok(copied)
}

fn copy_binary_bounded(reader: &mut impl Read, output: &mut std::fs::File) -> Result<u64> {
    let mut copied = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| sidecar_error("artifact_entry_read_failed"))?;
        if read == 0 {
            break;
        }
        copied = copied
            .checked_add(read as u64)
            .ok_or_else(|| sidecar_error("artifact_binary_too_large"))?;
        if copied > MAX_ENGINE_BINARY_BYTES {
            return Err(sidecar_error("artifact_binary_too_large"));
        }
        output
            .write_all(&buffer[..read])
            .map_err(|_| sidecar_error("artifact_staged_binary_write_failed"))?;
    }
    Ok(copied)
}

pub(super) fn activate_verified_engine(
    mut prepared: PreparedEngineInstall,
    install_path: &Path,
) -> Result<PathBuf> {
    let parent = install_path
        .parent()
        .ok_or_else(|| sidecar_error("artifact_install_path_invalid"))?;
    let backup = parent.join(format!(".tandem-engine-{}-rollback", Uuid::new_v4()));
    let had_existing = match std::fs::symlink_metadata(install_path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                return Err(sidecar_error("artifact_install_target_rejected"));
            }
            rename_with_retry(install_path, &backup, "artifact_install_backup_failed")?;
            if let Err(sync_error) = sync_parent(parent) {
                rename_with_retry(&backup, install_path, "artifact_install_rollback_failed")?;
                if let Err(rollback_sync_error) = sync_parent(parent) {
                    tracing::warn!(
                        error = ?rollback_sync_error,
                        "Runtime artifact backup rollback directory sync failed"
                    );
                }
                return Err(sync_error);
            }
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => return Err(sidecar_error("artifact_install_target_metadata_failed")),
    };

    if let Err(error) = rename_with_retry(
        prepared.staged.path(),
        install_path,
        "artifact_install_activation_failed",
    ) {
        if had_existing {
            rename_with_retry(&backup, install_path, "artifact_install_rollback_failed")?;
            sync_parent(parent)?;
        }
        return Err(error);
    }
    prepared.staged.disarm();
    sync_parent(parent)?;

    if !probe_binary_version(install_path)
        .as_deref()
        .is_ok_and(|reported| version_matches(reported, &prepared.expected_version))
    {
        let cleanup_error = std::fs::remove_file(install_path).err();
        let rollback_error = had_existing
            .then(|| rename_with_retry(&backup, install_path, "artifact_install_rollback_failed"))
            .and_then(Result::err);
        let sync_error = sync_parent(parent).err();
        if let Some(cleanup_error) = cleanup_error {
            tracing::warn!(
                error = ?cleanup_error,
                rollback_error = ?rollback_error,
                sync_error = ?sync_error,
                "Rejected runtime artifact cleanup failed after rollback was attempted"
            );
            return Err(sidecar_error("artifact_install_rejected_remove_failed"));
        }
        if let Some(rollback_error) = rollback_error {
            return Err(rollback_error);
        }
        if let Some(sync_error) = sync_error {
            return Err(sync_error);
        }
        return Err(sidecar_error("artifact_install_version_probe_failed"));
    }

    if had_existing {
        std::fs::remove_file(&backup)
            .map_err(|_| sidecar_error("artifact_install_backup_cleanup_failed"))?;
        sync_parent(parent)?;
    }
    Ok(install_path.to_path_buf())
}

fn rename_with_retry(source: &Path, destination: &Path, code: &'static str) -> Result<()> {
    let mut last_error = None;
    for attempt in 0..5 {
        match std::fs::rename(source, destination) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                if attempt < 4 {
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }
    }
    tracing::warn!(error = ?last_error, "Runtime artifact rename exhausted retries");
    Err(sidecar_error(code))
}

fn probe_binary_version(path: &Path) -> Result<String> {
    let output = std::process::Command::new(path)
        .arg("--version")
        .output()
        .map_err(|_| sidecar_error("artifact_version_probe_execute_failed"))?;
    if !output.status.success() {
        return Err(sidecar_error("artifact_version_probe_status_failed"));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(|_| sidecar_error("artifact_version_probe_output_invalid"))
}

fn version_matches(reported: &str, expected: &str) -> bool {
    let prefixed = format!("v{expected}");
    reported
        .split(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+' | '_'))
        })
        .any(|token| token == expected || token == prefixed)
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<()> {
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| sidecar_error("artifact_install_directory_sync_failed"))
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> Result<()> {
    Ok(())
}

fn sidecar_error(message: impl Into<String>) -> TandemError {
    TandemError::Sidecar(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_policy_accepts_only_https_github_hosts() {
        assert!(trusted_release_redirect(
            &reqwest::Url::parse("https://release-assets.githubusercontent.com/object").unwrap()
        ));
        assert!(!trusted_release_redirect(
            &reqwest::Url::parse("https://example.com/object").unwrap()
        ));
        assert!(!trusted_release_redirect(
            &reqwest::Url::parse("http://github.com/object").unwrap()
        ));
    }

    #[test]
    fn exact_release_asset_rejects_duplicates() {
        let release = GitHubRelease {
            tag_name: "v0.7.1".to_string(),
            draft: false,
            prerelease: false,
            assets: vec![
                GitHubAsset {
                    name: ARTIFACT_MANIFEST_FILENAME.to_string(),
                    browser_download_url: "https://example.com/one".to_string(),
                    size: 1,
                },
                GitHubAsset {
                    name: ARTIFACT_MANIFEST_FILENAME.to_string(),
                    browser_download_url: "https://example.com/two".to_string(),
                    size: 1,
                },
            ],
        };
        assert_eq!(
            exact_release_asset(&release, ARTIFACT_MANIFEST_FILENAME)
                .unwrap_err()
                .to_string(),
            "Sidecar error: release_duplicate_asset"
        );
    }

    #[test]
    fn exact_release_asset_rejects_untrusted_initial_url() {
        let release = GitHubRelease {
            tag_name: "v0.7.1".to_string(),
            draft: false,
            prerelease: false,
            assets: vec![GitHubAsset {
                name: ARTIFACT_MANIFEST_FILENAME.to_string(),
                browser_download_url: "https://example.com/manifest".to_string(),
                size: 1,
            }],
        };
        assert_eq!(
            exact_release_asset(&release, ARTIFACT_MANIFEST_FILENAME)
                .unwrap_err()
                .to_string(),
            "Sidecar error: release_asset_url_untrusted"
        );
    }

    #[test]
    fn zip_extraction_requires_one_exact_regular_binary() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("engine.zip");
        let archive_file = std::fs::File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(archive_file);
        archive
            .start_file(
                super::super::get_binary_name(),
                zip::write::FileOptions::default(),
            )
            .unwrap();
        archive.write_all(b"engine-binary").unwrap();
        archive.finish().unwrap();

        let staged =
            extract_verified_archive(&archive_path, directory.path(), "tandem-engine-test.zip")
                .unwrap();
        assert_eq!(std::fs::read(staged.path()).unwrap(), b"engine-binary");
    }

    #[test]
    fn zip_extraction_rejects_extra_or_traversal_entries() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("engine.zip");
        let archive_file = std::fs::File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(archive_file);
        archive
            .start_file(
                super::super::get_binary_name(),
                zip::write::FileOptions::default(),
            )
            .unwrap();
        archive.write_all(b"engine-binary").unwrap();
        archive
            .start_file("../unexpected", zip::write::FileOptions::default())
            .unwrap();
        archive.write_all(b"unexpected").unwrap();
        archive.finish().unwrap();

        assert_eq!(
            extract_verified_archive(&archive_path, directory.path(), "tandem-engine-test.zip",)
                .unwrap_err()
                .to_string(),
            "Sidecar error: artifact_zip_entry_count_invalid"
        );
    }

    #[test]
    fn version_match_is_token_exact() {
        assert!(version_matches("tandem-engine 0.7.1", "0.7.1"));
        assert!(version_matches("tandem-engine v0.7.1", "0.7.1"));
        assert!(!version_matches("tandem-engine 0.7.10", "0.7.1"));
    }

    #[cfg(unix)]
    #[test]
    fn failed_version_probe_restores_previous_binary() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let install_path = directory.path().join(super::super::get_binary_name());
        std::fs::write(&install_path, b"old-binary").unwrap();
        let (staged, mut file) =
            create_blocking_pending_file(directory.path(), "staged-test").unwrap();
        file.write_all(b"#!/bin/sh\necho tandem-engine 9.9.9\n")
            .unwrap();
        let mut permissions = file.metadata().unwrap().permissions();
        permissions.set_mode(0o755);
        file.set_permissions(permissions).unwrap();
        drop(file);
        let prepared = PreparedEngineInstall {
            staged,
            downloaded_bytes: 42,
            expected_version: "0.7.1".to_string(),
        };

        assert_eq!(
            activate_verified_engine(prepared, &install_path)
                .unwrap_err()
                .to_string(),
            "Sidecar error: artifact_install_version_probe_failed"
        );
        assert_eq!(std::fs::read(&install_path).unwrap(), b"old-binary");
    }
}
