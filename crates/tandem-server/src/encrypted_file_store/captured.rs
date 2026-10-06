// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::{
    crypto, required_crypto_for, validate_hosted_crypto_handle_ready, ProtectedFileCrypto,
    ProtectedRecordContext,
};

/// An independently configured crypto handle captured before an owned writer
/// is spawned. Task-local test providers, principals and DEK caches therefore
/// cannot be replaced by ambient configuration at the actual publication.
#[derive(Clone)]
pub(crate) struct CapturedRequiredFileCrypto(ProtectedFileCrypto);

/// Capture ambient/task-local configuration without demanding a key for a
/// genuinely absent standalone store. Required sealing is resolved only after
/// the owned loader has observed the file under its actual writer lock.
pub(crate) struct CapturedFileCryptoConfiguration(ProtectedFileCrypto);

impl CapturedFileCryptoConfiguration {
    pub(crate) fn capture() -> Self {
        Self(crypto())
    }

    pub(crate) fn into_required(
        self,
        context: &ProtectedRecordContext,
        hosted_required: bool,
    ) -> anyhow::Result<CapturedRequiredFileCrypto> {
        ProtectedFileCrypto::validate_context(context)?;
        let required =
            hosted_required || tandem_memory::envelope::hosted_memory_encryption_required();
        let handle = CapturedRequiredFileCrypto(required_crypto_for(self.0, required)?);
        handle.validate_required_mode(required)?;
        Ok(handle)
    }

    pub(crate) fn is_hosted(&self) -> bool {
        self.0.provider.is_hosted()
    }
}

impl CapturedRequiredFileCrypto {
    pub(crate) fn capture(
        context: &ProtectedRecordContext,
        hosted_required: bool,
    ) -> anyhow::Result<Self> {
        CapturedFileCryptoConfiguration::capture().into_required(context, hosted_required)
    }

    pub(crate) fn validate_required_mode(&self, hosted_required: bool) -> anyhow::Result<()> {
        anyhow::ensure!(
            !hosted_required || self.0.provider.is_hosted(),
            "hosted candidate storage requires a hosted KMS provider"
        );
        anyhow::ensure!(
            self.0.provider.is_encrypted_ready(),
            "candidate storage encryption key is unavailable"
        );
        Ok(())
    }

    pub(crate) fn validate_hosted_ready(
        &self,
        context: &ProtectedRecordContext,
    ) -> anyhow::Result<()> {
        validate_hosted_crypto_handle_ready(&self.0, context)
    }

    pub(crate) fn encrypt(
        &self,
        plaintext: &str,
        context: &ProtectedRecordContext,
    ) -> anyhow::Result<String> {
        self.0.encrypt_record(plaintext, context)
    }

    pub(crate) fn decrypt(
        &self,
        stored: &str,
        expected: &ProtectedRecordContext,
    ) -> anyhow::Result<String> {
        self.0.decrypt_record(stored.trim(), expected)
    }

    pub(crate) fn is_hosted(&self) -> bool {
        self.0.provider.is_hosted()
    }
}
