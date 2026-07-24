// Browser-sidecar release artifacts are authenticated before any archive
// content is extracted, made executable, or activated.

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

async fn install_verified_browser_sidecar<F>(
    config: &BrowserConfig,
    authorize_write: F,
) -> anyhow::Result<BrowserSidecarInstallResult>
where
    F: Fn() -> anyhow::Result<()> + Send + Sync,
{
    authorize_write()?;
    let version = env!("CARGO_PKG_VERSION").to_string();
    let expected_release = format!("v{version}");
    let client = browser_release_client()?;
    let release = fetch_verified_browser_release(&client, &version).await?;
    if release.tag_name != expected_release {
        anyhow::bail!("artifact_release_mismatch");
    }

    let manifest_asset = exact_release_asset(&release, ARTIFACT_MANIFEST_FILENAME)?;
    let signature_asset = exact_release_asset(&release, ARTIFACT_MANIFEST_SIGNATURE_FILENAME)?;
    let manifest_bytes = download_small_release_asset(
        &client,
        manifest_asset,
        MAX_ARTIFACT_MANIFEST_BYTES,
    )
    .await?;
    let signature_bytes = download_small_release_asset(
        &client,
        signature_asset,
        MAX_ARTIFACT_SIGNATURE_BYTES,
    )
    .await?;
    let verified = verify_artifact_manifest(
        &manifest_bytes,
        &signature_bytes,
        ArtifactManifestExpectation {
            source_repository: RELEASE_REPO,
            release: &expected_release,
            version: &version,
            allowed_workflows: BROWSER_RELEASE_WORKFLOWS,
        },
    )
    .map_err(|error| anyhow!(error.to_string()))?;

    let asset_name = browser_release_asset_name()?;
    let platform = current_artifact_platform().map_err(|error| anyhow!(error.to_string()))?;
    let architecture =
        current_artifact_architecture().map_err(|error| anyhow!(error.to_string()))?;
    let manifest_entry = verified
        .artifact(
            ArtifactKind::Browser,
            platform,
            architecture,
            &asset_name,
        )
        .map_err(|error| anyhow!(error.to_string()))?;
    let asset = exact_release_asset(&release, &asset_name)?;
    if asset.size != manifest_entry.length {
        anyhow::bail!("artifact_metadata_length_mismatch");
    }

    let install_path = sidecar_install_path(config)?;
    let parent = install_path
        .parent()
        .ok_or_else(|| anyhow!("browser_install_path_invalid"))?;
    authorize_write()?;
    fs::create_dir_all(parent)
        .await
        .context("browser_install_directory_create_failed")?;
    let archive = download_verified_browser_archive(&client, asset, manifest_entry, parent).await?;

    authorize_write()?;
    let archive_path = archive.path().to_path_buf();
    let parent = parent.to_path_buf();
    let asset_name_for_extract = asset_name.clone();
    let staged = tokio::task::spawn_blocking(move || {
        extract_verified_browser_archive(&archive_path, &parent, &asset_name_for_extract)
    })
    .await
    .context("browser_archive_extract_task_failed")??;
    drop(archive);

    authorize_write()?;
    let install_for_activation = install_path.clone();
    let version_for_activation = version.clone();
    let installed = tokio::task::spawn_blocking(move || {
        activate_browser_binary(staged, &install_for_activation, &version_for_activation)
    })
    .await
    .context("browser_artifact_activation_task_failed")??;

    let status = evaluate_browser_status(config.clone());
    Ok(BrowserSidecarInstallResult {
        version,
        asset_name,
        installed_path: installed.to_string_lossy().to_string(),
        downloaded_bytes: manifest_entry.length,
        status,
    })
}

fn browser_release_client() -> anyhow::Result<reqwest::Client> {
    let redirect = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 4 {
            return attempt.stop();
        }
        if trusted_browser_release_redirect(attempt.url()) {
            attempt.follow()
        } else {
            attempt.stop()
        }
    });
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(5 * 60))
        .redirect(redirect)
        .user_agent(BROWSER_INSTALL_USER_AGENT)
        .build()
        .context("browser_release_client_create_failed")
}

fn trusted_browser_release_redirect(url: &reqwest::Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    url.host_str().is_some_and(|host| {
        host == "github.com"
            || host == "api.github.com"
            || host.ends_with(".githubusercontent.com")
    })
}

async fn fetch_verified_browser_release(
    client: &reqwest::Client,
    version: &str,
) -> anyhow::Result<GitHubRelease> {
    let base = std::env::var(RELEASES_URL_ENV)
        .unwrap_or_else(|_| format!("https://api.github.com/repos/{RELEASE_REPO}/releases/tags"));
    let url = format!("{}/v{}", base.trim_end_matches('/'), version);
    let response = client
        .get(url)
        .send()
        .await
        .context("release_metadata_request_failed")?;
    if !response.status().is_success() {
        anyhow::bail!("release_lookup_failed: {}", response.status());
    }
    let body = bounded_response_bytes(response, MAX_RELEASE_METADATA_BYTES, None).await?;
    serde_json::from_slice::<GitHubRelease>(&body)
        .context("release_metadata_invalid")
}

fn exact_release_asset<'a>(
    release: &'a GitHubRelease,
    name: &str,
) -> anyhow::Result<&'a GitHubAsset> {
    let mut matches = release.assets.iter().filter(|asset| asset.name == name);
    let asset = matches
        .next()
        .ok_or_else(|| anyhow!("release_required_asset_missing"))?;
    if matches.next().is_some() {
        anyhow::bail!("release_duplicate_asset");
    }
    let trusted_url = reqwest::Url::parse(&asset.browser_download_url)
        .ok()
        .is_some_and(|url| trusted_browser_release_redirect(&url));
    if !trusted_url {
        anyhow::bail!("release_asset_url_untrusted");
    }
    Ok(asset)
}

async fn download_small_release_asset(
    client: &reqwest::Client,
    asset: &GitHubAsset,
    limit: usize,
) -> anyhow::Result<Vec<u8>> {
    if asset.size == 0 || asset.size > limit as u64 {
        anyhow::bail!("release_metadata_asset_size_invalid");
    }
    let response = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .context("release_metadata_asset_request_failed")?;
    if !response.status().is_success() {
        anyhow::bail!("release_metadata_asset_download_failed");
    }
    bounded_response_bytes(response, limit, Some(asset.size)).await
}

async fn bounded_response_bytes(
    mut response: reqwest::Response,
    limit: usize,
    expected_length: Option<u64>,
) -> anyhow::Result<Vec<u8>> {
    if let Some(content_length) = response.content_length() {
        if content_length > limit as u64
            || expected_length.is_some_and(|expected| expected != content_length)
        {
            anyhow::bail!("release_response_length_invalid");
        }
    }
    let mut body = Vec::with_capacity(
        expected_length
            .unwrap_or_default()
            .min(limit as u64) as usize,
    );
    while let Some(chunk) = response
        .chunk()
        .await
        .context("release_response_read_failed")?
    {
        if body.len().saturating_add(chunk.len()) > limit {
            anyhow::bail!("release_response_too_large");
        }
        body.extend_from_slice(&chunk);
    }
    if expected_length.is_some_and(|expected| expected != body.len() as u64) {
        anyhow::bail!("release_response_length_invalid");
    }
    Ok(body)
}

async fn download_verified_browser_archive(
    client: &reqwest::Client,
    asset: &GitHubAsset,
    manifest_entry: &ArtifactManifestEntry,
    parent: &Path,
) -> anyhow::Result<PendingArtifactPath> {
    let mut verifier = ArtifactDigestVerifier::new(manifest_entry, MAX_BROWSER_ARCHIVE_BYTES)
        .map_err(|error| anyhow!(error.to_string()))?;
    let response = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .context("browser_artifact_download_request_failed")?;
    if !response.status().is_success() {
        anyhow::bail!("browser_artifact_download_failed");
    }
    if response
        .content_length()
        .is_some_and(|length| length != verifier.expected_length())
    {
        anyhow::bail!("artifact_length_mismatch");
    }

    let (pending, mut file) = create_pending_browser_file(parent, "archive").await?;
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .context("browser_artifact_download_read_failed")?
    {
        verifier
            .update(&chunk)
            .map_err(|error| anyhow!(error.to_string()))?;
        file.write_all(&chunk)
            .await
            .context("browser_artifact_write_failed")?;
    }
    file.flush()
        .await
        .context("browser_artifact_flush_failed")?;
    file.sync_all()
        .await
        .context("browser_artifact_sync_failed")?;
    drop(file);
    verifier
        .finalize()
        .map_err(|error| anyhow!(error.to_string()))?;
    Ok(pending)
}

async fn create_pending_browser_file(
    parent: &Path,
    label: &str,
) -> anyhow::Result<(PendingArtifactPath, fs::File)> {
    for _ in 0..8 {
        let path = parent.join(format!(
            ".tandem-browser-{}-{label}",
            Uuid::new_v4()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
        {
            Ok(file) => return Ok((PendingArtifactPath::new(path), file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => anyhow::bail!("browser_pending_file_create_failed"),
        }
    }
    anyhow::bail!("browser_pending_file_collision")
}

fn extract_verified_browser_archive(
    archive_path: &Path,
    parent: &Path,
    asset_name: &str,
) -> anyhow::Result<PendingArtifactPath> {
    let (pending, mut output) = create_blocking_pending_browser_file(parent, "staged")?;
    let extracted = if asset_name.ends_with(".zip") {
        extract_single_browser_zip(archive_path, &mut output)?
    } else if asset_name.ends_with(".tar.gz") {
        extract_single_browser_tar(archive_path, &mut output)?
    } else {
        anyhow::bail!("browser_archive_format_unsupported");
    };
    if extracted == 0 || extracted > MAX_BROWSER_BINARY_BYTES {
        anyhow::bail!("browser_archive_binary_size_invalid");
    }
    output
        .sync_all()
        .context("browser_staged_binary_sync_failed")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = output
            .metadata()
            .context("browser_staged_binary_metadata_failed")?
            .permissions();
        permissions.set_mode(0o755);
        output
            .set_permissions(permissions)
            .context("browser_staged_binary_permissions_failed")?;
    }
    drop(output);
    Ok(pending)
}

fn create_blocking_pending_browser_file(
    parent: &Path,
    label: &str,
) -> anyhow::Result<(PendingArtifactPath, std::fs::File)> {
    for _ in 0..8 {
        let path = parent.join(format!(
            ".tandem-browser-{}-{label}",
            Uuid::new_v4()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((PendingArtifactPath::new(path), file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => anyhow::bail!("browser_staged_file_create_failed"),
        }
    }
    anyhow::bail!("browser_staged_file_collision")
}

fn extract_single_browser_zip(
    archive_path: &Path,
    output: &mut std::fs::File,
) -> anyhow::Result<u64> {
    let archive_file = std::fs::File::open(archive_path)
        .context("browser_zip_open_failed")?;
    let mut archive = zip::ZipArchive::new(archive_file)
        .context("browser_zip_invalid")?;
    if archive.len() != 1 {
        anyhow::bail!("browser_zip_entry_count_invalid");
    }
    let mut entry = archive.by_index(0).context("browser_zip_entry_invalid")?;
    if entry.is_dir()
        || entry.name() != sidecar_binary_name()
        || entry.size() == 0
        || entry.size() > MAX_BROWSER_BINARY_BYTES
        || entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
    {
        anyhow::bail!("browser_zip_entry_rejected");
    }
    copy_browser_binary_bounded(&mut entry, output)
}

fn extract_single_browser_tar(
    archive_path: &Path,
    output: &mut std::fs::File,
) -> anyhow::Result<u64> {
    let archive_file = std::fs::File::open(archive_path)
        .context("browser_tar_open_failed")?;
    let decoder = GzDecoder::new(archive_file);
    let mut archive = tar::Archive::new(decoder);
    let mut entries = archive.entries().context("browser_tar_invalid")?;
    let mut entry = entries
        .next()
        .ok_or_else(|| anyhow!("browser_tar_empty"))?
        .context("browser_tar_entry_invalid")?;
    let path = entry.path().context("browser_tar_path_invalid")?.into_owned();
    let size = entry.header().size().context("browser_tar_size_invalid")?;
    if path != Path::new(sidecar_binary_name())
        || !entry.header().entry_type().is_file()
        || size == 0
        || size > MAX_BROWSER_BINARY_BYTES
    {
        anyhow::bail!("browser_tar_entry_rejected");
    }
    let copied = copy_browser_binary_bounded(&mut entry, output)?;
    if entries.next().is_some() {
        anyhow::bail!("browser_tar_entry_count_invalid");
    }
    Ok(copied)
}

fn copy_browser_binary_bounded(
    reader: &mut impl std::io::Read,
    output: &mut std::fs::File,
) -> anyhow::Result<u64> {
    use std::io::Write;

    let mut copied = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .context("browser_archive_entry_read_failed")?;
        if read == 0 {
            break;
        }
        copied = copied
            .checked_add(read as u64)
            .ok_or_else(|| anyhow!("browser_archive_binary_too_large"))?;
        if copied > MAX_BROWSER_BINARY_BYTES {
            anyhow::bail!("browser_archive_binary_too_large");
        }
        output
            .write_all(&buffer[..read])
            .context("browser_staged_binary_write_failed")?;
    }
    Ok(copied)
}

fn activate_browser_binary(
    mut staged: PendingArtifactPath,
    install_path: &Path,
    expected_version: &str,
) -> anyhow::Result<PathBuf> {
    let parent = install_path
        .parent()
        .ok_or_else(|| anyhow!("browser_install_path_invalid"))?;
    let backup = parent.join(format!(
        ".tandem-browser-{}-rollback",
        Uuid::new_v4()
    ));
    let had_existing = match std::fs::symlink_metadata(install_path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                anyhow::bail!("browser_install_target_rejected");
            }
            rename_browser_with_retry(
                install_path,
                &backup,
                "browser_install_backup_failed",
            )?;
            sync_browser_install_parent(parent)?;
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => anyhow::bail!("browser_install_target_metadata_failed"),
    };

    if let Err(error) = rename_browser_with_retry(
        staged.path(),
        install_path,
        "browser_install_activation_failed",
    ) {
        if had_existing {
            rename_browser_with_retry(
                &backup,
                install_path,
                "browser_install_rollback_failed",
            )?;
            sync_browser_install_parent(parent)?;
        }
        return Err(error);
    }
    staged.disarm();
    sync_browser_install_parent(parent)?;

    let probe = probe_binary_version(install_path);
    if !probe
        .as_deref()
        .is_ok_and(|reported| browser_version_matches(reported, expected_version))
    {
        std::fs::remove_file(install_path)
            .context("browser_install_rejected_remove_failed")?;
        if had_existing {
            rename_browser_with_retry(
                &backup,
                install_path,
                "browser_install_rollback_failed",
            )?;
        }
        sync_browser_install_parent(parent)?;
        anyhow::bail!("browser_install_version_probe_failed");
    }

    if had_existing {
        std::fs::remove_file(&backup)
            .context("browser_install_backup_cleanup_failed")?;
        sync_browser_install_parent(parent)?;
    }
    Ok(install_path.to_path_buf())
}

fn rename_browser_with_retry(source: &Path, destination: &Path, code: &str) -> anyhow::Result<()> {
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
    tracing::warn!(error = ?last_error, "Browser artifact rename exhausted retries");
    anyhow::bail!(code.to_string())
}

#[cfg(unix)]
fn sync_browser_install_parent(parent: &Path) -> anyhow::Result<()> {
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .context("browser_install_directory_sync_failed")
}

#[cfg(not(unix))]
fn sync_browser_install_parent(_parent: &Path) -> anyhow::Result<()> {
    Ok(())
}

fn browser_version_matches(reported: &str, expected: &str) -> bool {
    let prefixed = format!("v{expected}");
    reported
        .split(|character: char| {
            !(character.is_ascii_alphanumeric()
                || matches!(character, '.' | '-' | '+' | '_'))
        })
        .any(|token| token == expected || token == prefixed)
}

#[cfg(test)]
mod browser_artifact_install_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn redirect_policy_accepts_only_https_github_hosts() {
        assert!(trusted_browser_release_redirect(
            &reqwest::Url::parse("https://release-assets.githubusercontent.com/object").unwrap()
        ));
        assert!(!trusted_browser_release_redirect(
            &reqwest::Url::parse("https://example.com/object").unwrap()
        ));
        assert!(!trusted_browser_release_redirect(
            &reqwest::Url::parse("http://github.com/object").unwrap()
        ));
    }

    #[test]
    fn exact_release_asset_rejects_duplicates() {
        let release = GitHubRelease {
            tag_name: "v0.7.1".to_string(),
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
            "release_duplicate_asset"
        );
    }

    #[test]
    fn exact_release_asset_rejects_untrusted_initial_url() {
        let release = GitHubRelease {
            tag_name: "v0.7.1".to_string(),
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
            "release_asset_url_untrusted"
        );
    }

    #[test]
    fn zip_extraction_requires_one_exact_regular_binary() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("browser.zip");
        let archive_file = std::fs::File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(archive_file);
        archive
            .start_file(
                sidecar_binary_name(),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(b"browser-binary").unwrap();
        archive.finish().unwrap();

        let staged = extract_verified_browser_archive(
            &archive_path,
            directory.path(),
            "tandem-browser-test.zip",
        )
        .unwrap();
        assert_eq!(std::fs::read(staged.path()).unwrap(), b"browser-binary");
    }

    #[test]
    fn zip_extraction_rejects_extra_or_traversal_entries() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("browser.zip");
        let archive_file = std::fs::File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(archive_file);
        archive
            .start_file(
                sidecar_binary_name(),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(b"browser-binary").unwrap();
        archive
            .start_file(
                "../unexpected",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(b"unexpected").unwrap();
        archive.finish().unwrap();

        assert_eq!(
            extract_verified_browser_archive(
                &archive_path,
                directory.path(),
                "tandem-browser-test.zip",
            )
            .unwrap_err()
            .to_string(),
            "browser_zip_entry_count_invalid"
        );
    }

    #[test]
    fn version_match_is_token_exact() {
        assert!(browser_version_matches("tandem-browser 0.7.1", "0.7.1"));
        assert!(browser_version_matches("tandem-browser v0.7.1", "0.7.1"));
        assert!(!browser_version_matches("tandem-browser 0.7.10", "0.7.1"));
    }

    #[cfg(unix)]
    #[test]
    fn failed_version_probe_restores_previous_binary() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let install_path = directory.path().join(sidecar_binary_name());
        std::fs::write(&install_path, b"old-binary").unwrap();
        let (staged, mut file) =
            create_blocking_pending_browser_file(directory.path(), "staged-test").unwrap();
        file.write_all(b"#!/bin/sh\necho tandem-browser 9.9.9\n")
            .unwrap();
        let mut permissions = file.metadata().unwrap().permissions();
        permissions.set_mode(0o755);
        file.set_permissions(permissions).unwrap();
        drop(file);

        assert_eq!(
            activate_browser_binary(staged, &install_path, "0.7.1")
                .unwrap_err()
                .to_string(),
            "browser_install_version_probe_failed"
        );
        assert_eq!(std::fs::read(&install_path).unwrap(), b"old-binary");
    }
}
