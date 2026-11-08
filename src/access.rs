//! Who may talk to Dogitor at the network level, shared by the HTTP API and
//! the MediaMTX auth hook (`/internal/mediamtx-auth` in `src/api.rs`).
//!
//! Dogitor is meant to be reached only from this machine or over Tailscale.
//! Its listeners bind every interface (so they keep working whatever the
//! tailnet address is, and even if Tailscale comes up after Dogitor), so
//! callers on the LAN are rejected by source address instead.

use std::net::IpAddr;

/// Tailscale's address ranges: IPv4 from the CGNAT block `100.64.0.0/10`,
/// IPv6 from `fd7a:115c:a1e0::/48`.
pub fn is_tailnet(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 100 && (o[1] & 0xc0) == 64
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0
        }
    }
}

pub fn is_loopback(ip: IpAddr) -> bool {
    ip.to_canonical().is_loopback()
}

/// Loopback or a tailnet peer: the only sources allowed to reach the API.
pub fn is_trusted_peer(ip: IpAddr) -> bool {
    is_loopback(ip) || is_tailnet(ip)
}

/// Compare a presented token with the expected one without bailing out at the
/// first differing byte, so response timing doesn't leak how much matched.
pub fn token_eq(presented: &str, expected: &str) -> bool {
    let (a, b) = (presented.as_bytes(), expected.as_bytes());
    if a.is_empty() || a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn tailnet_ranges() {
        assert!(is_tailnet(ip("100.64.0.1")));
        assert!(is_tailnet(ip("100.101.102.103")));
        assert!(is_tailnet(ip("100.127.255.254")));
        assert!(is_tailnet(ip("fd7a:115c:a1e0::1")));
        assert!(is_tailnet(ip("::ffff:100.100.1.1")));

        assert!(!is_tailnet(ip("100.63.255.255")));
        assert!(!is_tailnet(ip("100.128.0.0")));
        assert!(!is_tailnet(ip("192.168.1.20")));
        assert!(!is_tailnet(ip("10.0.0.5")));
        assert!(!is_tailnet(ip("fd7a:115c:a1e1::1")));
    }

    #[test]
    fn trusted_peers() {
        assert!(is_trusted_peer(ip("127.0.0.1")));
        assert!(is_trusted_peer(ip("::1")));
        assert!(is_trusted_peer(ip("::ffff:127.0.0.1")));
        assert!(is_trusted_peer(ip("100.90.1.2")));
        assert!(!is_trusted_peer(ip("192.168.1.20")));
        assert!(!is_trusted_peer(ip("0.0.0.0")));
    }

    #[test]
    fn token_comparison() {
        assert!(token_eq("abc123", "abc123"));
        assert!(!token_eq("abc124", "abc123"));
        assert!(!token_eq("abc12", "abc123"));
        assert!(!token_eq("", ""));
        assert!(!token_eq("", "abc"));
    }
}
