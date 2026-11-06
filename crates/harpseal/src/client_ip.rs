//! Which IP address a request came from, without trusting headers a
//! client can forge.
//!
//! 1. `CF-Connecting-IP`, when the runtime recorded no peer (Workers:
//!    Cloudflare sets the header and overwrites any value the client sent)
//!    or `[staff] trust_forwarded_for` is on (behind a known proxy, which
//!    may be Cloudflare). On a native server reached directly the client
//!    could send it, so it is ignored there.
//! 2. `X-Forwarded-For`, only with `trust_forwarded_for`: the right-most
//!    hop that is not a private, loopback or link-local address (each proxy
//!    appends the address it saw, so the left of the list is the client's
//!    to write). When every hop is private, the left-most one.
//! 3. The [`crate::PeerIp`] the runtime recorded.

use std::net::IpAddr;

use axum::http::HeaderMap;

/// The client IP for `headers`, `peer` and the trust setting.
pub(crate) fn resolve(
    headers: &HeaderMap,
    peer: Option<String>,
    trust_forwarded_for: bool,
) -> Option<String> {
    let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    if (peer.is_none() || trust_forwarded_for)
        && let Some(ip) = text("cf-connecting-ip").and_then(parse)
    {
        return Some(ip.to_string());
    }
    if trust_forwarded_for && let Some(ip) = text("x-forwarded-for").and_then(right_most_untrusted)
    {
        return Some(ip.to_string());
    }
    peer
}

fn parse(s: &str) -> Option<IpAddr> {
    s.trim().parse().ok()
}

fn right_most_untrusted(xff: &str) -> Option<IpAddr> {
    let hops: Vec<IpAddr> = xff.split(',').filter_map(parse).collect();
    hops.iter()
        .rev()
        .find(|ip| !is_internal(ip))
        .or_else(|| hops.first())
        .copied()
}

/// Private, loopback, link-local or unique-local: a proxy, not a client.
fn is_internal(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, v.parse().unwrap());
        }
        h
    }

    const PEER: &str = "192.0.2.10";

    #[test]
    fn workers_use_cf_connecting_ip_and_ignore_forwarded_for() {
        let h = headers(&[
            ("cf-connecting-ip", "198.51.100.1"),
            ("x-forwarded-for", "6.6.6.6"),
        ]);
        assert_eq!(resolve(&h, None, false).as_deref(), Some("198.51.100.1"));
        let h = headers(&[("x-forwarded-for", "6.6.6.6, 198.51.100.1")]);
        assert_eq!(resolve(&h, None, false), None);
    }

    #[test]
    fn a_direct_native_server_uses_the_peer_whatever_the_headers_say() {
        let h = headers(&[
            ("cf-connecting-ip", "6.6.6.6"),
            ("x-forwarded-for", "6.6.6.7"),
        ]);
        assert_eq!(resolve(&h, Some(PEER.into()), false).as_deref(), Some(PEER));
    }

    #[test]
    fn behind_a_trusted_proxy_the_right_most_public_hop_wins() {
        let h = headers(&[("x-forwarded-for", "6.6.6.6, 203.0.113.7, 10.0.0.2")]);
        let ip = resolve(&h, Some("10.0.0.1".into()), true);
        assert_eq!(ip.as_deref(), Some("203.0.113.7"));
        let lan = headers(&[("x-forwarded-for", "192.168.1.20, 10.0.0.2")]);
        assert_eq!(
            resolve(&lan, Some("10.0.0.1".into()), true).as_deref(),
            Some("192.168.1.20")
        );
        let junk = headers(&[("x-forwarded-for", "not-an-ip")]);
        assert_eq!(
            resolve(&junk, Some(PEER.into()), true).as_deref(),
            Some(PEER)
        );
    }

    #[test]
    fn behind_a_trusted_proxy_cf_connecting_ip_comes_first() {
        let h = headers(&[
            ("cf-connecting-ip", "198.51.100.1"),
            ("x-forwarded-for", "203.0.113.7"),
        ]);
        assert_eq!(
            resolve(&h, Some(PEER.into()), true).as_deref(),
            Some("198.51.100.1")
        );
    }
}
