//! Transport guard for outbound requests that carry credentials.

use url::{Host, Url};

/// Returns `Ok(())` when `url` may carry credentials: `https://` for any host,
/// or `http://` only for loopback hosts (`localhost`, `127.0.0.0/8`, `::1`).
///
/// Parsed with the same parser reqwest uses, so the host checked here is the
/// host the request is sent to (no userinfo or delimiter tricks).
pub fn require_secure_transport(url: &str) -> Result<(), String> {
    let parsed = Url::parse(url)
        .map_err(|error| format!("invalid URL for credentialed request: {error}"))?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" => match parsed.host() {
            Some(Host::Domain("localhost")) => Ok(()),
            Some(Host::Ipv4(address)) if address.is_loopback() => Ok(()),
            Some(Host::Ipv6(address)) if address.is_loopback() => Ok(()),
            _ => Err(format!(
                "refusing to send credentials over plain HTTP to {}; use https://",
                parsed.host_str().unwrap_or("<no host>")
            )),
        },
        other => Err(format!(
            "unsupported URL scheme for credentialed request: {other}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::require_secure_transport;

    #[test]
    fn allows_https_and_loopback_http() {
        assert!(require_secure_transport("https://api.example.com/v1").is_ok());
        assert!(require_secure_transport("http://127.0.0.1:8080/").is_ok());
        assert!(require_secure_transport("http://127.9.9.9:8080/").is_ok());
        assert!(require_secure_transport("http://localhost:9/x").is_ok());
        assert!(require_secure_transport("http://[::1]:9/").is_ok());
    }

    #[test]
    fn rejects_plain_http_remote_and_other_schemes() {
        assert!(require_secure_transport("http://api.example.com/").is_err());
        assert!(require_secure_transport("http://127.0.0.1.evil.example/").is_err());
        assert!(require_secure_transport("ftp://example.com/").is_err());
        assert!(require_secure_transport("not a url").is_err());
    }

    #[test]
    fn rejects_userinfo_host_confusion() {
        assert!(require_secure_transport("http://localhost:x@evil.example/").is_err());
        assert!(require_secure_transport("http://localhost@evil.example/").is_err());
        assert!(require_secure_transport("http://[::1]@evil.example/").is_err());
        assert!(require_secure_transport("http://127.0.0.1@evil.example/").is_err());
    }
}
