use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{Message, TenantContext};

/// A reference to an authorized canonical memory source, never its plaintext.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemorySourceReference {
    pub memory_id: String,
    pub content_hash: String,
    pub restriction_digest: String,
}

/// Native provenance bound to the persisted message body. Request DTOs do not
/// accept this structure; missing legacy provenance cannot confer sharing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeMessageLineage {
    pub schema_version: u8,
    pub run_id: String,
    pub tenant_context: TenantContext,
    pub subject: String,
    pub message_digest: String,
    pub input_message_ids: Vec<String>,
    pub included_memory: Vec<MemorySourceReference>,
    pub complete: bool,
}

/// Digest only the native role and ordered parts. IDs, timestamps and lineage
/// are deliberately excluded so provenance cannot authenticate itself.
pub fn canonical_message_digest(message: &Message) -> String {
    let mut body = serde_json::to_value((&message.role, &message.parts))
        .expect("native message body is JSON serializable");
    sort_objects(&mut body);
    let bytes = serde_json::to_vec(&body).expect("canonical native message body is JSON serializable");
    format!("{:x}", Sha256::digest(bytes))
}

fn sort_objects(value: &mut Value) {
    match value {
        Value::Object(object) => {
            let mut entries = std::mem::take(object).into_iter().collect::<Vec<_>>();
            entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
            for (key, mut value) in entries {
                sort_objects(&mut value);
                object.insert(key, value);
            }
        }
        Value::Array(values) => values.iter_mut().for_each(sort_objects),
        _ => {}
    }
}

#[cfg(test)]
#[path = "memory_lineage_tests.rs"]
mod tests;
