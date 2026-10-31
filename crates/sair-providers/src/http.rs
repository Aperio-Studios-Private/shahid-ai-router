//! SSRF enforcement for outbound provider calls (docs/06; review finding
//! F-3, ADR-015).
//!
//! Mechanism: a validating custom resolver — resolution and validation
//! happen together, and only validated socket addresses are returned, so
//! reqwest connects exactly to validated addresses while TLS SNI and
//! certificate identity remain bound to the original hostname (an IP-rewrite
//! connector would break verification; weakening it is forbidden).
//!
//! Policy: HTTPS by default; HTTP only for explicit loopback literals; all
//! other schemes rejected. Denylist is default-deny: an address is allowed
//! only when it is unambiguously public unicast. Redirects never cross a
//! host or scheme change.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SsrfError {
    #[error("destination address is not permitted: {0}")]
    AddressBlocked(IpAddr),
    #[error("URL scheme is not permitted: {0}")]
    SchemeBlocked(String),
    #[error("redirect would cross a host or scheme change and was stopped")]
    RedirectCrossedOrigin,
    #[error("address resolution failed: {0}")]
    Resolution(String),
}

/// Reads a response body incrementally with a hard byte cap. The previous
/// `bytes().await` + post-hoc length check buffered the whole body BEFORE
/// consulting the cap, so a hostile upstream could exhaust memory (audit P1
/// fix). Chunked reads stop as soon as the cap is exceeded.
///
/// # Errors
/// [`BodyLimitError::TooLarge`] when the cap is exceeded;
/// [`BodyLimitError::Transport`] on network failures (classify as usual).
pub async fn read_body_bounded(
    mut response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, BodyLimitError> {
    let mut out = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(BodyLimitError::Transport)? {
        if out.len().saturating_add(chunk.len()) > max_bytes {
            return Err(BodyLimitError::TooLarge { limit: max_bytes });
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// Failure modes of [`read_body_bounded`].
#[derive(Debug, Error)]
pub enum BodyLimitError {
    #[error("upstream body exceeded {limit} bytes")]
    TooLarge { limit: usize },
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
}

/// Parses the standard `Retry-After` header value: seconds form or HTTP-date
/// (RFC 7231 IMF-fixdate, previously ignored — audit P2 fix). Result is
/// capped at `max_secs` so a hostile provider cannot freeze credentials
/// long-term.
#[must_use]
pub fn parse_retry_after(value: &str, max_secs: u64) -> Option<u64> {
    let v = value.trim();
    if let Ok(secs) = v.parse::<u64>() {
        return Some(secs.min(max_secs));
    }
    // "Sun, 06 Nov 1994 08:49:37 GMT" — parse the fixed IMF-fixdate shape
    // (the trailing " GMT" is implied by the fixed format).
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(v, "%a, %d %b %Y %H:%M:%S") {
        let delta = dt.and_utc().timestamp() - chrono::Utc::now().timestamp();
        return Some((delta.max(0) as u64).min(max_secs));
    }
    None
}

/// SSRF policy applied to every outbound provider connection.
#[derive(Debug, Clone, Default)]
pub struct SsrfPolicy {
    /// Explicit opt-in (local development providers such as LM Studio /
    /// Ollama on the same machine). When false, loopback targets are blocked
    /// like everything else private.
    pub allow_loopback: bool,
}

impl SsrfPolicy {
    #[must_use]
    pub fn allow_loopback() -> Self {
        Self {
            allow_loopback: true,
        }
    }

    /// Validates the URL scheme (docs/06: HTTPS by default; HTTP only for
    /// explicit loopback literals; non-HTTP schemes rejected).
    ///
    /// # Errors
    /// [`SsrfError::SchemeBlocked`] for disallowed schemes.
    pub fn validate_url(&self, url: &url::Url) -> Result<(), SsrfError> {
        match url.scheme() {
            "https" => Ok(()),
            "http" => {
                let host = url
                    .host_str()
                    .ok_or_else(|| SsrfError::SchemeBlocked("missing host".into()))?;
                let is_loopback_literal = host
                    .parse::<IpAddr>()
                    .ok()
                    .is_some_and(|ip| ip.is_loopback())
                    || host.eq_ignore_ascii_case("localhost");
                if self.allow_loopback && is_loopback_literal {
                    Ok(())
                } else {
                    Err(SsrfError::SchemeBlocked(
                        "http is only allowed for explicit loopback literals".into(),
                    ))
                }
            }
            other => Err(SsrfError::SchemeBlocked(other.to_string())),
        }
    }

    /// Default-deny address check: allowed only when unambiguously public.
    #[must_use]
    pub fn is_allowed(&self, ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(v4) => self.is_allowed_v4(v4),
            IpAddr::V6(v6) => self.is_allowed_v6(v6),
        }
    }

    fn is_allowed_v4(&self, ip: Ipv4Addr) -> bool {
        if self.allow_loopback && ip.is_loopback() {
            return true;
        }
        let o = ip.octets();
        let blocked = matches!(
            (o[0], o[1]),
            (0, _)
                | (10, _)
                | (100, 64..=127)
                | (127, _)
                | (169, 254)
                | (172, 16..=31)
                | (192, 0)
                | (192, 168)
                | (198, 18..=19)
                | (203, 0)
                | (224..=255, _)
        ) || (o[0] == 198 && o[1] == 51 && o[2] == 100)
            || (o[0] == 203 && o[1] == 0 && o[2] == 113)
            || ip.is_broadcast();
        !blocked
    }

    fn is_allowed_v6(&self, ip: Ipv6Addr) -> bool {
        // IPv4-mapped addresses are judged by their IPv4 rules.
        if let Some(mapped) = ip.to_ipv4_mapped() {
            return self.is_allowed_v4(mapped);
        }
        if self.allow_loopback && ip.is_loopback() {
            return true;
        }
        // Global unicast space only, minus transition/keep ranges.
        let seg = ip.segments();
        let in_global_unicast = (0x2000..=0x3fff).contains(&seg[0]);
        if !in_global_unicast {
            return false;
        }
        // Exclude Teredo (2001::/32), documentation (2001:db8::/32),
        // 6to4 (2002::/16). NAT64 (64:ff9b::/96) is outside 2000::/3.
        let is_teredo = seg[0] == 0x2001 && seg[1] == 0x0000;
        let is_doc = seg[0] == 0x2001 && seg[1] == 0x0db8;
        let is_6to4 = seg[0] == 0x2002;
        !(is_teredo || is_doc || is_6to4)
    }

    /// Redirect guard: redirects may not cross a host or scheme change.
    #[must_use]
    pub fn redirect_allowed(
        &self,
        prev_host: &str,
        prev_scheme: &str,
        next_host: &str,
        next_scheme: &str,
    ) -> bool {
        prev_host.eq_ignore_ascii_case(next_host) && prev_scheme.eq_ignore_ascii_case(next_scheme)
    }
}

/// Source of candidate addresses for a host. Abstraction exists so tests can
/// inject DNS answers offline (review finding F-3).
pub trait AddressSource: Send + Sync {
    /// # Errors
    /// Resolution failures.
    fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>>;
}

/// Production source: the system resolver via `ToSocketAddrs`.
#[derive(Default)]
pub struct SystemResolver;

impl AddressSource for SystemResolver {
    fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
        use std::net::ToSocketAddrs;
        Ok((host, port).to_socket_addrs()?.collect())
    }
}

/// Validating resolver handed to reqwest: resolution + validation together
/// (ADR-015). Literal IPs never hit the inner resolver. Cheaply cloneable so
/// the Resolve future can own a handle for `spawn_blocking`.
#[derive(Clone)]
pub struct ValidatingResolver {
    policy: SsrfPolicy,
    source: Arc<dyn AddressSource>,
}

impl ValidatingResolver {
    #[must_use]
    pub fn new(policy: SsrfPolicy) -> Self {
        Self {
            policy,
            source: Arc::new(SystemResolver),
        }
    }

    #[must_use]
    pub fn with_source(policy: SsrfPolicy, source: Arc<dyn AddressSource>) -> Self {
        Self { policy, source }
    }

    /// Validates one host:port pair, returning only permitted addresses.
    ///
    /// # Errors
    /// [`SsrfError::AddressBlocked`] when every resolved address is blocked;
    /// [`SsrfError::Resolution`] when the source fails.
    pub fn validate_host(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, SsrfError> {
        // Literal IPs bypass DNS entirely but still go through the policy.
        if let Ok(ip) = host.parse::<IpAddr>() {
            if self.policy.is_allowed(ip) {
                return Ok(vec![SocketAddr::new(ip, port)]);
            }
            return Err(SsrfError::AddressBlocked(ip));
        }
        let is_localhost_name = host.eq_ignore_ascii_case("localhost");
        let addrs = self
            .source
            .resolve(host, port)
            .map_err(|e| SsrfError::Resolution(e.to_string()))?;
        let allowed: Vec<SocketAddr> = addrs
            .into_iter()
            .filter(|addr| {
                if is_localhost_name {
                    // Cleartext HTTP is allowed for `localhost` by name only
                    // because of the loopback exception; a public address for
                    // that name must not inherit the exception (audit P2 fix).
                    addr.ip().is_loopback() && self.policy.is_allowed(addr.ip())
                } else {
                    self.policy.is_allowed(addr.ip())
                }
            })
            .collect();
        if allowed.is_empty() {
            return Err(SsrfError::AddressBlocked(
                // When everything is filtered we do not know the specific
                // addresses the caller would have used; 0.0.0.0 marks
                // "unknown blocked destination" in diagnostics.
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            ));
        }
        Ok(allowed)
    }
}

impl reqwest::dns::Resolve for ValidatingResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        // reqwest invokes custom resolvers with the hostname only; the port
        // comes from the request URL (doc: explicit URL port overrides the
        // resolved SocketAddr port), so 0 is a placeholder here.
        //
        // getaddrinfo is blocking; keep it off the tokio worker threads via
        // spawn_blocking (audit P2 fix — the previous ready-future ran the
        // blocking call inline on the polling task).
        let host = name.as_str().to_string();
        let resolver = self.clone();
        Box::pin(async move {
            let result = tokio::task::spawn_blocking(move || resolver.validate_host(&host, 0))
                .await
                .map_err(|e| {
                    Box::new(io::Error::other(e.to_string()))
                        as Box<dyn std::error::Error + Send + Sync>
                })?;
            result
                .map(|addrs| Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        })
    }
}

/// Builds an outbound client with the validating resolver and the
/// no-cross-origin redirect policy.
///
/// Returns [`SsrfCheckedClient`], not the raw client: hyper-util
/// short-circuits IP-literal authorities before invoking the custom
/// resolver, so literal and scheme enforcement must happen per request at
/// this boundary (security review finding SEC2-01). Adapters never receive
/// the raw client.
#[must_use]
pub fn build_client(policy: SsrfPolicy) -> SsrfCheckedClient {
    build_client_with_timeout(policy, std::time::Duration::from_secs(120))
}

/// As [`build_client`], with an explicit total-request timeout budget.
#[must_use]
pub fn build_client_with_timeout(
    policy: SsrfPolicy,
    timeout: std::time::Duration,
) -> SsrfCheckedClient {
    let redirect_policy = policy.clone();
    let inner = reqwest::Client::builder()
        .dns_resolver(Arc::new(ValidatingResolver::new(policy.clone())))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            let max_hops = 3;
            if attempt.previous().len() >= max_hops {
                return attempt.error("too many redirects");
            }
            let Some(prev) = attempt.previous().last() else {
                return attempt.follow();
            };
            let prev_host = prev.host_str().unwrap_or_default();
            let next_host = attempt.url().host_str().unwrap_or_default();
            if redirect_policy.redirect_allowed(
                prev_host,
                prev.scheme(),
                next_host,
                attempt.url().scheme(),
            ) {
                attempt.follow()
            } else {
                // Returns the redirect response to the caller; adapters
                // surface it as a classified error.
                attempt.stop()
            }
        }))
        .timeout(timeout)
        .build()
        .expect("outbound client must build; static policy and resolver only");
    SsrfCheckedClient {
        client: inner,
        policy,
    }
}

/// A reqwest client that enforces the [`SsrfPolicy`] on every target URL
/// before a request is built: scheme rules (HTTPS default; HTTP only for
/// explicit loopback literals) and IP-literal denylist checks. Hostname
/// targets are additionally validated per resolved address by the
/// [`ValidatingResolver`] inside the connection layer.
#[derive(Clone)]
pub struct SsrfCheckedClient {
    client: reqwest::Client,
    policy: SsrfPolicy,
}

impl SsrfCheckedClient {
    /// Validates the target and returns a request builder.
    ///
    /// # Errors
    /// [`SsrfError::SchemeBlocked`] for disallowed schemes,
    /// [`SsrfError::AddressBlocked`] for blocked IP-literal hosts.
    pub fn request(
        &self,
        method: reqwest::Method,
        url: url::Url,
    ) -> Result<reqwest::RequestBuilder, SsrfError> {
        self.policy.validate_url(&url)?;
        if let Some(host) = url.host_str()
            && let Ok(ip) = host.parse::<IpAddr>()
            && !self.policy.is_allowed(ip)
        {
            return Err(SsrfError::AddressBlocked(ip));
        }
        Ok(self.client.request(method, url))
    }
}

/// As [`build_client_with_timeout`], but for STREAMING: no total deadline
/// (Phase 7 review F-1 — a total timeout would truncate a long-lived SSE
/// stream after commitment). Bounded by `connect_timeout` (establishment)
/// and `read_timeout` (idle between reads, client-level in reqwest 0.13).
#[must_use]
pub fn build_streaming_client(policy: SsrfPolicy) -> SsrfCheckedClient {
    build_streaming_client_with_idle_timeout(policy, std::time::Duration::from_secs(300))
}

/// As [`build_streaming_client`], with an explicit idle-read timeout for
/// tests of stalled upstreams (Phase 11).
#[must_use]
pub fn build_streaming_client_with_idle_timeout(
    policy: SsrfPolicy,
    idle_read: std::time::Duration,
) -> SsrfCheckedClient {
    let redirect_policy = policy.clone();
    let inner = reqwest::Client::builder()
        .dns_resolver(Arc::new(ValidatingResolver::new(policy.clone())))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 3 {
                return attempt.error("too many redirects");
            }
            let Some(prev) = attempt.previous().last() else {
                return attempt.follow();
            };
            if redirect_policy.redirect_allowed(
                prev.host_str().unwrap_or_default(),
                prev.scheme(),
                attempt.url().host_str().unwrap_or_default(),
                attempt.url().scheme(),
            ) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(idle_read)
        .build()
        .expect("streaming client must build; static policy and resolver only");
    SsrfCheckedClient {
        client: inner,
        policy,
    }
}

#[cfg(test)]
mod tests {
    use super::{AddressSource, SsrfError, SsrfPolicy, ValidatingResolver, build_client};
    use std::net::{IpAddr, SocketAddr};
    use std::sync::Arc;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn denylist_matrix_blocks_private_and_transition_ranges() {
        let policy = SsrfPolicy::default();
        for blocked in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254", // cloud metadata
            "0.0.0.0",
            "0.1.2.3",
            "100.64.0.1",      // CGNAT
            "100.127.255.255", // CGNAT top
            "224.0.0.1",       // multicast
            "239.255.255.255", // multicast
            "240.0.0.1",       // reserved
            "255.255.255.255", // broadcast
            "192.0.2.1",       // TEST-NET-1
            "198.51.100.7",    // TEST-NET-2
            "203.0.113.9",     // TEST-NET-3
            "198.18.0.1",      // benchmarking
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd12:3456::1",           // ULA
            "ff02::1",                // multicast
            "::ffff:10.0.0.5",        // v4-mapped private
            "::ffff:169.254.169.254", // v4-mapped metadata
            "64:ff9b::7f00:1",        // NAT64 wrapping 127.0.0.1
            "2002:7f00:1::",          // 6to4 wrapping 127.0.0.1
            "2001:0000:4136:e378::1", // Teredo
            "2001:db8::1",            // documentation
        ] {
            assert!(!policy.is_allowed(ip(blocked)), "{blocked} must be blocked");
        }
    }

    #[test]
    fn public_addresses_are_allowed() {
        let policy = SsrfPolicy::default();
        for allowed in [
            "8.8.8.8",
            "1.1.1.1",
            "93.184.216.34",
            "2600::1",
            "2606:4700::1111",
        ] {
            assert!(policy.is_allowed(ip(allowed)), "{allowed} must be allowed");
        }
    }

    #[test]
    fn loopback_exception_is_explicit() {
        let strict = SsrfPolicy::default();
        assert!(!strict.is_allowed(ip("127.0.0.1")));
        assert!(!strict.is_allowed(ip("::1")));

        let relaxed = SsrfPolicy::allow_loopback();
        assert!(relaxed.is_allowed(ip("127.0.0.1")));
        assert!(relaxed.is_allowed(ip("::1")));
        // Other private ranges stay blocked even with the exception.
        assert!(!relaxed.is_allowed(ip("10.0.0.1")));
        assert!(!relaxed.is_allowed(ip("192.168.1.1")));
    }

    #[test]
    fn scheme_policy_requires_https_unless_loopback_optin() {
        let strict = SsrfPolicy::default();
        assert!(
            strict
                .validate_url(&url::Url::parse("https://api.example.com/v1").unwrap())
                .is_ok()
        );
        assert!(matches!(
            strict.validate_url(&url::Url::parse("http://api.example.com/v1").unwrap()),
            Err(SsrfError::SchemeBlocked(_))
        ));
        assert!(matches!(
            strict.validate_url(&url::Url::parse("ftp://api.example.com").unwrap()),
            Err(SsrfError::SchemeBlocked(_))
        ));

        let relaxed = SsrfPolicy::allow_loopback();
        assert!(
            relaxed
                .validate_url(&url::Url::parse("http://127.0.0.1:8080/v1").unwrap())
                .is_ok()
        );
        assert!(
            relaxed
                .validate_url(&url::Url::parse("http://localhost:11434/v1").unwrap())
                .is_ok()
        );
        // The exception does not extend to remote hosts.
        assert!(matches!(
            relaxed.validate_url(&url::Url::parse("http://api.example.com/v1").unwrap()),
            Err(SsrfError::SchemeBlocked(_))
        ));
    }

    #[test]
    fn redirects_may_not_cross_host_or_scheme() {
        let policy = SsrfPolicy::default();
        assert!(policy.redirect_allowed("api.example.com", "https", "api.example.com", "https"));
        assert!(!policy.redirect_allowed("api.example.com", "https", "evil.example.net", "https"));
        assert!(!policy.redirect_allowed("api.example.com", "https", "api.example.com", "http"));
    }

    /// Offline rebinding-shaped test via injected address source (F-3):
    /// only validated addresses may ever reach the connect call.
    #[test]
    fn validating_resolver_filters_mixed_answers() {
        struct Mock(Vec<SocketAddr>);
        impl AddressSource for Mock {
            fn resolve(&self, _host: &str, _port: u16) -> std::io::Result<Vec<SocketAddr>> {
                Ok(self.0.clone())
            }
        }

        let mixed = vec![
            SocketAddr::new(ip("8.8.8.8"), 443),
            SocketAddr::new(ip("10.0.0.1"), 443),
        ];
        let resolver =
            ValidatingResolver::with_source(SsrfPolicy::default(), Arc::new(Mock(mixed)));
        let out = resolver.validate_host("rebinding.example", 443).unwrap();
        assert_eq!(out, vec![SocketAddr::new(ip("8.8.8.8"), 443)]);

        let all_private = vec![SocketAddr::new(ip("10.0.0.1"), 443)];
        let resolver =
            ValidatingResolver::with_source(SsrfPolicy::default(), Arc::new(Mock(all_private)));
        assert!(matches!(
            resolver.validate_host("private.example", 443),
            Err(SsrfError::AddressBlocked(_))
        ));
    }

    #[test]
    fn literal_ips_bypass_dns_but_not_policy() {
        let resolver = ValidatingResolver::new(SsrfPolicy::default());
        let ok = resolver.validate_host("8.8.8.8", 443).unwrap();
        assert_eq!(ok, vec![SocketAddr::new(ip("8.8.8.8"), 443)]);
        assert!(matches!(
            resolver.validate_host("169.254.169.254", 80),
            Err(SsrfError::AddressBlocked(_))
        ));

        let relaxed = ValidatingResolver::new(SsrfPolicy::allow_loopback());
        assert!(relaxed.validate_host("127.0.0.1", 8080).is_ok());
    }

    /// SEC2-01: IP-literal URLs never reach the resolver (hyper-util
    /// short-circuits literals), so the client wrapper must enforce the
    /// policy per request. This is the boundary adapters use.
    #[test]
    fn checked_client_blocks_literal_and_scheme_violations() {
        // Client construction requires an installed rustls provider (ADR-006).
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = build_client(SsrfPolicy::default());

        // Metadata + private literals blocked at the request boundary.
        for literal in [
            "https://169.254.169.254/latest/meta-data",
            "https://10.0.0.5/v1",
        ] {
            let err = client
                .request(reqwest::Method::GET, url::Url::parse(literal).unwrap())
                .unwrap_err();
            assert!(matches!(err, SsrfError::AddressBlocked(_)), "{literal}");
        }
        // Scheme violations blocked (also covers the no-production-caller
        // concern for validate_url).
        let err = client
            .request(
                reqwest::Method::GET,
                url::Url::parse("http://api.example.com/v1").unwrap(),
            )
            .unwrap_err();
        assert!(matches!(err, SsrfError::SchemeBlocked(_)));

        // Public literal and public hostname pass validation.
        assert!(
            client
                .request(
                    reqwest::Method::GET,
                    url::Url::parse("https://8.8.8.8/dns-query").unwrap()
                )
                .is_ok()
        );
        assert!(
            client
                .request(
                    reqwest::Method::GET,
                    url::Url::parse("https://api.example.com/v1").unwrap()
                )
                .is_ok()
        );

        // Loopback opt-in admits only loopback literals over http.
        let local = build_client(SsrfPolicy::allow_loopback());
        assert!(
            local
                .request(
                    reqwest::Method::GET,
                    url::Url::parse("http://127.0.0.1:11434/v1").unwrap()
                )
                .is_ok()
        );
        assert!(
            local
                .request(
                    reqwest::Method::GET,
                    url::Url::parse("http://10.0.0.1:11434/v1").unwrap()
                )
                .is_err()
        );
    }
}
