#[cfg(test)]
mod tests {
    use super::*;

    fn text_message(role: MessageRole, id: &str, text: &str) -> Message {
        let mut message = Message::new(
            role,
            vec![MessagePart::Text {
                text: text.to_string(),
            }],
        );
        message.id = id.to_string();
        message
    }

    #[test]
    fn latest_archived_exchange_candidate_uses_latest_user_assistant_pair() {
        let mut session = Session::new(Some("chat".to_string()), Some(".".to_string()));
        session.workspace_root = Some("/tmp/tandem".to_string());
        session.project_id = Some("workspace-123".to_string());
        session.messages = vec![
            text_message(MessageRole::User, "u1", "first request"),
            text_message(MessageRole::Assistant, "a1", "first answer"),
            text_message(MessageRole::User, "u2", "second request"),
            text_message(MessageRole::Assistant, "a2", "second answer"),
        ];

        let candidate = latest_archived_exchange_candidate(&session).expect("candidate");
        assert_eq!(candidate.user_message_id, "u2");
        assert_eq!(candidate.assistant_message_id, "a2");
        assert_eq!(candidate.user_text, "second request");
        assert_eq!(candidate.assistant_text, "second answer");
    }

    #[test]
    fn latest_archived_exchange_candidate_skips_slash_commands_and_errors() {
        let mut session = Session::new(Some("chat".to_string()), Some(".".to_string()));
        session.messages = vec![
            text_message(MessageRole::User, "u1", "/new"),
            text_message(
                MessageRole::Assistant,
                "a1",
                "ENGINE_ERROR: ENGINE_DISPATCH_FAILED: boom",
            ),
            text_message(MessageRole::User, "u2", "real question"),
            text_message(MessageRole::Assistant, "a2", "real answer"),
        ];

        let candidate = latest_archived_exchange_candidate(&session).expect("candidate");
        assert_eq!(candidate.user_message_id, "u2");
        assert_eq!(candidate.assistant_message_id, "a2");
    }

    #[test]
    fn latest_archived_exchange_candidate_ignores_reasoning_parts() {
        let mut session = Session::new(Some("chat".to_string()), Some(".".to_string()));
        let mut user = Message::new(
            MessageRole::User,
            vec![MessagePart::Text {
                text: "what changed?".to_string(),
            }],
        );
        user.id = "u1".to_string();
        let mut assistant = Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::Reasoning {
                    text: "private chain of thought".to_string(),
                },
                MessagePart::Text {
                    text: "We archived the exchange.".to_string(),
                },
            ],
        );
        assistant.id = "a1".to_string();
        session.messages = vec![user, assistant];

        let candidate = latest_archived_exchange_candidate(&session).expect("candidate");
        assert_eq!(candidate.user_text, "what changed?");
        assert_eq!(candidate.assistant_text, "We archived the exchange.");
    }

    #[test]
    fn archive_source_hash_is_stable_for_same_exchange() {
        let candidate = ArchivedExchangeCandidate {
            user_message_id: "u1".to_string(),
            assistant_message_id: "a1".to_string(),
            user_text: "hello".to_string(),
            assistant_text: "world".to_string(),
        };

        let a = archive_source_hash("session-1", &candidate);
        let b = archive_source_hash("session-1", &candidate);
        let c = archive_source_hash("session-2", &candidate);

        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
