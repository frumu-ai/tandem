// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

type WorkflowLearningCandidateMap = std::collections::HashMap<String, WorkflowLearningCandidate>;

const WORKFLOW_LEARNING_STORE_ID: &str = "tandem-workflow-learning-candidates";
const WORKFLOW_LEARNING_STORE_VERSION: u8 = 1;

/// Storage-service authority is code-owned, not reconstructed from a candidate,
/// its source binding, or the persisted envelope. User/source authorization is
/// still performed by the existing candidate consumers after opening the map.
/// This binds the store domain, not a deployment-specific restore authority.
pub(crate) fn workflow_learning_candidate_storage_context(
) -> crate::encrypted_file_store::ProtectedRecordContext {
    let tenant = tandem_memory::types::MemoryTenantScope {
        org_id: "tandem-system".to_string(),
        workspace_id: "workflow-learning-candidate-store".to_string(),
        deployment_id: None,
    };
    let scope = tandem_memory::envelope::MemoryKeyScope::new(
        &tenant,
        tandem_enterprise_contract::DataClass::Restricted,
        Some(WORKFLOW_LEARNING_STORE_ID.to_string()),
    );
    crate::encrypted_file_store::ProtectedRecordContext::new(
        scope,
        "tandem-workflow-learning-candidate-store:snapshot:v1",
        "tandem-workflow-learning-candidate-store:snapshot",
    )
}

#[derive(Serialize)]
struct WorkflowLearningSnapshotRef<'a> {
    schema_version: u8,
    store_id: &'static str,
    candidates: &'a WorkflowLearningCandidateMap,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkflowLearningSnapshot {
    schema_version: u8,
    store_id: String,
    #[serde(deserialize_with = "deserialize_workflow_learning_candidate_map")]
    candidates: WorkflowLearningCandidateMap,
}

fn deserialize_workflow_learning_candidate_map<'de, D>(
    deserializer: D,
) -> Result<WorkflowLearningCandidateMap, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct CandidateMapVisitor;

    impl<'de> serde::de::Visitor<'de> for CandidateMapVisitor {
        type Value = WorkflowLearningCandidateMap;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a workflow-learning candidate map")
        }

        fn visit_map<A>(self, mut entries: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            let mut rows = WorkflowLearningCandidateMap::new();
            while let Some((key, value)) = entries.next_entry::<String, WorkflowLearningCandidate>()? {
                if rows.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate candidate key"));
                }
            }
            Ok(rows)
        }
    }

    deserializer.deserialize_map(CandidateMapVisitor)
}

fn validate_workflow_learning_candidate_map(rows: &WorkflowLearningCandidateMap) -> anyhow::Result<()> {
    anyhow::ensure!(
        rows.iter().all(|(key, row)| !key.trim().is_empty() && key == &row.candidate_id),
        "workflow-learning candidate store has invalid row identity"
    );
    Ok(())
}

struct LoadedWorkflowLearningCandidates {
    rows: WorkflowLearningCandidateMap,
    legacy: bool,
}

struct WorkflowLearningCandidateStore {
    crypto: crate::encrypted_file_store::CapturedRequiredFileCrypto,
    context: crate::encrypted_file_store::ProtectedRecordContext,
}

impl WorkflowLearningCandidateStore {
    fn capture(state: &AppState) -> anyhow::Result<Self> {
        let context = workflow_learning_candidate_storage_context();
        let crypto = crate::encrypted_file_store::CapturedRequiredFileCrypto::capture(
            &context,
            state.workflow_learning_hosted_storage_required()?,
        )?;
        Ok(Self { crypto, context })
    }

    fn from_configuration(
        state: &AppState,
        configuration: crate::encrypted_file_store::CapturedFileCryptoConfiguration,
    ) -> anyhow::Result<Self> {
        let context = workflow_learning_candidate_storage_context();
        let crypto = configuration.into_required(
            &context, state.workflow_learning_hosted_storage_required()?,
        )?;
        Ok(Self { crypto, context })
    }

    /// Check the captured handle itself after the actual writer wait and again
    /// before publication. A queued local handle cannot use a newly resolved
    /// ambient hosted provider to pass readiness while still sealing locally.
    fn require_current_crypto(&self, state: &AppState) -> anyhow::Result<()> {
        let hosted_required = state.workflow_learning_hosted_storage_required()?;
        self.crypto.validate_required_mode(hosted_required)?;
        if hosted_required || self.crypto.is_hosted() {
            self.crypto.validate_hosted_ready(&self.context)?;
        }
        // A KMS roundtrip can outlast a policy snapshot. Mode/freshness must
        // still be current when that synchronous roundtrip returns.
        self.crypto.validate_required_mode(state.workflow_learning_hosted_storage_required()?)?;
        Ok(())
    }

    fn seal(&self, rows: &WorkflowLearningCandidateMap) -> anyhow::Result<String> {
        validate_workflow_learning_candidate_map(rows)?;
        let payload = serde_json::to_string(&WorkflowLearningSnapshotRef {
            schema_version: WORKFLOW_LEARNING_STORE_VERSION,
            store_id: WORKFLOW_LEARNING_STORE_ID,
            candidates: rows,
        })?;
        // Reject a native value that serializes to an unreadable typed row
        // (for example a non-finite numeric field) before publishing any bytes.
        serde_json::from_str::<WorkflowLearningSnapshot>(&payload)
            .map_err(|_| anyhow::anyhow!("workflow-learning candidate snapshot cannot roundtrip"))?;
        self.crypto.encrypt(&payload, &self.context)
    }

    fn open(&self, stored: &str, allow_local_legacy: bool) -> anyhow::Result<LoadedWorkflowLearningCandidates> {
        let scoped = stored.trim_start().starts_with(crate::encrypted_file_store::SCOPED_RECORD_PREFIX);
        let plaintext = self.crypto.decrypt(stored, &self.context)
            .map_err(|_| anyhow::anyhow!("cannot open protected workflow-learning candidate store"))?;
        let rows = if scoped {
            let snapshot = serde_json::from_str::<WorkflowLearningSnapshot>(&plaintext)
                .map_err(|_| anyhow::anyhow!("invalid workflow-learning candidate snapshot"))?;
            anyhow::ensure!(
                snapshot.schema_version == WORKFLOW_LEARNING_STORE_VERSION
                    && snapshot.store_id == WORKFLOW_LEARNING_STORE_ID,
                "unsupported workflow-learning candidate snapshot"
            );
            snapshot.candidates
        } else {
            anyhow::ensure!(
                allow_local_legacy && !self.crypto.is_hosted(),
                "hosted workflow-learning candidate storage refuses legacy payloads"
            );
            let mut decoder = serde_json::Deserializer::from_str(&plaintext);
            let rows = deserialize_workflow_learning_candidate_map(&mut decoder)
                .map_err(|_| anyhow::anyhow!("invalid legacy workflow-learning candidate map"))?;
            decoder.end().map_err(|_| anyhow::anyhow!("invalid legacy workflow-learning candidate map"))?;
            anyhow::ensure!(
                rows.values().all(|row| row.source_binding.as_ref().map_or(true, |binding| match binding {
                    WorkflowLearningCandidateSourceBinding::Workflow { tenant_context, .. }
                    | WorkflowLearningCandidateSourceBinding::Session { tenant_context, .. } => tenant_context.is_local_implicit(),
                })),
                "legacy workflow-learning candidate map is not standalone"
            );
            rows
        };
        validate_workflow_learning_candidate_map(&rows)?;
        Ok(LoadedWorkflowLearningCandidates { rows, legacy: !scoped })
    }

    async fn read_existing(&self, state: &AppState) -> anyhow::Result<Option<LoadedWorkflowLearningCandidates>> {
        let Some(stored) = read_workflow_learning_candidate_store(state).await? else {
            return Ok(None);
        };
        let allow_local_legacy = !state.workflow_learning_hosted_storage_required()? && !self.crypto.is_hosted();
        self.open(&stored, allow_local_legacy).map(Some)
    }

    async fn require_existing_state(
        &self,
        state: &AppState,
        rows: &WorkflowLearningCandidateMap,
    ) -> anyhow::Result<()> {
        let existing = self.read_existing(state).await?;
        anyhow::ensure!(
            existing.is_some() || rows.is_empty(),
            "workflow-learning candidate store is missing after initialization"
        );
        Ok(())
    }
}

async fn read_workflow_learning_candidate_store(state: &AppState) -> anyhow::Result<Option<String>> {
    match fs::read_to_string(&state.workflow_learning_candidates_path).await {
        Ok(stored) => Ok(Some(stored)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("read workflow-learning candidate store"),
    }
}

struct PreparedWorkflowLearningFile(std::path::PathBuf);

impl PreparedWorkflowLearningFile {
    fn publish(&self, destination: &Path) -> anyhow::Result<()> {
        // The temporary file is synced before this rename. Rename is the
        // publication boundary, followed immediately by the cache swap. There
        // is no fallible post-rename directory sync here; crash/parent-directory
        // durability is a separate recovery requirement.
        std::fs::rename(&self.0, destination).context("publish workflow-learning candidate store")
    }
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkflowLearningPreparationFaultForTest {
    Write,
    Sync,
}

#[cfg(test)]
pub(crate) struct WorkflowLearningPreparedFileGateForTest {
    pub(crate) prepared: tokio::sync::oneshot::Sender<()>,
    pub(crate) release: tokio::sync::oneshot::Receiver<()>,
}

#[derive(Default)]
struct WorkflowLearningFilePreparation {
    #[cfg(test)]
    fault: Option<WorkflowLearningPreparationFaultForTest>,
    #[cfg(test)]
    prepared_gate: Option<WorkflowLearningPreparedFileGateForTest>,
}

impl WorkflowLearningFilePreparation {
    fn before_write(&self) -> std::io::Result<()> {
        #[cfg(test)]
        if self.fault == Some(WorkflowLearningPreparationFaultForTest::Write) {
            return Err(std::io::Error::other("injected candidate write failure"));
        }
        Ok(())
    }

    fn before_sync(&self) -> std::io::Result<()> {
        #[cfg(test)]
        if self.fault == Some(WorkflowLearningPreparationFaultForTest::Sync) {
            return Err(std::io::Error::other("injected candidate sync failure"));
        }
        Ok(())
    }

    async fn after_preparation(&mut self) -> anyhow::Result<()> {
        #[cfg(test)]
        if let Some(gate) = self.prepared_gate.take() {
            let _ = gate.prepared.send(());
            gate.release.await.context("candidate prepared-file test gate closed")?;
        }
        Ok(())
    }
}

impl Drop for PreparedWorkflowLearningFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

async fn prepare_workflow_learning_file(
    store: &WorkflowLearningCandidateStore,
    destination: &Path,
    rows: &WorkflowLearningCandidateMap,
    preparation: &mut WorkflowLearningFilePreparation,
) -> anyhow::Result<PreparedWorkflowLearningFile> {
    use tokio::io::AsyncWriteExt;

    // No candidate plaintext reaches the filesystem, including temporary files.
    let payload = store.seal(rows)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).await?;
    }
    let temporary_path = destination.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&temporary_path).await?;
    let prepared = PreparedWorkflowLearningFile(temporary_path);
    let result = async {
        preparation.before_write()?;
        file.write_all(payload.as_bytes()).await?;
        preparation.before_sync()?;
        file.sync_all().await?;
        anyhow::Ok(())
    }.await;
    // Windows must close the file before RAII cleanup on a failed preparation.
    drop(file);
    result?;
    preparation.after_preparation().await?;
    Ok(prepared)
}

impl AppState {
    fn workflow_learning_hosted_storage_required(&self) -> anyhow::Result<bool> {
        let configured = self.hosted_policy_source_configured().map_err(anyhow::Error::msg)?;
        let installed = self.enterprise.hosted_policy.current().map_err(anyhow::Error::msg)?.is_some();
        Ok(configured || installed || tandem_memory::envelope::hosted_memory_encryption_required())
    }

    pub async fn load_workflow_learning_candidates(&self) -> anyhow::Result<()> {
        let configuration = crate::encrypted_file_store::CapturedFileCryptoConfiguration::capture();
        let state = self.clone();
        tokio::spawn(async move {
            let mut rows = state.workflow_learning_candidates.write().await;
            let _publication = state.enterprise.hosted_policy.lock_publication_owned().await;
            let hosted_required = state.workflow_learning_hosted_storage_required()?;
            let Some(stored) = read_workflow_learning_candidate_store(&state).await? else {
                anyhow::ensure!(rows.is_empty(), "workflow-learning candidate store is missing after initialization");
                if hosted_required || configuration.is_hosted() {
                    let store = WorkflowLearningCandidateStore::from_configuration(&state, configuration)?;
                    store.require_current_crypto(&state)?;
                }
                // An absent genuine standalone feature store needs no key.
                return Ok(());
            };
            let store = WorkflowLearningCandidateStore::from_configuration(&state, configuration)?;
            store.require_current_crypto(&state)?;
            let loaded = store.open(&stored, !hosted_required && !store.crypto.is_hosted())?;
            if loaded.legacy {
                // Only genuine standalone legacy rows reach this branch. Their
                // complete encrypted migration precedes the first cache swap.
                let prepared = prepare_workflow_learning_file(&store, &state.workflow_learning_candidates_path, &loaded.rows, &mut WorkflowLearningFilePreparation::default()).await?;
                store.require_current_crypto(&state)?;
                prepared.publish(&state.workflow_learning_candidates_path)?;
            } else {
                store.require_current_crypto(&state)?;
            }
            *rows = loaded.rows;
            Ok(())
        }).await?
    }

    async fn mutate_workflow_learning_candidates<T, F>(&self, mutation: F) -> anyhow::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut WorkflowLearningCandidateMap) -> anyhow::Result<(T, bool)> + Send + 'static,
    {
        self.mutate_workflow_learning_candidates_with_preparation(
            mutation, WorkflowLearningFilePreparation::default(),
        ).await
    }

    async fn mutate_workflow_learning_candidates_with_preparation<T, F>(
        &self,
        mutation: F,
        mut preparation: WorkflowLearningFilePreparation,
    ) -> anyhow::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut WorkflowLearningCandidateMap) -> anyhow::Result<(T, bool)> + Send + 'static,
    {
        let store = WorkflowLearningCandidateStore::capture(self)?;
        let state = self.clone();
        tokio::spawn(async move {
            let mut rows = state.workflow_learning_candidates.write().await;
            let _publication = state.enterprise.hosted_policy.lock_publication_owned().await;
            store.require_current_crypto(&state)?;
            // Corrupt, forbidden legacy, or unavailable durable state cannot be
            // overwritten by an apparently healthy in-memory map.
            store.require_existing_state(&state, &rows).await?;
            let mut next = rows.clone();
            let (result, publish) = mutation(&mut next)?;
            if !publish {
                return Ok(result);
            }
            let prepared = prepare_workflow_learning_file(&store, &state.workflow_learning_candidates_path, &next, &mut preparation).await?;
            store.require_current_crypto(&state)?;
            prepared.publish(&state.workflow_learning_candidates_path)?;
            *rows = next;
            Ok(result)
        }).await?
    }

    pub async fn persist_workflow_learning_candidates(&self) -> anyhow::Result<()> {
        self.mutate_workflow_learning_candidates(|_| Ok(((), true))).await
    }

    pub async fn put_workflow_learning_candidate(
        &self,
        candidate: WorkflowLearningCandidate,
    ) -> anyhow::Result<WorkflowLearningCandidate> {
        self.put_workflow_learning_candidate_with_preparation(
            candidate, WorkflowLearningFilePreparation::default(),
        ).await
    }

    async fn put_workflow_learning_candidate_with_preparation(
        &self,
        mut candidate: WorkflowLearningCandidate,
        preparation: WorkflowLearningFilePreparation,
    ) -> anyhow::Result<WorkflowLearningCandidate> {
        anyhow::ensure!(!candidate.candidate_id.trim().is_empty(), "candidate_id is required");
        self.mutate_workflow_learning_candidates_with_preparation(move |rows| {
            let now = now_ms();
            if candidate.created_at_ms == 0 {
                candidate.created_at_ms = now;
            }
            candidate.updated_at_ms = now;
            rows.insert(candidate.candidate_id.clone(), candidate.clone());
            Ok((candidate, true))
        }, preparation).await
    }

    pub async fn upsert_workflow_learning_candidate(
        &self,
        candidate: WorkflowLearningCandidate,
    ) -> anyhow::Result<WorkflowLearningCandidate> {
        self.upsert_workflow_learning_candidate_with_preparation(
            candidate, WorkflowLearningFilePreparation::default(),
        ).await
    }

    async fn upsert_workflow_learning_candidate_with_preparation(
        &self,
        candidate: WorkflowLearningCandidate,
        preparation: WorkflowLearningFilePreparation,
    ) -> anyhow::Result<WorkflowLearningCandidate> {
        self.mutate_workflow_learning_candidates_with_preparation(move |rows| {
            Ok((merge_workflow_learning_candidate(rows, candidate), true))
        }, preparation).await
    }

    #[cfg(test)]
    pub(crate) async fn put_workflow_learning_candidate_with_preparation_fault_for_test(
        &self,
        candidate: WorkflowLearningCandidate,
        fault: WorkflowLearningPreparationFaultForTest,
    ) -> anyhow::Result<WorkflowLearningCandidate> {
        self.put_workflow_learning_candidate_with_preparation(
            candidate,
            WorkflowLearningFilePreparation { fault: Some(fault), ..Default::default() },
        ).await
    }

    #[cfg(test)]
    pub(crate) async fn put_workflow_learning_candidate_with_prepared_file_gate_for_test(
        &self,
        candidate: WorkflowLearningCandidate,
        gate: WorkflowLearningPreparedFileGateForTest,
    ) -> anyhow::Result<WorkflowLearningCandidate> {
        self.put_workflow_learning_candidate_with_preparation(
            candidate,
            WorkflowLearningFilePreparation { prepared_gate: Some(gate), ..Default::default() },
        ).await
    }

    #[cfg(test)]
    pub(crate) async fn upsert_workflow_learning_candidate_with_preparation_fault_for_test(
        &self,
        candidate: WorkflowLearningCandidate,
        fault: WorkflowLearningPreparationFaultForTest,
    ) -> anyhow::Result<WorkflowLearningCandidate> {
        self.upsert_workflow_learning_candidate_with_preparation(
            candidate,
            WorkflowLearningFilePreparation { fault: Some(fault), ..Default::default() },
        ).await
    }

    #[cfg(test)]
    pub(crate) async fn upsert_workflow_learning_candidate_with_prepared_file_gate_for_test(
        &self,
        candidate: WorkflowLearningCandidate,
        gate: WorkflowLearningPreparedFileGateForTest,
    ) -> anyhow::Result<WorkflowLearningCandidate> {
        self.upsert_workflow_learning_candidate_with_preparation(
            candidate,
            WorkflowLearningFilePreparation { prepared_gate: Some(gate), ..Default::default() },
        ).await
    }

    pub async fn update_workflow_learning_candidate(
        &self,
        candidate_id: &str,
        update: impl FnOnce(&mut WorkflowLearningCandidate) + Send + 'static,
    ) -> anyhow::Result<Option<WorkflowLearningCandidate>> {
        self.update_workflow_learning_candidate_with_preparation(
            candidate_id, update, WorkflowLearningFilePreparation::default(),
        ).await
    }

    async fn update_workflow_learning_candidate_with_preparation(
        &self,
        candidate_id: &str,
        update: impl FnOnce(&mut WorkflowLearningCandidate) + Send + 'static,
        preparation: WorkflowLearningFilePreparation,
    ) -> anyhow::Result<Option<WorkflowLearningCandidate>> {
        let candidate_id = candidate_id.to_string();
        self.mutate_workflow_learning_candidates_with_preparation(move |rows| {
            let Some(candidate) = rows.get_mut(&candidate_id) else {
                return Ok((None, false));
            };
            update(candidate);
            candidate.updated_at_ms = now_ms();
            Ok((Some(candidate.clone()), true))
        }, preparation).await
    }

    #[cfg(test)]
    pub(crate) async fn update_workflow_learning_candidate_with_preparation_fault_for_test(
        &self,
        candidate_id: &str,
        update: impl FnOnce(&mut WorkflowLearningCandidate) + Send + 'static,
        fault: WorkflowLearningPreparationFaultForTest,
    ) -> anyhow::Result<Option<WorkflowLearningCandidate>> {
        self.update_workflow_learning_candidate_with_preparation(
            candidate_id,
            update,
            WorkflowLearningFilePreparation { fault: Some(fault), ..Default::default() },
        ).await
    }

    #[cfg(test)]
    pub(crate) async fn update_workflow_learning_candidate_with_prepared_file_gate_for_test(
        &self,
        candidate_id: &str,
        update: impl FnOnce(&mut WorkflowLearningCandidate) + Send + 'static,
        gate: WorkflowLearningPreparedFileGateForTest,
    ) -> anyhow::Result<Option<WorkflowLearningCandidate>> {
        self.update_workflow_learning_candidate_with_preparation(
            candidate_id,
            update,
            WorkflowLearningFilePreparation { prepared_gate: Some(gate), ..Default::default() },
        ).await
    }
}
