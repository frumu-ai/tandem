// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Shared fail-closed policy for server-initiated HTTPS requests.
//!
//! Callers resolve and validate a destination immediately before building the
//! client. DNS answers are pinned into that client, redirects are disabled,
//! and response bodies must be consumed through an explicit byte limit.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use anyhow::Context;
use reqwest::{redirect::Policy as RedirectPolicy, Client, Url};

#[derive(Debug, Clone)]
pub struct ResolvedPublicHttpsTarget {
    url: Url,
    dns_override_host: Option<String>,
    dns_override_addrs: Vec<SocketAddr>,
}

impl ResolvedPublicHttpsTarget {
    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn client(&self, timeout: Duration) -> anyhow::Result<Client> {
        let mut builder = Client::builder()
            .no_proxy()
            .redirect(RedirectPolicy::none())
            .timeout(timeout);
        if let Some(host) = self.dns_override_host.as_deref() {
            builder = builder.resolve_to_addrs(host, &self.dns_override_addrs);
        }
        builder.build().context("build hardened outbound client")
    }
}

pub async fn resolve_public_https_url(raw: &str) -> anyhow::Result<ResolvedPublicHttpsTarget> {
    resolve_outbound_url(raw, false).await
}

/// Standalone-local provider discovery intentionally supports private HTTP
/// endpoints such as a local llama.cpp server. Hosted callers must use the
/// public-HTTPS resolver above.
pub async fn resolve_standalone_provider_url(
    raw: &str,
) -> anyhow::Result<ResolvedPublicHttpsTarget> {
    resolve_outbound_url(raw, true).await
}

async fn resolve_outbound_url(
    raw: &str,
    allow_private_provider_endpoint: bool,
) -> anyhow::Result<ResolvedPublicHttpsTarget> {
    resolve_outbound_url_with_resolver(
        raw,
        allow_private_provider_endpoint,
        |host, port| async move {
            Ok(tokio::net::lookup_host((host.as_str(), port))
                .await?
                .collect::<Vec<_>>())
        },
    )
    .await
}

async fn resolve_outbound_url_with_resolver<F, Fut>(
    raw: &str,
    allow_private_provider_endpoint: bool,
    resolver: F,
) -> anyhow::Result<ResolvedPublicHttpsTarget>
where
    F: FnOnce(String, u16) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Vec<SocketAddr>>>,
{
    let url = Url::parse(raw).context("parse outbound URL")?;
    let insecure_http = url.scheme() == "http";
    if url.scheme() != "https" && !(insecure_http && allow_private_provider_endpoint) {
        anyhow::bail!("outbound URL must use https");
    }
    if !url.username().is_empty() || url.password().is_some() {
        anyhow::bail!("outbound URL must not include credentials");
    }
    let host = url
        .host()
        .ok_or_else(|| anyhow::anyhow!("outbound URL host is missing"))?
        .to_owned();
    let port = url.port_or_known_default().unwrap_or(443);
    match host {
        url::Host::Ipv4(ip) => {
            let public = ipv4_is_publicly_routable(ip);
            if !public && !allow_private_provider_endpoint {
                anyhow::bail!("outbound URL resolves to a private or internal address");
            }
            if insecure_http && public {
                anyhow::bail!("insecure provider HTTP is limited to private standalone endpoints");
            }
            Ok(ResolvedPublicHttpsTarget {
                url,
                dns_override_host: None,
                dns_override_addrs: Vec::new(),
            })
        }
        url::Host::Ipv6(ip) => {
            let public = ipv6_is_publicly_routable(ip);
            if !public && !allow_private_provider_endpoint {
                anyhow::bail!("outbound URL resolves to a private or internal address");
            }
            if insecure_http && public {
                anyhow::bail!("insecure provider HTTP is limited to private standalone endpoints");
            }
            Ok(ResolvedPublicHttpsTarget {
                url,
                dns_override_host: None,
                dns_override_addrs: Vec::new(),
            })
        }
        url::Host::Domain(host) => {
            let normalized = host.trim().trim_end_matches('.').to_ascii_lowercase();
            if !allow_private_provider_endpoint
                && (normalized == "localhost" || normalized.ends_with(".localhost"))
            {
                anyhow::bail!("outbound URL points to localhost/private network");
            }
            let addrs = resolver(host.clone(), port)
                .await
                .context("resolve outbound destination host")?;
            if addrs.is_empty() {
                anyhow::bail!("outbound destination host did not resolve");
            }
            validate_resolved_addresses(&addrs, allow_private_provider_endpoint, insecure_http)?;
            Ok(ResolvedPublicHttpsTarget {
                url,
                dns_override_host: Some(host),
                dns_override_addrs: addrs,
            })
        }
    }
}

fn validate_resolved_addresses(
    addrs: &[SocketAddr],
    allow_private_provider_endpoint: bool,
    insecure_http: bool,
) -> anyhow::Result<()> {
    let any_private = addrs.iter().any(|addr| !ip_is_publicly_routable(addr.ip()));
    let any_public = addrs.iter().any(|addr| ip_is_publicly_routable(addr.ip()));
    if any_private && !allow_private_provider_endpoint {
        anyhow::bail!("outbound URL resolves to a private or internal address");
    }
    if insecure_http && any_public {
        anyhow::bail!("insecure provider HTTP is limited to private standalone endpoints");
    }
    Ok(())
}

pub async fn read_response_body_limited(
    mut response: reqwest::Response,
    limit: usize,
) -> anyhow::Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        anyhow::bail!("outbound response exceeds {limit} bytes");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.context("read outbound response")? {
        if body.len().saturating_add(chunk.len()) > limit {
            anyhow::bail!("outbound response exceeds {limit} bytes");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn ip_is_publicly_routable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ipv4_is_publicly_routable(ip),
        IpAddr::V6(ip) => ipv6_is_publicly_routable(ip),
    }
}

fn ipv4_is_publicly_routable(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 169 && octets[1] == 254)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
        || (octets[0] == 198 && (18..=19).contains(&octets[1]))
        || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
        || octets[0] >= 240)
}

fn ipv6_is_publicly_routable(ip: Ipv6Addr) -> bool {
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return ipv4_is_publicly_routable(mapped);
    }
    let segments = ip.segments();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ((segments[0] & 0xfe00) == 0xfc00)
        || ((segments[0] & 0xffc0) == 0xfe80)
        || ((segments[0] & 0xffc0) == 0xfec0)
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        || (segments[0] == 0x0100 && segments[1..4] == [0, 0, 0]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, Bytes};
    use axum::http::{header, StatusCode};
    use axum::response::Response;
    use axum::routing::get;
    use axum::Router;

    #[tokio::test]
    async fn rejects_private_literal_and_url_credentials() {
        for url in [
            "https://127.0.0.1/archive.zip",
            "https://[::1]/archive.zip",
            "https://user:password@example.com/archive.zip",
            "http://example.com/archive.zip",
            "https://240.0.0.1/archive.zip",
            "https://[2001:db8::1]/archive.zip",
        ] {
            assert!(resolve_public_https_url(url).await.is_err(), "{url}");
        }
    }

    #[tokio::test]
    async fn fake_dns_and_cloud_metadata_answers_fail_closed() {
        let public: SocketAddr = "8.8.8.8:443".parse().expect("public address");
        let private: SocketAddr = "10.0.0.7:443".parse().expect("private address");
        let metadata: SocketAddr = "169.254.169.254:443".parse().expect("metadata address");

        let public_target = resolve_outbound_url_with_resolver(
            "https://public.fixture.invalid/archive",
            false,
            move |host, port| async move {
                assert_eq!(host, "public.fixture.invalid");
                assert_eq!(port, 443);
                Ok(vec![public])
            },
        )
        .await
        .expect("public fake DNS answer");
        assert_eq!(public_target.dns_override_addrs, vec![public]);
        public_target
            .client(Duration::from_secs(2))
            .expect("public fake DNS answer builds pinned client");

        for answers in [vec![metadata], vec![public, private]] {
            assert!(resolve_outbound_url_with_resolver(
                "https://blocked.fixture.invalid/archive",
                false,
                move |_, _| async move { Ok(answers) },
            )
            .await
            .is_err());
        }

        assert!(resolve_outbound_url_with_resolver(
            "http://public.fixture.invalid/archive",
            true,
            move |_, _| async move { Ok(vec![public]) },
        )
        .await
        .is_err());

        let private_target = resolve_outbound_url_with_resolver(
            "http://private.fixture.invalid/archive",
            true,
            move |_, _| async move { Ok(vec![private]) },
        )
        .await
        .expect("explicit standalone private endpoint allowance");
        private_target
            .client(Duration::from_secs(2))
            .expect("private fake DNS answer builds pinned client");
    }

    #[tokio::test]
    async fn fake_redirect_and_streaming_budget_service_is_contained() {
        async fn redirect_to_metadata() -> Response<Body> {
            Response::builder()
                .status(StatusCode::TEMPORARY_REDIRECT)
                .header(header::LOCATION, "http://169.254.169.254/latest/meta-data/")
                .body(Body::empty())
                .expect("redirect response")
        }
        async fn unbounded_stream() -> Response<Body> {
            Response::new(Body::from_stream(futures::stream::iter([
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"12345678")),
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"overflow")),
            ])))
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake egress service");
        let address = listener.local_addr().expect("fake service address");
        let app = Router::new()
            .route("/redirect", get(redirect_to_metadata))
            .route("/stream", get(unbounded_stream));
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("fake egress server");
        });
        let target = resolve_outbound_url_with_resolver(
            &format!("http://fixture.invalid:{}/redirect", address.port()),
            true,
            move |host, port| async move {
                assert_eq!(host, "fixture.invalid");
                assert_eq!(port, address.port());
                Ok(vec![address])
            },
        )
        .await
        .expect("fake DNS-pinned target");
        let client = target
            .client(Duration::from_secs(2))
            .expect("hardened client");
        let redirect = client
            .get(target.url().clone())
            .send()
            .await
            .expect("redirect response");
        assert_eq!(redirect.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(
            redirect
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("http://169.254.169.254/latest/meta-data/")
        );

        let stream = client
            .get(target.url().join("/stream").expect("stream URL"))
            .send()
            .await
            .expect("stream response");
        assert!(read_response_body_limited(stream, 8).await.is_err());
        server.abort();
    }
}
