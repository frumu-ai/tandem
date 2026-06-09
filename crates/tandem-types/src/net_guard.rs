//! Shared SSRF guard for outbound URL access.
//!
//! Runtime tools that fetch attacker-influenced URLs (web fetch, browser
//! navigation, connector callbacks) must not be able to reach loopback,
//! private, link-local, or otherwise internal addresses. Historically each
//! caller carried its own narrow guard; this module is the single shared
//! validator so every surface blocks the same address space.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

/// Reason an outbound URL was rejected by the SSRF guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SsrfBlockReason {
    /// The URL could not be parsed.
    InvalidUrl,
    /// The URL scheme is not an allowed outbound HTTP(S) scheme.
    UnsupportedScheme(String),
    /// The URL has no host component.
    MissingHost,
    /// The host resolves to a blocked (internal) address space.
    BlockedHost(String),
}

impl std::fmt::Display for SsrfBlockReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SsrfBlockReason::InvalidUrl => write!(f, "invalid url"),
            SsrfBlockReason::UnsupportedScheme(scheme) => {
                write!(
                    f,
                    "unsupported url scheme `{scheme}` (only http/https allowed)"
                )
            }
            SsrfBlockReason::MissingHost => write!(f, "url has no host"),
            SsrfBlockReason::BlockedHost(host) => {
                write!(f, "host `{host}` is blocked by network policy")
            }
        }
    }
}

impl std::error::Error for SsrfBlockReason {}

/// Returns true if an IPv4 address is in an internal/non-routable range that
/// outbound tools must not reach.
pub fn ipv4_is_ssrf_blocked(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        // 0.0.0.0/8 "this host" network.
        || octets[0] == 0
        // 100.64.0.0/10 carrier-grade NAT (shared address space).
        || (octets[0] == 100 && (octets[1] & 0xc0) == 64)
}

/// Returns true if an IPv6 address is in an internal/non-routable range, or is
/// an IPv4-mapped/compatible address pointing at a blocked IPv4 range.
pub fn ipv6_is_ssrf_blocked(ip: Ipv6Addr) -> bool {
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return ipv4_is_ssrf_blocked(mapped);
    }
    // `to_ipv4` also covers the deprecated IPv4-compatible range (::a.b.c.d).
    if let Some(compat) = ip.to_ipv4() {
        if ipv4_is_ssrf_blocked(compat) {
            return true;
        }
    }
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        // 2001:db8::/32 documentation range.
        || (ip.segments()[0] == 0x2001 && ip.segments()[1] == 0x0db8)
}

/// Returns true if an IP address must be blocked for SSRF safety.
pub fn ip_is_ssrf_blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => ipv4_is_ssrf_blocked(v4),
        IpAddr::V6(v6) => ipv6_is_ssrf_blocked(v6),
    }
}

/// Returns true if a host string (hostname or IP literal, optionally bracketed
/// for IPv6) must be blocked for SSRF safety.
///
/// Hostnames are only blocked when they are obviously local (`localhost` and
/// `*.localhost`) or when they are themselves IP literals. DNS resolution is
/// intentionally not performed here; callers that follow redirects should
/// re-check each resolved hop's host.
pub fn host_is_ssrf_blocked(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return true;
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    let ip_candidate = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = ip_candidate.parse::<IpAddr>() {
        return ip_is_ssrf_blocked(ip);
    }
    false
}

/// Returns the first resolved socket address whose IP is in a blocked
/// (internal) range, if any. Callers that resolve a hostname before connecting
/// should reject the request when this returns `Some`, which closes DNS
/// rebinding where a public name resolves to an internal address.
pub fn first_blocked_resolved_ip(addrs: &[std::net::SocketAddr]) -> Option<IpAddr> {
    addrs
        .iter()
        .map(|addr| addr.ip())
        .find(|ip| ip_is_ssrf_blocked(*ip))
}

/// Validate that a URL is a public HTTP(S) endpoint safe to fetch.
///
/// Returns the parsed [`Url`] on success, or an [`SsrfBlockReason`] describing
/// why it was rejected.
pub fn validate_public_http_url(raw: &str) -> Result<Url, SsrfBlockReason> {
    let parsed = Url::parse(raw.trim()).map_err(|_| SsrfBlockReason::InvalidUrl)?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(SsrfBlockReason::UnsupportedScheme(other.to_string())),
    }
    match parsed.host() {
        Some(Host::Ipv4(ip)) => {
            if ipv4_is_ssrf_blocked(ip) {
                return Err(SsrfBlockReason::BlockedHost(ip.to_string()));
            }
        }
        Some(Host::Ipv6(ip)) => {
            if ipv6_is_ssrf_blocked(ip) {
                return Err(SsrfBlockReason::BlockedHost(ip.to_string()));
            }
        }
        Some(Host::Domain(domain)) => {
            if host_is_ssrf_blocked(domain) {
                return Err(SsrfBlockReason::BlockedHost(domain.to_string()));
            }
        }
        None => return Err(SsrfBlockReason::MissingHost),
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_loopback_and_local_hosts() {
        assert!(host_is_ssrf_blocked("localhost"));
        assert!(host_is_ssrf_blocked("api.localhost"));
        assert!(host_is_ssrf_blocked("127.0.0.1"));
        assert!(host_is_ssrf_blocked("127.0.0.53"));
        assert!(host_is_ssrf_blocked("[::1]"));
        assert!(host_is_ssrf_blocked("::1"));
    }

    #[test]
    fn blocks_private_link_local_and_metadata_ranges() {
        assert!(host_is_ssrf_blocked("10.0.0.5"));
        assert!(host_is_ssrf_blocked("172.16.4.4"));
        assert!(host_is_ssrf_blocked("192.168.1.1"));
        // Cloud metadata endpoint (link-local).
        assert!(host_is_ssrf_blocked("169.254.169.254"));
        assert!(host_is_ssrf_blocked("0.0.0.0"));
        // Carrier-grade NAT shared range.
        assert!(host_is_ssrf_blocked("100.64.1.1"));
        // Unique-local and link-local IPv6.
        assert!(host_is_ssrf_blocked("fc00::1"));
        assert!(host_is_ssrf_blocked("fe80::1"));
    }

    #[test]
    fn blocks_ipv4_mapped_ipv6_loopback() {
        assert!(host_is_ssrf_blocked("::ffff:127.0.0.1"));
        assert!(host_is_ssrf_blocked("::ffff:10.0.0.1"));
    }

    #[test]
    fn allows_public_hosts() {
        assert!(!host_is_ssrf_blocked("example.com"));
        assert!(!host_is_ssrf_blocked("8.8.8.8"));
        assert!(!host_is_ssrf_blocked("93.184.216.34"));
        assert!(!host_is_ssrf_blocked("2606:2800:220:1:248:1893:25c8:1946"));
    }

    #[test]
    fn validate_url_rejects_non_http_schemes() {
        assert_eq!(
            validate_public_http_url("file:///etc/passwd"),
            Err(SsrfBlockReason::UnsupportedScheme("file".to_string()))
        );
        assert_eq!(
            validate_public_http_url("ftp://example.com/x"),
            Err(SsrfBlockReason::UnsupportedScheme("ftp".to_string()))
        );
    }

    #[test]
    fn validate_url_rejects_blocked_hosts_including_userinfo_trick() {
        assert!(matches!(
            validate_public_http_url("http://127.0.0.1/admin"),
            Err(SsrfBlockReason::BlockedHost(_))
        ));
        assert!(matches!(
            validate_public_http_url("http://169.254.169.254/latest/meta-data/"),
            Err(SsrfBlockReason::BlockedHost(_))
        ));
        // Userinfo must not smuggle a public-looking authority past the host check.
        assert!(matches!(
            validate_public_http_url("http://example.com@127.0.0.1/"),
            Err(SsrfBlockReason::BlockedHost(_))
        ));
        assert!(matches!(
            validate_public_http_url("http://[::1]:8080/"),
            Err(SsrfBlockReason::BlockedHost(_))
        ));
    }

    #[test]
    fn validate_url_allows_public_endpoints() {
        assert!(validate_public_http_url("https://example.com/path?q=1").is_ok());
        assert!(validate_public_http_url("http://93.184.216.34/").is_ok());
    }

    #[test]
    fn first_blocked_resolved_ip_flags_internal_addresses() {
        use std::net::SocketAddr;
        let public: SocketAddr = "93.184.216.34:443".parse().unwrap();
        let loopback: SocketAddr = "127.0.0.1:80".parse().unwrap();
        let metadata: SocketAddr = "169.254.169.254:80".parse().unwrap();

        assert_eq!(first_blocked_resolved_ip(&[public]), None);
        // A hostname that resolves to any internal address must be rejected.
        assert_eq!(
            first_blocked_resolved_ip(&[public, loopback]),
            Some(loopback.ip())
        );
        assert_eq!(first_blocked_resolved_ip(&[metadata]), Some(metadata.ip()));
    }

    #[test]
    fn validate_url_rejects_garbage() {
        assert_eq!(
            validate_public_http_url("not a url"),
            Err(SsrfBlockReason::InvalidUrl)
        );
    }
}
