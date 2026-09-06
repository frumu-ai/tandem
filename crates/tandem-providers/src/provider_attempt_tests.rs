#[cfg(test)]
mod provider_attempt_tests {
    use super::*;

    fn responses(base_url: String) -> OpenAIResponsesProvider {
        OpenAIResponsesProvider {
            id: "openai-codex".into(),
            name: "Synthetic Responses".into(),
            base_url,
            api_key: None,
            default_model: "synthetic-model".into(),
            models: Vec::new(),
            client: dispatch_authority::provider_client(),
        }
    }

    fn compatible(base_url: String) -> OpenAICompatibleProvider {
        OpenAICompatibleProvider {
            id: "ollama".into(),
            name: "Synthetic local transport".into(),
            base_url,
            api_key: None,
            default_model: "synthetic-model".into(),
        }
    }

    async fn invoke(provider: &dyn Provider, streaming: bool) -> anyhow::Result<String> {
        if streaming {
            let mut stream = provider
                .stream(
                    vec![ChatMessage {
                        role: "user".into(),
                        content: "synthetic request".into(),
                        attachments: Vec::new(),
                    }],
                    None,
                    ToolMode::None,
                    None,
                    SamplingParams::default(),
                    CancellationToken::new(),
                )
                .await?;
            while let Some(chunk) = stream.next().await {
                chunk?;
            }
            Ok(String::new())
        } else {
            provider.complete("synthetic request", None).await
        }
    }

    fn membership_guard(revoked: Arc<AtomicBool>) -> ProviderDispatchAuthority {
        ProviderDispatchAuthority::new(move || {
            let revoked = revoked.clone();
            async move {
                anyhow::ensure!(
                    !revoked.load(Ordering::SeqCst),
                    "synthetic membership revoked"
                );
                Ok(())
            }
        })
    }

    async fn reply(socket: &mut tokio::net::TcpStream, status: &str, body: &str) {
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.shutdown().await.unwrap();
    }

    async fn fallback_case(revoke: bool) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider = responses(format!("http://{}", listener.local_addr().unwrap()));
        let revoked = Arc::new(AtomicBool::new(false));
        let authority = membership_guard(revoked.clone());
        let requests = Arc::new(AtomicUsize::new(0));
        let observed = requests.clone();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let first = read_single_http_request(&mut socket).await;
            observed.fetch_add(1, Ordering::SeqCst);
            assert!(first.2.contains("\"stream\":false"));
            revoked.store(revoke, Ordering::SeqCst);
            reply(
                &mut socket,
                "400 Bad Request",
                r#"{"detail":"Stream must be set to true"}"#,
            )
            .await;
            if !revoke {
                let (mut socket, _) = listener.accept().await.unwrap();
                let second = read_single_http_request(&mut socket).await;
                observed.fetch_add(1, Ordering::SeqCst);
                assert!(second.2.contains("\"stream\":true"));
                reply(
                    &mut socket,
                    "200 OK",
                    "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Recovered\"}\n\ndata: [DONE]\n\n",
                )
                .await;
            }
            // Keep the listener alive until the caller finishes, so an extra
            // request cannot be disguised as connection-refused retry behavior.
            listener
        });
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            authority.scope(provider.complete("synthetic request", None)),
        )
        .await
        .expect("fallback must finish without another unauthorized send");
        let listener = tokio::time::timeout(Duration::from_secs(3), server)
            .await
            .expect("synthetic fallback server must finish")
            .unwrap();
        if revoke {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("membership revoked"));
            assert_eq!(requests.load(Ordering::SeqCst), 1);
        } else {
            assert_eq!(result.unwrap(), "Recovered");
            assert_eq!(requests.load(Ordering::SeqCst), 2);
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn hosted_policy_adapter_fallback_rechecks_revoked_membership() {
        fallback_case(true).await;
    }

    #[tokio::test]
    async fn hosted_policy_adapter_fallback_preserves_permitted_requests() {
        fallback_case(false).await;
    }

    #[tokio::test]
    async fn hosted_policy_adapter_transport_retries_recheck_authority() {
        // Refused local connections enter each real adapter's retry loop.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let providers: Vec<Box<dyn Provider>> =
            vec![Box::new(compatible(url.clone())), Box::new(responses(url))];
        for provider in providers {
            for streaming in [false, true] {
                let checks = Arc::new(AtomicUsize::new(0));
                let count = checks.clone();
                let authority = ProviderDispatchAuthority::new(move || {
                    let count = count.clone();
                    async move {
                        anyhow::ensure!(
                            count.fetch_add(1, Ordering::SeqCst) == 0,
                            "synthetic membership revoked before retry"
                        );
                        Ok(())
                    }
                });
                let result = tokio::time::timeout(
                    Duration::from_secs(5),
                    PROVIDER_PRIVATE_ENDPOINTS_ALLOWED
                        .scope(true, authority.scope(invoke(provider.as_ref(), streaming))),
                )
                .await
                .expect("retry check must terminate");
                assert!(result
                    .unwrap_err()
                    .to_string()
                    .contains("membership revoked"));
                assert_eq!(checks.load(Ordering::SeqCst), 2);
            }
        }
    }

    #[tokio::test]
    async fn hosted_policy_adapter_denial_prevents_first_network_request() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let providers: Vec<Box<dyn Provider>> = vec![
            Box::new(compatible(url.clone())),
            Box::new(responses(url.clone())),
            // A local proxy prevents any accidental Internet call if the
            // Anthropic send barrier regresses, including its fixed HTTPS URL.
            Box::new(AnthropicProvider {
                api_key: None,
                default_model: "synthetic-model".into(),
                client: Client::builder()
                    .proxy(reqwest::Proxy::all(&url).unwrap())
                    .build()
                    .unwrap(),
            }),
        ];
        for provider in providers {
            for streaming in [false, true] {
                let authority = membership_guard(Arc::new(AtomicBool::new(true)));
                let result = tokio::time::timeout(
                    Duration::from_secs(3),
                    PROVIDER_PRIVATE_ENDPOINTS_ALLOWED
                        .scope(true, authority.scope(invoke(provider.as_ref(), streaming))),
                )
                .await
                .expect("denied request must terminate before transport");
                assert!(result
                    .unwrap_err()
                    .to_string()
                    .contains("membership revoked"));
            }
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn hosted_policy_adapter_redirect_cannot_bypass_dispatch_boundary() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // The production Codex constructor deliberately ignores configured URL
        // overrides. Use the existing fixture replacement with the production
        // client factory, never a config field that could reach the Internet.
        let registry = ProviderRegistry::new(cfg(&[], None, false));
        registry
            .replace_for_test(
                vec![Arc::new(responses(format!("http://{addr}")))],
                Some("openai-codex".into()),
            )
            .await;
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_single_http_request(&mut socket).await;
            socket
                .write_all(
                    format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{addr}/redirected\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes(),
                )
                .await
                .unwrap();
            socket.shutdown().await.unwrap();
            listener
        });
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            membership_guard(Arc::new(AtomicBool::new(false))).scope(
                registry.complete_for_provider(Some("openai-codex"), "synthetic request", None),
            ),
        )
        .await
        .expect("redirect must return without following Location");
        assert!(result.is_err());
        let listener = tokio::time::timeout(Duration::from_secs(3), server)
            .await
            .expect("synthetic redirect server must finish")
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    }
}
