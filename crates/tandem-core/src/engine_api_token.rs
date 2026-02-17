use std::path::PathBuf;
use uuid::Uuid;

use crate::resolve_shared_paths;

const TOKEN_SERVICE: &str = "ai.frumu.tandem";
const TOKEN_ACCOUNT: &str = "engine_api_token";

#[derive(Debug, Clone)]
pub struct EngineApiTokenMaterial {
    pub token: String,
    pub backend: String,
    pub file_path: PathBuf,
}

pub fn engine_api_token_file_path() -> PathBuf {
    if let Ok(paths) = resolve_shared_paths() {
        return paths
            .canonical_root
            .join("security")
            .join("engine_api_token");
    }
    PathBuf::from(".tandem").join("engine_api_token")
}

fn new_token() -> String {
    format!("tk_{}", Uuid::new_v4().simple())
}

fn keyring_entry() -> Option<keyring::Entry> {
    keyring::Entry::new(TOKEN_SERVICE, TOKEN_ACCOUNT).ok()
}

fn read_file_token(path: &PathBuf) -> Option<String> {
    let existing = std::fs::read_to_string(path).ok()?;
    let token = existing.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn write_file_token(path: &PathBuf, token: &str) -> bool {
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    std::fs::write(path, token).is_ok()
}

pub fn load_or_create_engine_api_token() -> EngineApiTokenMaterial {
    let file_path = engine_api_token_file_path();

    if let Some(entry) = keyring_entry() {
        if let Ok(token) = entry.get_password() {
            let token = token.trim().to_string();
            if !token.is_empty() {
                return EngineApiTokenMaterial {
                    token,
                    backend: "keychain".to_string(),
                    file_path,
                };
            }
        }
    }

    if let Some(token) = read_file_token(&file_path) {
        if let Some(entry) = keyring_entry() {
            let _ = entry.set_password(&token);
        }
        return EngineApiTokenMaterial {
            token,
            backend: "file".to_string(),
            file_path,
        };
    }

    let token = new_token();
    if let Some(entry) = keyring_entry() {
        if entry.set_password(&token).is_ok() {
            return EngineApiTokenMaterial {
                token,
                backend: "keychain".to_string(),
                file_path,
            };
        }
    }

    let wrote_file = write_file_token(&file_path, &token);
    EngineApiTokenMaterial {
        token,
        backend: if wrote_file {
            "file".to_string()
        } else {
            "memory".to_string()
        },
        file_path,
    }
}
