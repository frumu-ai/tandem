use super::*;
use tandem_types::VerifiedTenantContext;

impl EngineLoop {
    pub(super) async fn append_final_response(
        &self,
        session_id: &str,
        completion: &str,
        run_id: Option<&str>,
        original_verified: Option<VerifiedTenantContext>,
        source_lineage: Option<tandem_types::NativeMessageLineage>,
    ) -> anyhow::Result<()> {
        let mut assistant = Message::new(
            MessageRole::Assistant,
            vec![MessagePart::Text {
                text: completion.to_string(),
            }],
        );
        assistant.source_lineage = source_lineage.map(|mut lineage| {
            lineage.message_digest = tandem_types::canonical_message_digest(&assistant);
            lineage
        });
        let final_part =
            WireMessagePart::text(session_id, &assistant.id, truncate_text(completion, 16_000));
        let mut props = json!({"part": final_part, "sessionID": session_id});
        if let Some(run_id) = run_id {
            props
                .as_object_mut()
                .unwrap()
                .insert("runID".to_string(), json!(run_id));
        }
        let success_events = [
            EngineEvent::new("message.part.updated", props),
            EngineEvent::new(
                "session.updated",
                json!({"sessionID": session_id, "status": "idle"}),
            ),
            EngineEvent::new(
                "session.status",
                json!({"sessionID": session_id, "status": "idle"}),
            ),
        ];
        // Own the Arc, not an async registry guard, across the blocking append.
        let hook = self.tool_policy_hook.read().await.clone();
        let bus = self.event_bus.clone();
        self.storage
            .append_message_with_commit_guard(session_id, assistant, move |commit| {
                // SQLite has acquired its writer before this callback. The host
                // retains current policy through both durability and publication.
                let mut commit_and_publish = || {
                    commit()?;
                    for event in &success_events {
                        bus.publish(event.clone());
                    }
                    Ok(())
                };
                if let Some(hook) = hook {
                    hook.with_session_commit_authority(original_verified, &mut commit_and_publish)
                } else {
                    commit_and_publish()
                }
            })
            .await
    }
}
