//! Target parsing and bounded DNS resolution. Target text never enters a shell.
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use url::{Host, Url};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedTarget {
    /// ASCII hostname (IDNA where needed), or a canonical unbracketed IP address.
    pub host: String,
    pub canonical: String,
    /// The extracted hostname shown to the user; never contains URL credentials/path.
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetError {
    Empty,
    TooLong,
    InvalidUrl,
    InvalidHost,
    DnsTimeout,
    DnsFailed,
    NoAddresses,
}
impl std::fmt::Display for TargetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Empty => "empty_target",
                Self::TooLong => "target_too_long",
                Self::InvalidUrl => "invalid_url",
                Self::InvalidHost => "invalid_host",
                Self::DnsTimeout => "dns_timeout",
                Self::DnsFailed => "dns_failed",
                Self::NoAddresses => "no_addresses",
            }
        )
    }
}
impl std::error::Error for TargetError {}

pub fn normalize(value: &str) -> Result<NormalizedTarget, TargetError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(TargetError::Empty);
    }
    if value.len() > 4096 {
        return Err(TargetError::TooLong);
    }
    if value.chars().any(char::is_control) {
        return Err(TargetError::InvalidHost);
    }
    let host = if value.contains("://") {
        let parsed = Url::parse(value).map_err(|_| TargetError::InvalidUrl)?;
        match parsed.host().ok_or(TargetError::InvalidUrl)? {
            Host::Ipv4(ip) => ip.to_string(),
            Host::Ipv6(ip) => ip.to_string(),
            Host::Domain(domain) => domain.to_ascii_lowercase(),
        }
    } else {
        let raw = value
            .strip_prefix('[')
            .and_then(|v| v.strip_suffix(']'))
            .unwrap_or(value);
        if let Ok(ip) = raw.parse::<IpAddr>() {
            ip.to_string()
        } else {
            if value.contains(['/', ':', '@', '?', '#', '\\']) {
                return Err(TargetError::InvalidHost);
            }
            match Host::parse(value).map_err(|_| TargetError::InvalidHost)? {
                Host::Domain(domain) => domain.to_ascii_lowercase(),
                Host::Ipv4(ip) => ip.to_string(),
                Host::Ipv6(ip) => ip.to_string(),
            }
        }
    };
    let host = host.trim_end_matches('.').to_owned();
    if host.len() > 253 {
        return Err(TargetError::TooLong);
    }
    if host.parse::<IpAddr>().is_err() {
        if host.is_empty()
            || !host.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        {
            return Err(TargetError::InvalidHost);
        }
    }
    Ok(NormalizedTarget {
        canonical: host.clone(),
        display: host.clone(),
        host,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub chosen: IpAddr,
    pub candidates: Vec<IpAddr>,
}

pub async fn resolve(target: &NormalizedTarget) -> Result<Resolution, TargetError> {
    if let Ok(ip) = target.host.parse::<IpAddr>() {
        return Ok(Resolution {
            chosen: ip,
            candidates: vec![ip],
        });
    }
    // A cancelled OS resolver call may keep running. Keep its permit inside
    // the blocking closure, so repeated Start/Stop cannot grow a hidden queue.
    static DNS_LIMIT: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    let gate = DNS_LIMIT
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(4)))
        .clone();
    let host = target.host.clone();
    let mut candidates = tokio::time::timeout(Duration::from_secs(5), async move {
        let slot = gate
            .acquire_owned()
            .await
            .map_err(|_| TargetError::DnsFailed)?;
        tokio::task::spawn_blocking(move || {
            let _slot = slot;
            (host.as_str(), 0)
                .to_socket_addrs()
                .map(|addresses| addresses.take(64).map(|v| v.ip()).collect::<Vec<_>>())
                .map_err(|_| TargetError::DnsFailed)
        })
        .await
        .map_err(|_| TargetError::DnsFailed)?
    })
    .await
    .map_err(|_| TargetError::DnsTimeout)??;
    candidates.sort_by(|a, b| a.is_ipv6().cmp(&b.is_ipv6()).then_with(|| a.cmp(b)));
    candidates.dedup();
    let chosen = *candidates.first().ok_or(TargetError::NoAddresses)?;
    Ok(Resolution { chosen, candidates })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strips_url_path_credentials_and_port() {
        let t = normalize("https://user:secret@EXAMPLE.com:8443/path?q=x#frag").unwrap();
        assert_eq!(t.host, "example.com");
        assert_eq!(t.canonical, "example.com");
        assert!(!t.display.contains("secret"));
    }
    #[test]
    fn canonicalizes_ipv6_and_international_names() {
        assert_eq!(
            normalize("https://[2001:db8::1]/a").unwrap().host,
            "2001:db8::1"
        );
        assert_eq!(normalize("[::1]").unwrap().host, "::1");
        assert_eq!(
            normalize("BÜCHER.example.").unwrap().host,
            "xn--bcher-kva.example"
        );
    }
    #[test]
    fn refuses_invalid_and_shell_text() {
        for s in [
            "",
            "a;touch x",
            "$(command)",
            "bad host",
            "-x.example",
            "x..y",
            "http:///",
            "a\nb",
            "a/b",
            "host:443",
        ] {
            assert!(normalize(s).is_err(), "accepted {s:?}");
        }
    }
    #[tokio::test]
    async fn literals_are_pinned_without_dns() {
        let t = normalize("2001:db8::42").unwrap();
        let r = resolve(&t).await.unwrap();
        assert_eq!(
            r.candidates,
            vec!["2001:db8::42".parse::<IpAddr>().unwrap()]
        );
    }
}
