#[cfg(test)]
mod attempt_accounting_tests {
    use super::provider_attempt_tests::{compatible, reply, responses};
    use super::*;

    fn recording_policy(
        attempts: Arc<Mutex<Vec<ProviderAttempt>>>,
        outcomes: Arc<Mutex<Vec<(usize, ProviderAttemptOutcome)>>>,
        limit: usize,
    ) -> ProviderAttemptPolicy {
        ProviderAttemptPolicy::new(10, 4096, move |attempt| {
            let mut captured = attempts.lock().unwrap();
            let index = captured.len();
            captured.push(attempt);
            let outcomes = outcomes.clone();
            async move {
                anyhow::ensure!(index < limit, "synthetic attempt budget exhausted");
                Ok(ProviderAttemptReceipt::new(move |outcome| {
                    outcomes.lock().unwrap().push((index, outcome));
                    async { Ok(()) }
                }))
            }
        })
        .unwrap()
    }

    #[tokio::test]
    async fn attempt_accounting_bounds_actual_complete_and_records_confirmed_usage() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider = compatible(format!("http://{}", listener.local_addr().unwrap()));
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (_, _, body) = read_single_http_request(&mut socket).await;
            let body: serde_json::Value = serde_json::from_str(&body).unwrap();
            assert_eq!(body["max_tokens"], 10);
            reply(&mut socket, "200 OK", r#"{"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}}"#).await;
        });
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let outcomes = Arc::new(Mutex::new(Vec::new()));
        let policy = recording_policy(attempts.clone(), outcomes.clone(), 1);
        let output = PROVIDER_PRIVATE_ENDPOINTS_ALLOWED
            .scope(
                true,
                policy.scope(provider.complete("synthetic private prompt", None)),
            )
            .await
            .unwrap();
        assert_eq!(output, "ok");
        tokio::time::timeout(Duration::from_secs(3), server)
            .await
            .unwrap()
            .unwrap();
        let attempts = attempts.lock().unwrap();
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].maximum_output_tokens, 10);
        assert!(!format!("{:?}", attempts[0]).contains("synthetic private prompt"));
        assert_eq!(
            *outcomes.lock().unwrap(),
            vec![(
                0,
                ProviderAttemptOutcome::Usage(ConfirmedProviderUsage {
                    input_tokens: 2,
                    output_tokens: 3,
                    total_tokens: 5
                })
            )]
        );
    }

    async fn stream_case(drop_after_first: bool) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider = compatible(format!("http://{}", listener.local_addr().unwrap()));
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_single_http_request(&mut socket).await;
            reply(&mut socket, "200 OK", concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3}}\n\n",
                "data: [DONE]\n\n"
            )).await;
        });
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let outcomes = Arc::new(Mutex::new(Vec::new()));
        let policy = recording_policy(attempts.clone(), outcomes.clone(), 1);
        // Receipt observation must work after the task-local policy future ends.
        let mut stream = PROVIDER_PRIVATE_ENDPOINTS_ALLOWED
            .scope(
                true,
                policy.scope(provider.stream(
                    vec![ChatMessage {
                        role: "user".into(),
                        content: "synthetic".into(),
                        attachments: Vec::new(),
                    }],
                    None,
                    ToolMode::None,
                    None,
                    SamplingParams::default(),
                    CancellationToken::new(),
                )),
            )
            .await
            .unwrap();
        assert!(matches!(
            stream.next().await.unwrap().unwrap(),
            StreamChunk::TextDelta(_)
        ));
        if !drop_after_first {
            while let Some(chunk) = stream.next().await {
                chunk.unwrap();
            }
        }
        drop(stream);
        tokio::time::timeout(Duration::from_secs(3), server)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(attempts.lock().unwrap().len(), 1);
        assert_eq!(
            outcomes.lock().unwrap().len(),
            usize::from(!drop_after_first)
        );
    }

    #[tokio::test]
    async fn attempt_accounting_receipt_survives_stream_policy_scope_end() {
        stream_case(false).await;
    }

    #[tokio::test]
    async fn attempt_accounting_dropped_stream_never_refunds_unknown_usage() {
        stream_case(true).await;
    }

    #[tokio::test]
    async fn attempt_accounting_fallback_requires_another_admission() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider = responses(format!("http://{}", listener.local_addr().unwrap()));
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (_, _, body) = read_single_http_request(&mut socket).await;
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&body).unwrap()["max_output_tokens"],
                10
            );
            reply(
                &mut socket,
                "400 Bad Request",
                r#"{"detail":"Stream must be set to true"}"#,
            )
            .await;
            listener
        });
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let outcomes = Arc::new(Mutex::new(Vec::new()));
        let error = recording_policy(attempts.clone(), outcomes.clone(), 1)
            .scope(provider.complete("synthetic", None))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("budget exhausted"));
        let listener = tokio::time::timeout(Duration::from_secs(3), server)
            .await
            .unwrap()
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        assert_eq!(attempts.lock().unwrap().len(), 2);
        assert!(!attempts.lock().unwrap()[0].streaming);
        assert!(attempts.lock().unwrap()[1].streaming);
        assert!(
            outcomes.lock().unwrap().is_empty(),
            "HTTP error is not proof of zero charge"
        );
    }

    #[tokio::test]
    async fn attempt_accounting_revoked_after_admission_confirms_not_dispatched() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider = responses(format!("http://{}", listener.local_addr().unwrap()));
        let revoked = Arc::new(AtomicBool::new(false));
        let check = revoked.clone();
        let authority = ProviderDispatchAuthority::new(move || {
            let check = check.clone();
            async move {
                anyhow::ensure!(!check.load(Ordering::SeqCst), "revoked during admission");
                Ok(())
            }
        });
        let outcomes = Arc::new(Mutex::new(Vec::new()));
        let captured = outcomes.clone();
        let policy = ProviderAttemptPolicy::new(10, 4096, move |_| {
            revoked.store(true, Ordering::SeqCst);
            let captured = captured.clone();
            async move {
                Ok(ProviderAttemptReceipt::new(move |outcome| {
                    captured.lock().unwrap().push(outcome);
                    async { Ok(()) }
                }))
            }
        })
        .unwrap();
        let error = authority
            .scope(policy.scope(provider.complete("synthetic", None)))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("revoked"));
        assert_eq!(
            *outcomes.lock().unwrap(),
            vec![ProviderAttemptOutcome::NotDispatched]
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn attempt_accounting_unknown_adapter_is_rejected_before_legacy_dispatch() {
        let registry = ProviderRegistry::new(AppConfig::default());
        let policy = ProviderAttemptPolicy::new(10, 100, |_| async {
            panic!("unaccounted adapter cannot reach admission")
        })
        .unwrap();
        assert!(policy
            .scope(registry.complete_for_provider(Some("local"), "synthetic", None))
            .await
            .unwrap_err()
            .to_string()
            .contains("does not support"));
        assert!(registry
            .complete_for_provider(Some("local"), "synthetic", None)
            .await
            .is_ok());
    }

    #[test]
    fn attempt_accounting_missing_overflowing_or_extra_billable_units_stay_unknown() {
        use attempt_accounting::confirmed_usage;
        for value in [
            json!({}),
            json!({"usage":{}}),
            json!({"usage":{"prompt_tokens":0}}),
            json!({"usage":{"prompt_tokens":u64::MAX,"completion_tokens":1}}),
            json!({"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":1}}),
        ] {
            assert!(confirmed_usage(&value, ProviderProtocol::ChatCompletions).is_none());
        }
        assert_eq!(
            confirmed_usage(
                &json!({"usage":{"input_tokens":2,"cache_creation_input_tokens":4,
            "cache_read_input_tokens":5,"output_tokens":3}}),
                ProviderProtocol::Anthropic
            )
            .unwrap()
            .total_tokens,
            14
        );
        let cohere = json!({"usage":{"billed_units":{"input_tokens":2,"output_tokens":3},
            "tokens":{"input_tokens":7,"output_tokens":4}}});
        let usage = confirmed_usage(&cohere, ProviderProtocol::Cohere).unwrap();
        assert_eq!(
            (usage.input_tokens, usage.output_tokens, usage.total_tokens),
            (2, 3, 11)
        );
        let mut extra = cohere;
        extra["usage"]["billed_units"]["search_units"] = json!(1);
        assert!(confirmed_usage(&extra, ProviderProtocol::Cohere).is_none());
    }

    #[tokio::test]
    async fn attempt_accounting_stream_cannot_settle_incomplete_or_decreasing_usage() {
        for ending in [
            json!({"usage": {}}),
            json!({"usage": {"prompt_tokens": 1, "completion_tokens": 1}}),
        ] {
            let outcomes = Arc::new(Mutex::new(Vec::new()));
            let captured = outcomes.clone();
            let receipt = Some(ProviderAttemptReceipt::new(move |outcome| {
                captured.lock().unwrap().push(outcome);
                async { Ok(()) }
            }));
            let mut usage = attempt_accounting::StreamingUsage::default();
            usage.observe(
                &json!({"usage": {"prompt_tokens": 2, "completion_tokens": 3}}),
                ProviderProtocol::ChatCompletions,
            );
            usage.observe(&ending, ProviderProtocol::ChatCompletions);
            usage.confirm(&receipt).await.unwrap();
            assert!(outcomes.lock().unwrap().is_empty());
        }
    }
}
