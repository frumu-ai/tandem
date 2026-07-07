use std::path::Path;

use anyhow::Context;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tandem_memory::MemoryCryptoProvider;
use tokio::fs;

pub(crate) const ENCRYPTED_PAYLOAD_PREFIX: &str = "tce1:";

fn crypto_provider() -> MemoryCryptoProvider {
    MemoryCryptoProvider::from_env()
}

pub(crate) fn is_encrypted_payload(stored: &str) -> bool {
    stored.trim_start().starts_with(ENCRYPTED_PAYLOAD_PREFIX)
}

pub(crate) fn encrypt_text(plaintext: &str) -> anyhow::Result<String> {
    crypto_provider()
        .encrypt_field(plaintext)
        .context("encrypt protected file-store payload")
}

pub(crate) fn decrypt_text(stored: &str) -> anyhow::Result<String> {
    let provider = crypto_provider();
    if provider.is_plaintext() && is_encrypted_payload(stored) {
        return Err(anyhow::Error::msg(
            "encrypted protected file-store payload requires a configured decrypt provider",
        ));
    }
    provider
        .decrypt_field(stored)
        .context("decrypt protected file-store payload")
}

pub(crate) fn encrypt_jsonl_line(plaintext: &str) -> anyhow::Result<String> {
    encrypt_text(plaintext)
}

pub(crate) fn decrypt_jsonl_line(stored: &str) -> anyhow::Result<Option<String>> {
    let trimmed = stored.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    decrypt_text(trimmed).map(Some)
}

pub(crate) async fn read_text_file(path: &Path) -> anyhow::Result<String> {
    let stored = fs::read_to_string(path).await?;
    decrypt_text(&stored)
}

pub(crate) async fn write_text_file(path: &Path, plaintext: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let stored = encrypt_text(plaintext)?;
    fs::write(path, stored).await?;
    Ok(())
}

pub(crate) async fn read_json_file<T>(path: &Path) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let plaintext = read_text_file(path).await?;
    serde_json::from_str(&plaintext).with_context(|| {
        format!(
            "parse protected file-store JSON payload from {}",
            path.display()
        )
    })
}

pub(crate) async fn write_json_file<T>(path: &Path, value: &T) -> anyhow::Result<()>
where
    T: Serialize,
{
    let plaintext = serde_json::to_string_pretty(value)?;
    write_text_file(path, &plaintext).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::collections::HashMap;

    struct EnvRestore {
        provider: Option<String>,
        key_file: Option<String>,
        required: Option<String>,
        principal: Option<String>,
    }

    impl EnvRestore {
        fn capture() -> Self {
            Self {
                provider: std::env::var("TANDEM_MEMORY_DECRYPT_PROVIDER").ok(),
                key_file: std::env::var("TANDEM_MEMORY_LOCAL_KEY_FILE").ok(),
                required: std::env::var("TANDEM_MEMORY_ENCRYPTION_REQUIRED").ok(),
                principal: std::env::var("TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID").ok(),
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            restore_var("TANDEM_MEMORY_DECRYPT_PROVIDER", self.provider.as_deref());
            restore_var("TANDEM_MEMORY_LOCAL_KEY_FILE", self.key_file.as_deref());
            restore_var(
                "TANDEM_MEMORY_ENCRYPTION_REQUIRED",
                self.required.as_deref(),
            );
            restore_var(
                "TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID",
                self.principal.as_deref(),
            );
        }
    }

    fn restore_var(key: &str, value: Option<&str>) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    fn enable_local_encrypted(dir: &tempfile::TempDir) -> EnvRestore {
        let restore = EnvRestore::capture();
        std::env::set_var("TANDEM_MEMORY_DECRYPT_PROVIDER", "local-file");
        std::env::set_var(
            "TANDEM_MEMORY_LOCAL_KEY_FILE",
            dir.path().join("local_memory.key"),
        );
        std::env::remove_var("TANDEM_MEMORY_ENCRYPTION_REQUIRED");
        std::env::remove_var("TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID");
        restore
    }

    #[tokio::test]
    #[serial]
    async fn whole_file_json_round_trips_as_ciphertext() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _restore = enable_local_encrypted(&dir);
        let path = dir.path().join("policy_decisions.json");
        let payload = HashMap::from([(
            "decision-1".to_string(),
            serde_json::json!({"tenant": "acme", "secret": "finance-decision"}),
        )]);

        write_json_file(&path, &payload)
            .await
            .expect("write encrypted");
        let raw = fs::read_to_string(&path).await.expect("read raw");
        assert!(is_encrypted_payload(&raw));
        assert!(!raw.contains("finance-decision"));

        let decoded: HashMap<String, serde_json::Value> =
            read_json_file(&path).await.expect("read encrypted");
        assert_eq!(decoded, payload);
    }

    #[tokio::test]
    #[serial]
    async fn encrypted_payload_without_decrypt_provider_fails_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let encrypted = {
            let _restore = enable_local_encrypted(&dir);
            encrypt_text("protected-store-secret").expect("encrypt with local provider")
        };
        let _restore = EnvRestore::capture();
        std::env::remove_var("TANDEM_MEMORY_DECRYPT_PROVIDER");
        std::env::remove_var("TANDEM_MEMORY_LOCAL_KEY_FILE");
        std::env::remove_var("TANDEM_MEMORY_ENCRYPTION_REQUIRED");
        std::env::remove_var("TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID");

        let err = decrypt_text(&encrypted).expect_err("fail closed without provider");
        if !err
            .to_string()
            .contains("requires a configured decrypt provider")
        {
            std::panic::panic_any("unexpected decrypt provider error");
        }
    }

    #[tokio::test]
    #[serial]
    async fn hosted_required_refuses_plaintext_write() {
        let _restore = EnvRestore::capture();
        std::env::set_var("TANDEM_MEMORY_ENCRYPTION_REQUIRED", "true");
        std::env::set_var("TANDEM_MEMORY_DECRYPT_PROVIDER", "aws-kms");
        std::env::set_var("TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID", "runtime-tandem");
        std::env::remove_var("TANDEM_MEMORY_LOCAL_KEY_FILE");

        let err = encrypt_text("must not land as plaintext").expect_err("fail closed");
        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("refusing to store plaintext"),
            "unexpected error: {rendered}"
        );
    }
}
