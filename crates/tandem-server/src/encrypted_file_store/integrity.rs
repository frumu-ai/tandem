use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use anyhow::Context;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::{
    crypto, ProtectedFileCrypto, ProtectedJsonRecord, ProtectedRecordContext,
    ProtectedStoreContext, AUTHENTICATED_COLLECTION_PREFIX, AUTHENTICATED_JSONL_PREFIX,
    AUTHENTICATED_STORE_VERSION, SCOPED_COLLECTION_PREFIX,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScopedEncryptedCollection {
    version: u32,
    records: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScopedJsonEntry {
    key: String,
    value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthenticatedCollectionEntry {
    key: String,
    context: ProtectedRecordContext,
    stored_record: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthenticatedCollection {
    version: u32,
    store_id: String,
    generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_digest: Option<String>,
    records: Vec<AuthenticatedCollectionEntry>,
    digest: String,
}

#[derive(Debug, Clone, Serialize)]
struct CollectionForDigest<'a> {
    version: u32,
    store_id: &'a str,
    generation: u64,
    previous_digest: &'a Option<String>,
    records: &'a [AuthenticatedCollectionEntry],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AuthenticatedStoreHead {
    version: u32,
    store_id: String,
    generation: u64,
    digest: String,
}

/// Persistent local witness for the latest authenticated generation on disk.
///
/// This catches missing files and data+head rollback while the witness remains
/// current. It is not a cryptographic rollback root: an actor that coordinates
/// deletion or rollback of data, head, and witness can defeat the local check
/// after the in-memory cache is lost. That requires an external monotonic root.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AuthenticatedStoreState {
    version: u32,
    kind: String,
    store_id: String,
    generation: u64,
    digest: String,
}

const INITIALIZED_STATE_KIND: &str = "tandem-protected-store-initialized";

static CACHED_HEADS: OnceLock<tokio::sync::Mutex<HashMap<String, AuthenticatedStoreHead>>> =
    OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthenticatedJsonlFrame {
    version: u32,
    store_id: String,
    sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_digest: Option<String>,
    context: ProtectedRecordContext,
    stored_record: String,
    digest: String,
}

#[derive(Debug, Clone, Serialize)]
struct JsonlFrameForDigest<'a> {
    version: u32,
    store_id: &'a str,
    sequence: u64,
    previous_digest: &'a Option<String>,
    context: &'a ProtectedRecordContext,
    stored_record: &'a str,
}

#[derive(Debug)]
struct DecryptedCollection {
    plaintext_json: String,
    generation: u64,
    digest: String,
}

#[derive(Debug)]
struct DecryptedJsonl {
    lines: Vec<String>,
    generation: u64,
    digest: String,
    authenticated: bool,
}

impl ProtectedFileCrypto {
    fn plaintext_json(records: &[ProtectedJsonRecord]) -> anyhow::Result<String> {
        let mut values = BTreeMap::new();
        for record in records {
            Self::validate_context(&record.context)?;
            anyhow::ensure!(
                values
                    .insert(record.key.clone(), record.value.clone())
                    .is_none(),
                "duplicate protected JSON record key `{}`",
                record.key
            );
        }
        serde_json::to_string_pretty(&values).map_err(Into::into)
    }

    fn encrypt_json_collection(
        &self,
        records: &[ProtectedJsonRecord],
        store: &ProtectedStoreContext,
        generation: u64,
        previous_digest: Option<String>,
    ) -> anyhow::Result<(String, AuthenticatedStoreHead)> {
        anyhow::ensure!(
            !self.provider.is_plaintext(),
            "authenticated collection encryption requires an encrypted provider"
        );
        let mut sorted = records.iter().collect::<Vec<_>>();
        sorted.sort_by(|left, right| left.key.cmp(&right.key));
        let mut stored_records = Vec::with_capacity(sorted.len());
        for record in sorted {
            Self::validate_context(&record.context)?;
            let plaintext = serde_json::to_string(&ScopedJsonEntry {
                key: record.key.clone(),
                value: record.value.clone(),
            })?;
            stored_records.push(AuthenticatedCollectionEntry {
                key: record.key.clone(),
                context: record.context.clone(),
                stored_record: self.encrypt_record(&plaintext, &record.context)?,
            });
        }
        let mut collection = AuthenticatedCollection {
            version: AUTHENTICATED_STORE_VERSION,
            store_id: store.store_id.clone(),
            generation,
            previous_digest,
            records: stored_records,
            digest: String::new(),
        };
        collection.digest = collection_digest(&collection)?;
        let manifest =
            self.encrypt_record(&serde_json::to_string(&collection)?, &store.manifest)?;
        let head = AuthenticatedStoreHead {
            version: AUTHENTICATED_STORE_VERSION,
            store_id: store.store_id.clone(),
            generation,
            digest: collection.digest.clone(),
        };
        Ok((format!("{AUTHENTICATED_COLLECTION_PREFIX}{manifest}"), head))
    }

    fn decrypt_json_collection(
        &self,
        stored: &str,
        store: &ProtectedStoreContext,
    ) -> anyhow::Result<DecryptedCollection> {
        let encoded = stored
            .strip_prefix(AUTHENTICATED_COLLECTION_PREFIX)
            .context("protected JSON collection is missing its authenticated manifest")?;
        let manifest = self.decrypt_record(encoded, &store.manifest)?;
        let collection = serde_json::from_str::<AuthenticatedCollection>(&manifest)
            .context("parse authenticated protected JSON collection")?;
        anyhow::ensure!(
            collection.version == AUTHENTICATED_STORE_VERSION,
            "unsupported protected JSON collection version {}",
            collection.version
        );
        anyhow::ensure!(
            collection.store_id == store.store_id,
            "protected JSON collection store identity mismatch"
        );
        anyhow::ensure!(
            collection.generation > 0,
            "protected JSON collection generation must be positive"
        );
        anyhow::ensure!(
            (collection.generation == 1 && collection.previous_digest.is_none())
                || (collection.generation > 1 && collection.previous_digest.is_some()),
            "protected JSON collection generation linkage is invalid"
        );
        anyhow::ensure!(
            collection.digest == collection_digest(&collection)?,
            "protected JSON collection digest mismatch"
        );

        let mut values = BTreeMap::new();
        let mut previous_key: Option<&str> = None;
        for stored_record in &collection.records {
            if let Some(previous_key) = previous_key {
                anyhow::ensure!(
                    previous_key < stored_record.key.as_str(),
                    "protected JSON collection membership is duplicated or out of order"
                );
            }
            previous_key = Some(&stored_record.key);
            let plaintext =
                self.decrypt_record(&stored_record.stored_record, &stored_record.context)?;
            let record = serde_json::from_str::<ScopedJsonEntry>(&plaintext)
                .context("parse protected JSON collection record")?;
            anyhow::ensure!(
                record.key == stored_record.key,
                "protected JSON collection key does not match authenticated membership"
            );
            anyhow::ensure!(
                values.insert(record.key.clone(), record.value).is_none(),
                "duplicate protected JSON record key `{}`",
                record.key
            );
        }
        Ok(DecryptedCollection {
            plaintext_json: serde_json::to_string_pretty(&values)?,
            generation: collection.generation,
            digest: collection.digest,
        })
    }

    fn decrypt_legacy_json_document(&self, stored: &str) -> anyhow::Result<String> {
        let Some(encoded) = stored.strip_prefix(SCOPED_COLLECTION_PREFIX) else {
            return self.decrypt_legacy_record(stored);
        };
        anyhow::ensure!(
            !self.provider.is_hosted(),
            "legacy hosted protected collection lacks authenticated expected authorities"
        );
        let collection = serde_json::from_str::<ScopedEncryptedCollection>(encoded)
            .context("parse legacy protected JSON record collection")?;
        let mut values = BTreeMap::new();
        for stored_record in collection.records {
            let plaintext = self.decrypt_legacy_record(&stored_record)?;
            let record = serde_json::from_str::<ScopedJsonEntry>(&plaintext)
                .context("parse legacy protected JSON collection record")?;
            anyhow::ensure!(
                values.insert(record.key.clone(), record.value).is_none(),
                "duplicate legacy protected JSON record key `{}`",
                record.key
            );
        }
        serde_json::to_string_pretty(&values).map_err(Into::into)
    }
}

fn collection_digest(collection: &AuthenticatedCollection) -> anyhow::Result<String> {
    digest_json(&CollectionForDigest {
        version: collection.version,
        store_id: &collection.store_id,
        generation: collection.generation,
        previous_digest: &collection.previous_digest,
        records: &collection.records,
    })
}

fn jsonl_frame_digest(frame: &AuthenticatedJsonlFrame) -> anyhow::Result<String> {
    digest_json(&JsonlFrameForDigest {
        version: frame.version,
        store_id: &frame.store_id,
        sequence: frame.sequence,
        previous_digest: &frame.previous_digest,
        context: &frame.context,
        stored_record: &frame.stored_record,
    })
}

fn digest_json(value: &impl Serialize) -> anyhow::Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

fn integrity_head_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "governance-store".to_string());
    path.with_file_name(format!("{file_name}.integrity"))
}

fn initialized_state_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "governance-store".to_string());
    path.with_file_name(format!("{file_name}.integrity.initialized"))
}

fn process_lock_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "governance-store".to_string());
    path.with_file_name(format!("{file_name}.integrity.lock"))
}

struct ProcessWriteLock {
    file: std::fs::File,
}

impl ProcessWriteLock {
    async fn acquire(path: &Path) -> anyhow::Result<Self> {
        let lock_path = process_lock_path(path);
        tokio::task::spawn_blocking(move || {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .open(&lock_path)
                .with_context(|| {
                    format!("open protected store process lock {}", lock_path.display())
                })?;
            file.lock_exclusive().with_context(|| {
                format!(
                    "acquire protected store process lock {}",
                    lock_path.display()
                )
            })?;
            Ok(Self { file })
        })
        .await
        .context("join protected store process-lock acquisition")?
    }
}

impl Drop for ProcessWriteLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

async fn path_lock_for(path: &Path) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
        OnceLock::new();
    let locks = LOCKS.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()));
    let mut guard = locks.lock().await;
    guard
        .entry(path.to_string_lossy().into_owned())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

async fn validate_cached_head(path: &Path, head: &AuthenticatedStoreHead) -> anyhow::Result<()> {
    let heads = CACHED_HEADS.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()));
    let mut guard = heads.lock().await;
    let key = path.to_string_lossy().into_owned();
    if let Some(previous) = guard.get(&key) {
        anyhow::ensure!(
            head.generation >= previous.generation,
            "protected store rollback detected: generation {} is older than trusted generation {}",
            head.generation,
            previous.generation
        );
        if head.generation == previous.generation {
            anyhow::ensure!(
                head.digest == previous.digest,
                "protected store replay detected at generation {}",
                head.generation
            );
        }
    }
    guard.insert(key, head.clone());
    Ok(())
}

async fn rollback_cached_head(
    path: &Path,
    failed: &AuthenticatedStoreHead,
    previous: Option<&AuthenticatedStoreHead>,
) -> anyhow::Result<()> {
    let heads = CACHED_HEADS.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()));
    let mut guard = heads.lock().await;
    let key = path.to_string_lossy().into_owned();
    anyhow::ensure!(
        guard.get(&key) == Some(failed),
        "protected store trusted head changed during append rollback"
    );
    match previous {
        Some(previous) => {
            guard.insert(key, previous.clone());
        }
        None => {
            guard.remove(&key);
        }
    }
    Ok(())
}

async fn cached_head(path: &Path) -> Option<AuthenticatedStoreHead> {
    let heads = CACHED_HEADS.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()));
    heads
        .lock()
        .await
        .get(&path.to_string_lossy().into_owned())
        .cloned()
}

async fn poison_cached_head(path: &Path, failed: &AuthenticatedStoreHead) {
    let heads = CACHED_HEADS.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()));
    let mut guard = heads.lock().await;
    let mut poisoned = failed.clone();
    poisoned.generation = poisoned.generation.saturating_add(1);
    poisoned.digest = format!("failed-append:{}", poisoned.digest);
    guard.insert(path.to_string_lossy().into_owned(), poisoned);
}

#[cfg(test)]
pub(super) async fn forget_cached_head_for_test(path: &Path) {
    if let Some(heads) = CACHED_HEADS.get() {
        heads
            .lock()
            .await
            .remove(&path.to_string_lossy().into_owned());
    }
}

async fn read_authenticated_head(
    crypto: &ProtectedFileCrypto,
    path: &Path,
    store: &ProtectedStoreContext,
) -> anyhow::Result<AuthenticatedStoreHead> {
    let head_path = integrity_head_path(path);
    let stored = fs::read_to_string(&head_path).await.with_context(|| {
        format!(
            "read protected store integrity head {}",
            head_path.display()
        )
    })?;
    let plaintext = crypto
        .decrypt_record(stored.trim(), &store.head)
        .context("decrypt protected store integrity head")?;
    let head = serde_json::from_str::<AuthenticatedStoreHead>(&plaintext)
        .context("parse protected store integrity head")?;
    anyhow::ensure!(
        head.version == AUTHENTICATED_STORE_VERSION,
        "unsupported protected store integrity version {}",
        head.version
    );
    anyhow::ensure!(
        head.store_id == store.store_id,
        "protected store integrity head identity mismatch"
    );
    anyhow::ensure!(
        head.generation > 0 && !head.digest.is_empty(),
        "protected store integrity head is incomplete"
    );
    Ok(head)
}

async fn read_authenticated_state(
    crypto: &ProtectedFileCrypto,
    path: &Path,
    store: &ProtectedStoreContext,
) -> anyhow::Result<AuthenticatedStoreState> {
    let state_path = initialized_state_path(path);
    let stored = fs::read_to_string(&state_path).await.with_context(|| {
        format!(
            "read protected store initialized state {}",
            state_path.display()
        )
    })?;
    let plaintext = crypto
        .decrypt_record(stored.trim(), &store.head)
        .context("decrypt protected store initialized state")?;
    let state = serde_json::from_str::<AuthenticatedStoreState>(&plaintext)
        .context("parse protected store initialized state")?;
    anyhow::ensure!(
        state.version == AUTHENTICATED_STORE_VERSION,
        "unsupported protected store initialized-state version {}",
        state.version
    );
    anyhow::ensure!(
        state.kind == INITIALIZED_STATE_KIND,
        "protected store initialized-state kind mismatch"
    );
    anyhow::ensure!(
        state.store_id == store.store_id,
        "protected store initialized-state identity mismatch"
    );
    anyhow::ensure!(
        state.generation > 0 && !state.digest.is_empty(),
        "protected store initialized state is incomplete"
    );
    Ok(state)
}

async fn read_committed_head(
    crypto: &ProtectedFileCrypto,
    path: &Path,
    store: &ProtectedStoreContext,
) -> anyhow::Result<AuthenticatedStoreHead> {
    let head = read_authenticated_head(crypto, path, store).await?;
    let state = read_authenticated_state(crypto, path, store).await?;
    anyhow::ensure!(
        state.store_id == head.store_id
            && state.generation == head.generation
            && state.digest == head.digest,
        "protected store persistent initialized witness does not match its integrity head"
    );
    Ok(head)
}

fn encode_authenticated_head(
    crypto: &ProtectedFileCrypto,
    head: &AuthenticatedStoreHead,
    store: &ProtectedStoreContext,
) -> anyhow::Result<String> {
    crypto.encrypt_record(&serde_json::to_string(head)?, &store.head)
}

fn authenticated_state(head: &AuthenticatedStoreHead) -> AuthenticatedStoreState {
    AuthenticatedStoreState {
        version: head.version,
        kind: INITIALIZED_STATE_KIND.to_string(),
        store_id: head.store_id.clone(),
        generation: head.generation,
        digest: head.digest.clone(),
    }
}

fn encode_authenticated_state(
    crypto: &ProtectedFileCrypto,
    state: &AuthenticatedStoreState,
    store: &ProtectedStoreContext,
) -> anyhow::Result<String> {
    crypto.encrypt_record(&serde_json::to_string(state)?, &store.head)
}

pub(super) async fn atomic_replace(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).await?;
    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "governance-store".to_string());
    let temporary = parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
    let result = async {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .await?;
        file.write_all(bytes).await?;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        fs::rename(&temporary, path).await?;
        #[cfg(test)]
        fail_injected_replace_point(path, ReplaceFaultPoint::ParentSync)?;
        sync_parent_directory(parent).await?;
        anyhow::Ok(())
    }
    .await;
    if result.is_err() {
        let _ = fs::remove_file(&temporary).await;
    }
    result
}

#[cfg(unix)]
async fn sync_parent_directory(parent: &Path) -> anyhow::Result<()> {
    fs::File::open(parent).await?.sync_all().await?;
    Ok(())
}

#[cfg(not(unix))]
async fn sync_parent_directory(_parent: &Path) -> anyhow::Result<()> {
    Ok(())
}

fn durable_create_needs_parent_sync(durable: bool, existed_before: bool) -> bool {
    durable && !existed_before
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplaceFaultPoint {
    ParentSync,
}

#[cfg(test)]
fn replace_faults() -> &'static std::sync::Mutex<HashMap<PathBuf, ReplaceFaultPoint>> {
    static FAULTS: OnceLock<std::sync::Mutex<HashMap<PathBuf, ReplaceFaultPoint>>> =
        OnceLock::new();
    FAULTS.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

#[cfg(test)]
fn inject_replace_fault_for_test(path: &Path, point: ReplaceFaultPoint) {
    replace_faults()
        .lock()
        .expect("replace fault lock")
        .insert(path.to_path_buf(), point);
}

#[cfg(test)]
fn fail_injected_replace_point(path: &Path, point: ReplaceFaultPoint) -> anyhow::Result<()> {
    let mut faults = replace_faults().lock().expect("replace fault lock");
    if faults.get(path) == Some(&point) {
        faults.remove(path);
        anyhow::bail!("injected atomic replace {point:?} failure after rename");
    }
    Ok(())
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppendFaultPoint {
    DataSync,
    ParentSync,
}

#[cfg(test)]
fn append_faults() -> &'static std::sync::Mutex<HashMap<PathBuf, AppendFaultPoint>> {
    static FAULTS: OnceLock<std::sync::Mutex<HashMap<PathBuf, AppendFaultPoint>>> = OnceLock::new();
    FAULTS.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

#[cfg(test)]
fn inject_append_fault_for_test(path: &Path, point: AppendFaultPoint) {
    append_faults()
        .lock()
        .expect("append fault lock")
        .insert(path.to_path_buf(), point);
}

#[cfg(test)]
fn fail_injected_append_point(path: &Path, point: AppendFaultPoint) -> anyhow::Result<()> {
    let mut faults = append_faults().lock().expect("append fault lock");
    if faults.get(path) == Some(&point) {
        faults.remove(path);
        anyhow::bail!("injected protected JSONL {point:?} failure after data write");
    }
    Ok(())
}

async fn sync_durable_append(
    file: &fs::File,
    path: &Path,
    parent: &Path,
    durable: bool,
    existed_before: bool,
) -> anyhow::Result<()> {
    if durable {
        #[cfg(test)]
        fail_injected_append_point(path, AppendFaultPoint::DataSync)?;
        file.sync_all().await?;
        if durable_create_needs_parent_sync(durable, existed_before) {
            #[cfg(test)]
            fail_injected_append_point(path, AppendFaultPoint::ParentSync)?;
            sync_parent_directory(parent).await?;
        }
    }
    Ok(())
}

async fn read_optional_file(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    match fs::read(path).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

async fn ensure_integrity_sidecars_absent(path: &Path, store_kind: &str) -> anyhow::Result<()> {
    let head_exists = read_optional_file(&integrity_head_path(path))
        .await?
        .is_some();
    let state_exists = read_optional_file(&initialized_state_path(path))
        .await?
        .is_some();
    anyhow::ensure!(
        !head_exists && !state_exists,
        "{store_kind} conflicts with protected store integrity state"
    );
    Ok(())
}

async fn restore_file(path: &Path, previous: Option<&[u8]>) -> anyhow::Result<()> {
    match previous {
        Some(previous) => atomic_replace(path, previous).await,
        None => match fs::remove_file(path).await {
            Ok(()) => sync_parent_directory(path.parent().unwrap_or_else(|| Path::new("."))).await,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        },
    }
}

async fn restore_integrity_sidecars(
    head_path: &Path,
    previous_head: Option<&[u8]>,
    state_path: &Path,
    previous_state: Option<&[u8]>,
) -> anyhow::Result<()> {
    let head_result = restore_file(head_path, previous_head).await;
    let state_result = restore_file(state_path, previous_state).await;
    match (head_result, state_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(head_error), Ok(())) => Err(head_error.context("restore integrity head")),
        (Ok(()), Err(state_error)) => Err(state_error.context("restore initialized-state witness")),
        (Err(head_error), Err(state_error)) => Err(head_error.context(format!(
            "restore integrity head; initialized-state witness restore also failed: {state_error:#}"
        ))),
    }
}

async fn restore_jsonl_append_data(
    path: &Path,
    existed_before: bool,
    previous_len: u64,
) -> anyhow::Result<()> {
    if !existed_before {
        return restore_file(path, None).await;
    }

    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .await
        .with_context(|| format!("open protected JSONL data for rollback {}", path.display()))?;
    file.set_len(previous_len)
        .await
        .with_context(|| format!("truncate protected JSONL data {}", path.display()))?;
    file.sync_all()
        .await
        .with_context(|| format!("sync rolled-back protected JSONL data {}", path.display()))?;
    Ok(())
}

async fn append_jsonl_without_integrity(
    path: &Path,
    row: &[u8],
    durable: bool,
    existed_before: bool,
    previous_len: u64,
) -> anyhow::Result<()> {
    let result = async {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        file.write_all(row).await?;
        file.flush().await?;
        sync_durable_append(
            &file,
            path,
            path.parent().unwrap_or_else(|| Path::new(".")),
            durable,
            existed_before,
        )
        .await
    }
    .await;

    if let Err(error) = result {
        return match restore_jsonl_append_data(path, existed_before, previous_len).await {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(error.context(format!(
                "failed to roll back uncommitted protected JSONL bytes: {rollback_error:#}"
            ))),
        };
    }
    Ok(())
}

async fn restore_failed_jsonl_append(
    path: &Path,
    existed_before: bool,
    previous_len: u64,
    head_path: &Path,
    previous_head: Option<&[u8]>,
    state_path: &Path,
    previous_state: Option<&[u8]>,
    failed_head: &AuthenticatedStoreHead,
    previous_committed_head: Option<&AuthenticatedStoreHead>,
) -> anyhow::Result<()> {
    // Restore data before sidecars. If any rollback step cannot be confirmed,
    // poison the trusted head so this process rejects every on-disk generation.
    let data_result = restore_jsonl_append_data(path, existed_before, previous_len)
        .await
        .context("restore protected JSONL data after failed append");
    let sidecar_result =
        restore_integrity_sidecars(head_path, previous_head, state_path, previous_state)
            .await
            .context("restore protected JSONL integrity sidecars after failed append");

    match (data_result, sidecar_result) {
        (Ok(()), Ok(())) => {
            if let Err(error) =
                rollback_cached_head(path, failed_head, previous_committed_head).await
            {
                poison_cached_head(path, failed_head).await;
                return Err(
                    error.context("restore protected JSONL trusted head after failed append")
                );
            }
            Ok(())
        }
        (Err(data_error), Ok(())) => {
            poison_cached_head(path, failed_head).await;
            Err(data_error)
        }
        (Ok(()), Err(sidecar_error)) => {
            poison_cached_head(path, failed_head).await;
            Err(sidecar_error)
        }
        (Err(data_error), Err(sidecar_error)) => {
            poison_cached_head(path, failed_head).await;
            Err(data_error.context(format!(
                "integrity sidecar rollback also failed: {sidecar_error:#}"
            )))
        }
    }
}

async fn restore_failed_collection_write(
    path: &Path,
    previous_data: Option<&[u8]>,
    head_path: &Path,
    previous_head: Option<&[u8]>,
    state_path: &Path,
    previous_state: Option<&[u8]>,
    failed_head: &AuthenticatedStoreHead,
    previous_cached_head: Option<&AuthenticatedStoreHead>,
) -> anyhow::Result<()> {
    let data_result = restore_file(path, previous_data)
        .await
        .context("restore protected collection data after failed write");
    let sidecar_result =
        restore_integrity_sidecars(head_path, previous_head, state_path, previous_state)
            .await
            .context("restore protected collection integrity sidecars after failed write");
    let files_result = match (data_result, sidecar_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(data_error), Ok(())) => Err(data_error),
        (Ok(()), Err(sidecar_error)) => Err(sidecar_error),
        (Err(data_error), Err(sidecar_error)) => Err(data_error.context(format!(
            "integrity sidecar rollback also failed: {sidecar_error:#}"
        ))),
    };

    let rollback = match files_result {
        Ok(()) => rollback_cached_head(path, failed_head, previous_cached_head)
            .await
            .context("restore protected collection trusted head after failed write"),
        Err(error) => Err(error),
    };
    if rollback.is_err() {
        poison_cached_head(path, failed_head).await;
    }
    rollback
}

fn encrypt_legacy_local_line(
    crypto: &ProtectedFileCrypto,
    plaintext: &str,
    context: &ProtectedRecordContext,
) -> anyhow::Result<String> {
    anyhow::ensure!(
        !crypto.provider.is_plaintext() && !crypto.provider.is_hosted(),
        "legacy local line encryption requires a local-key provider"
    );
    Ok(crypto
        .provider
        .encrypt_field_scoped(
            plaintext,
            &context.key_scope,
            &context.policy_decision_id,
            &context.audit_id,
        )?
        .0)
}

async fn decrypt_jsonl_state(
    crypto: &ProtectedFileCrypto,
    path: &Path,
    store: &ProtectedStoreContext,
) -> anyhow::Result<DecryptedJsonl> {
    let content = fs::read_to_string(path).await?;
    let non_empty = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if non_empty.is_empty() {
        ensure_integrity_sidecars_absent(path, "empty protected JSONL store").await?;
        return Ok(DecryptedJsonl {
            lines: Vec::new(),
            generation: 0,
            digest: String::new(),
            authenticated: false,
        });
    }

    if !non_empty[0].starts_with(AUTHENTICATED_JSONL_PREFIX) {
        anyhow::ensure!(
            !crypto.provider.is_hosted(),
            "hosted protected JSONL legacy rows lack authenticated expected authorities"
        );
        ensure_integrity_sidecars_absent(path, "legacy protected JSONL rows").await?;
        let lines = non_empty
            .into_iter()
            .map(|line| crypto.decrypt_legacy_record(line))
            .collect::<anyhow::Result<Vec<_>>>()?;
        return Ok(DecryptedJsonl {
            lines,
            generation: 0,
            digest: String::new(),
            authenticated: false,
        });
    }

    let mut lines = Vec::with_capacity(non_empty.len());
    let mut previous_digest: Option<String> = None;
    for (index, stored) in non_empty.into_iter().enumerate() {
        let encoded = stored
            .strip_prefix(AUTHENTICATED_JSONL_PREFIX)
            .context("mixed authenticated and legacy protected JSONL rows")?;
        let frame_plaintext = crypto.decrypt_record(encoded, &store.manifest)?;
        let frame = serde_json::from_str::<AuthenticatedJsonlFrame>(&frame_plaintext)
            .context("parse authenticated protected JSONL frame")?;
        let expected_sequence = index as u64 + 1;
        anyhow::ensure!(
            frame.version == AUTHENTICATED_STORE_VERSION
                && frame.store_id == store.store_id
                && frame.sequence == expected_sequence,
            "protected JSONL frame identity or sequence mismatch"
        );
        anyhow::ensure!(
            frame.previous_digest == previous_digest,
            "protected JSONL frame chain break at sequence {}",
            frame.sequence
        );
        anyhow::ensure!(
            frame.digest == jsonl_frame_digest(&frame)?,
            "protected JSONL frame digest mismatch at sequence {}",
            frame.sequence
        );
        let plaintext = crypto.decrypt_record(&frame.stored_record, &frame.context)?;
        previous_digest = Some(frame.digest);
        lines.push(plaintext);
    }
    let generation = lines.len() as u64;
    let digest = previous_digest.unwrap_or_default();
    let head = read_committed_head(crypto, path, store).await?;
    anyhow::ensure!(
        head.generation == generation && head.digest == digest,
        "protected JSONL rollback, deletion, or replay detected"
    );
    validate_cached_head(path, &head).await?;
    Ok(DecryptedJsonl {
        lines,
        generation,
        digest,
        authenticated: true,
    })
}

pub(crate) async fn read_text_file(
    path: &Path,
    store: &ProtectedStoreContext,
) -> anyhow::Result<String> {
    let stored = fs::read_to_string(path).await?;
    let crypto = crypto();
    if stored.starts_with(AUTHENTICATED_COLLECTION_PREFIX) {
        let collection = crypto.decrypt_json_collection(&stored, store)?;
        let head = read_committed_head(&crypto, path, store).await?;
        anyhow::ensure!(
            head.generation == collection.generation && head.digest == collection.digest,
            "protected JSON collection rollback, deletion, or replay detected"
        );
        validate_cached_head(path, &head).await?;
        return Ok(collection.plaintext_json);
    }
    ensure_integrity_sidecars_absent(path, "legacy protected JSON document").await?;
    crypto.decrypt_legacy_json_document(&stored)
}

pub(crate) async fn read_jsonl_records_file(
    path: &Path,
    store: &ProtectedStoreContext,
) -> anyhow::Result<Vec<String>> {
    Ok(decrypt_jsonl_state(&crypto(), path, store).await?.lines)
}

pub(crate) async fn append_jsonl_record_file(
    path: &Path,
    plaintext: &str,
    context: &ProtectedRecordContext,
    store: &ProtectedStoreContext,
    durable: bool,
) -> anyhow::Result<()> {
    let lock = path_lock_for(path).await;
    let _guard = lock.lock().await;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let crypto = crypto();
    let _process_guard = ProcessWriteLock::acquire(path).await?;
    if crypto.provider.is_plaintext() {
        ensure_integrity_sidecars_absent(path, "plaintext protected JSONL append").await?;
        let (existed_before, previous_len) = match fs::metadata(path).await {
            Ok(metadata) => (true, metadata.len()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (false, 0),
            Err(error) => return Err(error.into()),
        };
        let mut row = plaintext.as_bytes().to_vec();
        row.push(b'\n');
        return append_jsonl_without_integrity(path, &row, durable, existed_before, previous_len)
            .await;
    }

    let head_path = integrity_head_path(path);
    let state_path = initialized_state_path(path);
    let previous_head = read_optional_file(&head_path).await?;
    let previous_state = read_optional_file(&state_path).await?;
    let (prior, existed_before, previous_len) = match fs::metadata(path).await {
        Ok(metadata) => (
            decrypt_jsonl_state(&crypto, path, store).await?,
            true,
            metadata.len(),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            anyhow::ensure!(
                previous_head.is_none() && previous_state.is_none(),
                "protected JSONL data is missing from an initialized store"
            );
            (
                DecryptedJsonl {
                    lines: Vec::new(),
                    generation: 0,
                    digest: String::new(),
                    authenticated: true,
                },
                false,
                0,
            )
        }
        Err(error) => return Err(error.into()),
    };
    if !prior.authenticated && prior.generation == 0 && !prior.lines.is_empty() {
        let stored = encrypt_legacy_local_line(&crypto, plaintext, context)?;
        return append_jsonl_without_integrity(
            path,
            format!("{stored}\n").as_bytes(),
            durable,
            existed_before,
            previous_len,
        )
        .await;
    }

    let sequence = prior.generation.saturating_add(1);
    let previous_committed_head = (prior.generation > 0).then(|| AuthenticatedStoreHead {
        version: AUTHENTICATED_STORE_VERSION,
        store_id: store.store_id.clone(),
        generation: prior.generation,
        digest: prior.digest.clone(),
    });
    let mut frame = AuthenticatedJsonlFrame {
        version: AUTHENTICATED_STORE_VERSION,
        store_id: store.store_id.clone(),
        sequence,
        previous_digest: (prior.generation > 0).then(|| prior.digest.clone()),
        context: context.clone(),
        stored_record: crypto.encrypt_record(plaintext, context)?,
        digest: String::new(),
    };
    frame.digest = jsonl_frame_digest(&frame)?;
    let outer = crypto.encrypt_record(&serde_json::to_string(&frame)?, &store.manifest)?;
    let stored_line = format!("{AUTHENTICATED_JSONL_PREFIX}{outer}\n");
    let head = AuthenticatedStoreHead {
        version: AUTHENTICATED_STORE_VERSION,
        store_id: store.store_id.clone(),
        generation: sequence,
        digest: frame.digest,
    };
    let state = authenticated_state(&head);
    let encoded_head = encode_authenticated_head(&crypto, &head, store)?;
    let encoded_state = encode_authenticated_state(&crypto, &state, store)?;
    validate_cached_head(path, &head).await?;

    // The persistent witness advances first. Any crash before all three files
    // agree makes the store unavailable instead of accepting a partial append.
    if let Err(error) = atomic_replace(&state_path, encoded_state.as_bytes()).await {
        let rollback = async {
            restore_file(&state_path, previous_state.as_deref()).await?;
            rollback_cached_head(path, &head, previous_committed_head.as_ref()).await
        }
        .await;
        if rollback.is_err() {
            poison_cached_head(path, &head).await;
        }
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(error.context(format!(
                "failed to restore protected JSONL initialized state: {rollback_error:#}"
            ))),
        };
    }
    if let Err(error) = atomic_replace(&head_path, encoded_head.as_bytes()).await {
        let rollback = restore_integrity_sidecars(
            &head_path,
            previous_head.as_deref(),
            &state_path,
            previous_state.as_deref(),
        )
        .await;
        let rollback = match rollback {
            Ok(()) => rollback_cached_head(path, &head, previous_committed_head.as_ref()).await,
            Err(error) => Err(error),
        };
        if rollback.is_err() {
            poison_cached_head(path, &head).await;
        }
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(error.context(format!(
                "failed to restore protected JSONL integrity state: {rollback_error:#}"
            ))),
        };
    }

    let write_result = async {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        file.write_all(stored_line.as_bytes()).await?;
        file.flush().await?;
        sync_durable_append(
            &file,
            path,
            path.parent().unwrap_or_else(|| Path::new(".")),
            durable,
            existed_before,
        )
        .await?;
        anyhow::Ok(())
    }
    .await;
    if let Err(error) = write_result {
        let rollback = restore_failed_jsonl_append(
            path,
            existed_before,
            previous_len,
            &head_path,
            previous_head.as_deref(),
            &state_path,
            previous_state.as_deref(),
            &head,
            previous_committed_head.as_ref(),
        )
        .await;
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(error.context(format!(
                "failed to roll back protected JSONL append: {rollback_error:#}"
            ))),
        };
    }
    Ok(())
}

pub(crate) async fn write_json_records_file(
    path: &Path,
    records: &[ProtectedJsonRecord],
    store: &ProtectedStoreContext,
) -> anyhow::Result<()> {
    let lock = path_lock_for(path).await;
    let _guard = lock.lock().await;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let crypto = crypto();
    let _process_guard = ProcessWriteLock::acquire(path).await?;
    if crypto.provider.is_plaintext() {
        ensure_integrity_sidecars_absent(path, "plaintext protected JSON write").await?;
        return atomic_replace(
            path,
            ProtectedFileCrypto::plaintext_json(records)?.as_bytes(),
        )
        .await;
    }

    let old_data = read_optional_file(path).await?;
    let head_path = integrity_head_path(path);
    let state_path = initialized_state_path(path);
    let old_head = read_optional_file(&head_path).await?;
    let old_state = read_optional_file(&state_path).await?;
    let (generation, previous_digest) = match old_data.as_deref() {
        Some(bytes) => {
            let stored = std::str::from_utf8(bytes).context("protected JSON store is not UTF-8")?;
            if stored.starts_with(AUTHENTICATED_COLLECTION_PREFIX) {
                let current = crypto.decrypt_json_collection(stored, store)?;
                let head = read_committed_head(&crypto, path, store).await?;
                anyhow::ensure!(
                    current.generation == head.generation && current.digest == head.digest,
                    "protected JSON collection integrity head mismatch before write"
                );
                validate_cached_head(path, &head).await?;
                (current.generation.saturating_add(1), Some(current.digest))
            } else {
                anyhow::ensure!(
                    old_head.is_none() && old_state.is_none(),
                    "legacy protected JSON document conflicts with integrity state"
                );
                crypto.decrypt_legacy_json_document(stored)?;
                (1, None)
            }
        }
        None => {
            anyhow::ensure!(
                old_head.is_none() && old_state.is_none(),
                "protected JSON collection data is missing from an initialized store"
            );
            (1, None)
        }
    };
    let (stored, head) =
        crypto.encrypt_json_collection(records, store, generation, previous_digest)?;
    let previous_cached_head = cached_head(path).await;
    validate_cached_head(path, &head).await?;
    let state = authenticated_state(&head);
    let encoded_head = encode_authenticated_head(&crypto, &head, store)?;
    let encoded_state = encode_authenticated_state(&crypto, &state, store)?;

    // Advance the persistent witness before the sealed head and data. A crash
    // before all three renames agree leaves the store unavailable.
    let write_result = async {
        atomic_replace(&state_path, encoded_state.as_bytes())
            .await
            .context("write protected collection initialized state")?;
        atomic_replace(&head_path, encoded_head.as_bytes())
            .await
            .context("write protected collection integrity head")?;
        atomic_replace(path, stored.as_bytes())
            .await
            .context("write protected collection data")?;
        anyhow::Ok(())
    }
    .await;
    if let Err(error) = write_result {
        let rollback = restore_failed_collection_write(
            path,
            old_data.as_deref(),
            &head_path,
            old_head.as_deref(),
            &state_path,
            old_state.as_deref(),
            &head,
            previous_cached_head.as_ref(),
        )
        .await;
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(error.context(format!(
                "failed to roll back protected collection write: {rollback_error:#}"
            ))),
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use tandem_enterprise_contract::DataClass;
    use tandem_memory::envelope::MemoryKeyScope;
    use tandem_memory::types::MemoryTenantScope;
    use tandem_memory::MemoryCryptoProvider;
    use tokio::sync::oneshot;

    use super::{
        append_jsonl_record_file, crypto, durable_create_needs_parent_sync,
        encrypt_legacy_local_line, forget_cached_head_for_test, fs, initialized_state_path,
        inject_append_fault_for_test, inject_replace_fault_for_test, integrity_head_path,
        read_jsonl_records_file, read_optional_file, read_text_file, write_json_records_file,
        AppendFaultPoint, ProcessWriteLock, ReplaceFaultPoint,
    };
    use crate::encrypted_file_store::{
        with_test_crypto_provider, ProtectedJsonRecord, ProtectedRecordContext,
        ProtectedStoreContext,
    };

    fn record_context(record_id: &str) -> ProtectedRecordContext {
        let tenant = MemoryTenantScope {
            org_id: "fault-test-org".to_string(),
            workspace_id: "fault-test-workspace".to_string(),
            deployment_id: Some("fault-test-deployment".to_string()),
        };
        let scope = MemoryKeyScope::new(
            &tenant,
            DataClass::Restricted,
            Some("protected-jsonl-fault-test".to_string()),
        )
        .with_org_unit(Some("integrity".to_string()));
        ProtectedRecordContext::new(scope, "fault-test-policy", record_id)
    }

    fn store_context(store_id: &str) -> ProtectedStoreContext {
        ProtectedStoreContext::new(
            store_id,
            record_context(&format!("{store_id}-manifest")),
            record_context(&format!("{store_id}-head")),
        )
    }

    fn json_record(value: u64) -> Vec<ProtectedJsonRecord> {
        vec![
            ProtectedJsonRecord::new("record", &value, record_context("record"))
                .expect("JSON record"),
        ]
    }

    async fn assert_collection_value(path: &Path, store: &ProtectedStoreContext, expected: u64) {
        let plaintext = read_text_file(path, store).await.expect("read collection");
        let value = serde_json::from_str::<serde_json::Value>(&plaintext).expect("collection JSON");
        assert_eq!(value["record"], expected);
    }

    #[test]
    fn parent_sync_is_required_only_for_first_durable_append() {
        assert!(durable_create_needs_parent_sync(true, false));
        assert!(!durable_create_needs_parent_sync(true, true));
        assert!(!durable_create_needs_parent_sync(false, false));
    }

    #[tokio::test]
    async fn collection_post_rename_sync_failures_restore_all_files_and_retry() {
        for target_name in ["state", "head", "data"] {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join(format!("{target_name}.json"));
            let head_path = integrity_head_path(&path);
            let state_path = initialized_state_path(&path);
            let store = store_context(&format!("collection-{target_name}"));

            with_test_crypto_provider(MemoryCryptoProvider::local_key([0x27; 32]), None, async {
                write_json_records_file(&path, &json_record(1), &store)
                    .await
                    .expect("initial collection write");
                let committed = [
                    fs::read(&path).await.expect("data"),
                    fs::read(&head_path).await.expect("head"),
                    fs::read(&state_path).await.expect("state"),
                ];
                let fault_path = match target_name {
                    "state" => &state_path,
                    "head" => &head_path,
                    _ => &path,
                };
                inject_replace_fault_for_test(fault_path, ReplaceFaultPoint::ParentSync);
                let error = write_json_records_file(&path, &json_record(2), &store)
                    .await
                    .expect_err("post-rename sync must fail");
                assert!(format!("{error:#}").contains("after rename"));
                assert_eq!(fs::read(&path).await.expect("restored data"), committed[0]);
                assert_eq!(
                    fs::read(&head_path).await.expect("restored head"),
                    committed[1]
                );
                assert_eq!(
                    fs::read(&state_path).await.expect("restored state"),
                    committed[2]
                );
                assert_collection_value(&path, &store, 1).await;

                write_json_records_file(&path, &json_record(3), &store)
                    .await
                    .expect("collection retry");
                assert_collection_value(&path, &store, 3).await;
            })
            .await;
        }
    }

    #[tokio::test]
    async fn collection_rollback_failure_poisons_cache_until_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("poison.json");
        let state_path = initialized_state_path(&path);
        let store = store_context("collection-poison");

        with_test_crypto_provider(MemoryCryptoProvider::local_key([0x28; 32]), None, async {
            write_json_records_file(&path, &json_record(1), &store)
                .await
                .expect("initial collection write");
            inject_replace_fault_for_test(&state_path, ReplaceFaultPoint::ParentSync);
            inject_replace_fault_for_test(&path, ReplaceFaultPoint::ParentSync);
            let error = write_json_records_file(&path, &json_record(2), &store)
                .await
                .expect_err("write and rollback sync must fail");
            assert!(format!("{error:#}").contains("failed to roll back"));
            assert!(
                read_text_file(&path, &store).await.is_err(),
                "poisoned cache must fail closed"
            );

            forget_cached_head_for_test(&path).await;
            write_json_records_file(&path, &json_record(3), &store)
                .await
                .expect("retry after restart-style cache reset");
            assert_collection_value(&path, &store, 3).await;
        })
        .await;
    }

    #[tokio::test]
    async fn plaintext_jsonl_sync_failures_undo_bytes_and_retry() {
        for (name, point, existing) in [
            ("data", AppendFaultPoint::DataSync, true),
            ("parent", AppendFaultPoint::ParentSync, false),
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join(format!("plaintext-{name}.jsonl"));
            let store = store_context(&format!("plaintext-{name}"));
            with_test_crypto_provider(MemoryCryptoProvider::plaintext(), None, async {
                if existing {
                    append_jsonl_record_file(
                        &path,
                        "first",
                        &record_context("first"),
                        &store,
                        true,
                    )
                    .await
                    .expect("initial plaintext append");
                }
                let committed = read_optional_file(&path).await.expect("committed bytes");
                inject_append_fault_for_test(&path, point);
                append_jsonl_record_file(&path, "failed", &record_context("failed"), &store, true)
                    .await
                    .expect_err("injected plaintext sync failure");
                assert_eq!(
                    read_optional_file(&path).await.expect("restored bytes"),
                    committed
                );

                append_jsonl_record_file(&path, "retry", &record_context("retry"), &store, true)
                    .await
                    .expect("plaintext retry");
                let expected = if existing {
                    vec!["first".to_string(), "retry".to_string()]
                } else {
                    vec!["retry".to_string()]
                };
                assert_eq!(
                    read_jsonl_records_file(&path, &store).await.unwrap(),
                    expected
                );
            })
            .await;
        }
    }

    #[tokio::test]
    async fn legacy_encrypted_jsonl_sync_failure_truncates_and_retries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("legacy.jsonl");
        let store = store_context("legacy-sync");
        with_test_crypto_provider(MemoryCryptoProvider::local_key([0x29; 32]), None, async {
            let first = encrypt_legacy_local_line(&crypto(), "first", &record_context("first"))
                .expect("legacy encryption");
            fs::write(&path, format!("{first}\n"))
                .await
                .expect("legacy store");
            let committed = fs::read(&path).await.expect("committed bytes");
            inject_append_fault_for_test(&path, AppendFaultPoint::DataSync);
            append_jsonl_record_file(&path, "failed", &record_context("failed"), &store, true)
                .await
                .expect_err("injected legacy sync failure");
            assert_eq!(fs::read(&path).await.expect("restored bytes"), committed);

            append_jsonl_record_file(&path, "retry", &record_context("retry"), &store, true)
                .await
                .expect("legacy retry");
            assert_eq!(
                read_jsonl_records_file(&path, &store).await.unwrap(),
                vec!["first".to_string(), "retry".to_string()]
            );
        })
        .await;
    }

    #[tokio::test]
    async fn failed_jsonl_data_sync_restores_data_sidecars_and_next_append() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("data-sync.jsonl");
        let head_path = integrity_head_path(&path);
        let state_path = initialized_state_path(&path);
        let store = store_context("data-sync-store");

        with_test_crypto_provider(MemoryCryptoProvider::local_key([0x35; 32]), None, async {
            append_jsonl_record_file(
                &path,
                r#"{"sequence":1}"#,
                &record_context("record-1"),
                &store,
                true,
            )
            .await
            .expect("initial append");
            let committed_data = fs::read(&path).await.expect("committed data");
            let committed_head = fs::read(&head_path).await.expect("committed head");
            let committed_state = fs::read(&state_path).await.expect("committed state");

            inject_append_fault_for_test(&path, AppendFaultPoint::DataSync);
            let error = append_jsonl_record_file(
                &path,
                r#"{"sequence":2}"#,
                &record_context("record-2"),
                &store,
                true,
            )
            .await
            .expect_err("injected data sync must fail");
            assert!(
                format!("{error:#}").contains("DataSync"),
                "unexpected error: {error:?}"
            );
            assert_eq!(
                fs::read(&path).await.expect("rolled-back data"),
                committed_data
            );
            assert_eq!(
                fs::read(&head_path).await.expect("rolled-back head"),
                committed_head
            );
            assert_eq!(
                fs::read(&state_path).await.expect("rolled-back state"),
                committed_state
            );
            assert_eq!(
                read_jsonl_records_file(&path, &store)
                    .await
                    .expect("read after failed append"),
                vec![r#"{"sequence":1}"#.to_string()]
            );

            append_jsonl_record_file(
                &path,
                r#"{"sequence":3}"#,
                &record_context("record-3"),
                &store,
                true,
            )
            .await
            .expect("append after rollback");
            assert_eq!(
                read_jsonl_records_file(&path, &store)
                    .await
                    .expect("read after recovery"),
                vec![
                    r#"{"sequence":1}"#.to_string(),
                    r#"{"sequence":3}"#.to_string()
                ]
            );
        })
        .await;
    }

    #[tokio::test]
    async fn failed_jsonl_parent_sync_removes_uncommitted_new_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("parent-sync.jsonl");
        let head_path = integrity_head_path(&path);
        let state_path = initialized_state_path(&path);
        let store = store_context("parent-sync-store");

        with_test_crypto_provider(MemoryCryptoProvider::local_key([0x53; 32]), None, async {
            inject_append_fault_for_test(&path, AppendFaultPoint::ParentSync);
            let error = append_jsonl_record_file(
                &path,
                r#"{"sequence":1}"#,
                &record_context("record-1"),
                &store,
                true,
            )
            .await
            .expect_err("injected parent sync must fail");
            assert!(
                format!("{error:#}").contains("ParentSync"),
                "unexpected error: {error:?}"
            );
            assert!(!path.exists(), "failed append data must be removed");
            assert!(!head_path.exists(), "failed append head must be removed");
            assert!(!state_path.exists(), "failed append state must be removed");

            append_jsonl_record_file(
                &path,
                r#"{"sequence":2}"#,
                &record_context("record-2"),
                &store,
                true,
            )
            .await
            .expect("append after parent-sync rollback");
            assert_eq!(
                read_jsonl_records_file(&path, &store)
                    .await
                    .expect("read recreated store"),
                vec![r#"{"sequence":2}"#.to_string()]
            );
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn process_write_lock_serializes_independent_owners() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("independent-writers.json");
        let first = ProcessWriteLock::acquire(&path)
            .await
            .expect("first process-style owner");
        let second_path = path.clone();
        let (acquired_tx, mut acquired_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let second = tokio::spawn(async move {
            let guard = ProcessWriteLock::acquire(&second_path)
                .await
                .expect("second process-style owner");
            acquired_tx.send(()).expect("report acquisition");
            let _ = release_rx.await;
            drop(guard);
        });

        assert!(
            tokio::time::timeout(Duration::from_millis(150), &mut acquired_rx)
                .await
                .is_err(),
            "independent owner acquired the process lock before release"
        );
        drop(first);
        tokio::time::timeout(Duration::from_secs(2), &mut acquired_rx)
            .await
            .expect("second owner should acquire after release")
            .expect("acquisition notification");
        release_tx.send(()).expect("release second owner");
        second.await.expect("second owner task");
    }
}
