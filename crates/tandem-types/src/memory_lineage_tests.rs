use super::*;
use crate::{MessagePart, MessageRole};
use serde_json::json;

#[test]
fn digest_binds_role_ordered_parts_and_nested_tool_values() {
    let message = Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::Text {
                text: "native answer".into(),
            },
            MessagePart::ToolInvocation {
                tool: "memory_search".into(),
                args: json!({"query": "canonical", "nested": {"a": 1, "z": 2}}),
                result: Some(json!({"count": 1})),
                error: None,
            },
        ],
    );
    let digest = canonical_message_digest(&message);
    let mut changed = message.clone();
    changed.role = MessageRole::User;
    assert_ne!(digest, canonical_message_digest(&changed));
    changed = message.clone();
    changed.parts.reverse();
    assert_ne!(digest, canonical_message_digest(&changed));
    changed = message.clone();
    if let MessagePart::ToolInvocation { result, .. } = &mut changed.parts[1] {
        *result = Some(json!({"count": 2}));
    }
    assert_ne!(digest, canonical_message_digest(&changed));
    changed = message.clone();
    changed.id = "different-native-id".into();
    changed.created_at += chrono::Duration::seconds(1);
    assert_eq!(digest, canonical_message_digest(&changed));
}

#[test]
fn digest_excludes_lineage_and_lineage_survives_header_json_roundtrip() {
    let mut message = Message::new(
        MessageRole::Assistant,
        vec![MessagePart::Text {
            text: "answer".into(),
        }],
    );
    let digest = canonical_message_digest(&message);
    message.source_lineage = Some(NativeMessageLineage {
        schema_version: 1,
        run_id: "native-run".into(),
        tenant_context: TenantContext::default(),
        subject: "native-subject".into(),
        message_digest: digest.clone(),
        input_message_ids: vec!["native-input".into()],
        included_memory: vec![MemorySourceReference {
            memory_id: "memory-source".into(),
            content_hash: "content-digest".into(),
            restriction_digest: "restriction-digest".into(),
        }],
        complete: true,
    });
    assert_eq!(digest, canonical_message_digest(&message));
    let restored: Message = serde_json::from_slice(&serde_json::to_vec(&message).unwrap()).unwrap();
    assert_eq!(restored.source_lineage.unwrap().message_digest, digest);
    let mut legacy = serde_json::to_value(&message).unwrap();
    legacy.as_object_mut().unwrap().remove("source_lineage");
    assert!(serde_json::from_value::<Message>(legacy)
        .unwrap()
        .source_lineage
        .is_none());
}

#[test]
fn digest_canonicalizes_nested_object_key_order() {
    let first: serde_json::Value =
        serde_json::from_str(r#"{"z":{"two":2,"one":1},"a":0}"#).unwrap();
    let second: serde_json::Value =
        serde_json::from_str(r#"{"a":0,"z":{"one":1,"two":2}}"#).unwrap();
    let make = |args| {
        Message::new(
            MessageRole::User,
            vec![MessagePart::ToolInvocation {
                tool: "lookup".into(),
                args,
                result: None,
                error: None,
            }],
        )
    };
    assert_eq!(
        canonical_message_digest(&make(first)),
        canonical_message_digest(&make(second))
    );
}
