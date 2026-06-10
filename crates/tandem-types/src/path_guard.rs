//! Shared sensitive-path predicate.
//!
//! Runtime tools that resolve filesystem paths (read, MCP file tools, shell
//! sandboxing) must agree on which paths are sensitive — credentials, private
//! keys, and secret config files — so they can be uniformly blocked. This is
//! the single shared classifier; callers that resolve paths through fallbacks
//! (e.g. basename search) must re-check the *resolved* path here.

use std::path::Path;

/// Returns true if a resolved path points at a sensitive credential, key, or
/// secret-config file that runtime tools must not read or mutate.
pub fn is_sensitive_path(path: &Path) -> bool {
    // Normalize for the substring checks below: Windows separators become
    // `/`, and a leading `/` is prepended so relative inputs like
    // `.aws/credentials` hit the same `/.aws/credentials` patterns as
    // absolute resolved paths. Both were false negatives before.
    let lowered = format!(
        "/{}",
        path.to_string_lossy()
            .to_ascii_lowercase()
            .replace('\\', "/")
            .trim_start_matches('/')
    );

    // SSH / GPG directories
    if lowered.contains("/.ssh/") || lowered.ends_with("/.ssh") {
        return true;
    }
    if lowered.contains("/.gnupg/") || lowered.ends_with("/.gnupg") {
        return true;
    }

    // Cloud credential files
    if lowered.contains("/.aws/credentials")
        || lowered.contains("/.config/gcloud/")
        || lowered.contains("/.docker/config.json")
        || lowered.contains("/.kube/config")
        || lowered.contains("/.git-credentials")
    {
        return true;
    }

    // Package manager / tool secrets
    if lowered.ends_with("/.npmrc") || lowered.ends_with("/.netrc") || lowered.ends_with("/.pypirc")
    {
        return true;
    }

    // Known private key file names (use file_name() to avoid false positives on paths)
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let n = name.to_ascii_lowercase();
        // .env files (but not .env.example — check no extra extension after .env)
        if n == ".env"
            || n.starts_with(".env.") && !n.ends_with(".example") && !n.ends_with(".sample")
        {
            return true;
        }
        // Key identity files
        if n.starts_with("id_rsa")
            || n.starts_with("id_ed25519")
            || n.starts_with("id_ecdsa")
            || n.starts_with("id_dsa")
        {
            return true;
        }
    }

    // Certificate / private key extensions — use extension() to avoid substring false positives
    // e.g. keyboard.rs has no .key extension, so it won't match here.
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_ascii_lowercase();
        if matches!(
            ext_lower.as_str(),
            "pem" | "p12" | "pfx" | "key" | "keystore" | "jks"
        ) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn flags_credential_directories_and_files() {
        assert!(is_sensitive_path(&p("/home/u/.ssh/id_rsa")));
        assert!(is_sensitive_path(&p("/home/u/.ssh")));
        assert!(is_sensitive_path(&p("/home/u/.aws/credentials")));
        assert!(is_sensitive_path(&p("/work/.docker/config.json")));
        assert!(is_sensitive_path(&p("/home/u/.kube/config")));
        assert!(is_sensitive_path(&p("/home/u/.git-credentials")));
        assert!(is_sensitive_path(&p("/home/u/.npmrc")));
    }

    #[test]
    fn flags_env_and_key_files_by_name_and_extension() {
        assert!(is_sensitive_path(&p("/work/.env")));
        assert!(is_sensitive_path(&p("/work/.env.local")));
        assert!(is_sensitive_path(&p("/work/id_ed25519")));
        assert!(is_sensitive_path(&p("/work/server.key")));
        assert!(is_sensitive_path(&p("/work/cert.pem")));
        assert!(is_sensitive_path(&p("/work/store.jks")));
    }

    #[test]
    fn flags_relative_and_windows_style_paths() {
        assert!(is_sensitive_path(&p(".aws/credentials")));
        assert!(is_sensitive_path(&p(".docker/config.json")));
        assert!(is_sensitive_path(&p(".ssh/id_rsa")));
        assert!(is_sensitive_path(&p(".ssh")));
        assert!(is_sensitive_path(&p(r"C:\Users\u\.aws\credentials")));
        assert!(is_sensitive_path(&p(r"C:\work\.docker\config.json")));
        assert!(is_sensitive_path(&p(r"users\u\.ssh")));
    }

    #[test]
    fn allows_benign_paths() {
        assert!(!is_sensitive_path(&p("/work/src/main.rs")));
        assert!(!is_sensitive_path(&p("/work/keyboard.rs")));
        assert!(!is_sensitive_path(&p("/work/.env.example")));
        assert!(!is_sensitive_path(&p("/work/config.json")));
        assert!(!is_sensitive_path(&p("/work/README.md")));
    }
}
