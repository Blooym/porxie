//! Provides DNS networking utilities.

use core::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use thiserror::Error;

#[derive(Debug, Error)]
enum SsrfGuardedDnsResolveError {
    #[error("hostname resolves only to private or reserved IP addresses")]
    NoSafeAddress,
}

/// Reqwest DNS resolver that blocks connections to private or reserved IP addresses.
///
/// Uses the system DNS resolver via [`tokio::net::lookup_host`].
pub struct SsrfGuardedDnsResolver;

impl SsrfGuardedDnsResolver {
    /// Returns `true` if the IPv4 address is considered private.
    ///
    /// Covers the following ranges:
    /// - [`Ipv4Addr::is_broadcast`]
    /// - [`Ipv4Addr::is_documentation`]
    /// - [`Ipv4Addr::is_link_local`]
    /// - [`Ipv4Addr::is_loopback`]
    /// - [`Ipv4Addr::is_private`]
    /// - [`Ipv4Addr::is_unspecified`]
    /// - `0.0.0.0/8`  — "This" network (RFC 1122 §3.2.1.3).
    /// - `100.64.0.0/10` — CGNAT shared address space (RFC 6598).
    fn ip_is_private_v4(ip: Ipv4Addr) -> bool {
        ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_link_local()
        || ip.is_loopback()
        || ip.is_multicast()
        || ip.is_private()
        || ip.is_unspecified()
        || ip.octets()[0] == 0 // 0.0.0.0/8 is_unspecified only catches 0.0.0.0
        || (ip.octets()[0] == 100 && (ip.octets()[1] & 0xc0) == 64) // 100.64.0.0/10 CGNAT (Ipv4Addr::is_shared when stable).
    }

    /// Returns `true` if the IPv6 address is considered private.
    ///
    /// Covers the following ranges:
    /// - IPv4-mapped addresses: unwrapped and passed to [`Self::ip_is_private_v4`].
    /// - [`Ipv6Addr::is_loopback`]
    /// - [`Ipv6Addr::is_multicast`]
    /// - [`Ipv6Addr::is_unicast_link_local`]
    /// - [`Ipv6Addr::is_unique_local`]
    /// - [`Ipv6Addr::is_unspecified`]
    /// - `2001::/32`  — Teredo (RFC 4380).
    /// - `2002::/16`  — 6to4 (RFC 3056).
    /// - `64:ff9b::/32` — NAT64 (RFC 6052, RFC 8215).
    fn ip_is_private_v6(ip: Ipv6Addr) -> bool {
        if let Some(v4) = ip.to_ipv4_mapped() {
            return Self::ip_is_private_v4(v4);
        }

        ip.is_loopback()
        || ip.is_multicast()
        || ip.is_unicast_link_local()
        || ip.is_unique_local()
        || ip.is_unspecified()
        || matches!(ip.segments(), [0x2001, 0xdb8, ..] | [0x3fff, 0..=0x3fff, ..]) // (Ipv6Addr::is_documentation when stable).
        || matches!(ip.segments(), [0x2001, 0x0000, ..]) // Teredo (RFC 4380)
        || ip.segments()[0] == 0x2002 // 6to4 (RFC 3056)
        || matches!(ip.segments(), [0x0064, 0xff9b, ..]) // NAT64 (RFC 6052, RFC 8215)
    }
}

impl Resolve for SsrfGuardedDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            // Filter out all IPs within internal ranges.
            let safe: Vec<SocketAddr> = tokio::net::lookup_host((name.as_str(), 0u16))
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?
                .filter(|addr| match addr.ip() {
                    IpAddr::V4(ip) => !Self::ip_is_private_v4(ip),
                    IpAddr::V6(ip) => !Self::ip_is_private_v6(ip),
                })
                .collect();

            // No safe ip addresses to resolve with.
            if safe.is_empty() {
                return Err(Box::new(SsrfGuardedDnsResolveError::NoSafeAddress)
                    as Box<dyn std::error::Error + Send + Sync>);
            }

            // Only return safe addresses.
            Ok(Box::new(safe.into_iter()) as Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::net::{Ipv4Addr, Ipv6Addr};

    fn check_v4(cases: &[(Ipv4Addr, bool)]) {
        for (ip, expected) in cases {
            assert_eq!(
                SsrfGuardedDnsResolver::ip_is_private_v4(*ip),
                *expected,
                "{ip}"
            );
        }
    }

    fn check_v6(cases: &[(Ipv6Addr, bool)]) {
        for (ip, expected) in cases {
            assert_eq!(
                SsrfGuardedDnsResolver::ip_is_private_v6(*ip),
                *expected,
                "{ip}"
            );
        }
    }

    #[test]
    fn v4_ssrf() {
        check_v4(&[
            // Public
            (Ipv4Addr::new(1, 1, 1, 1), false),
            (Ipv4Addr::new(93, 184, 216, 34), false),
            // Loopback
            (Ipv4Addr::new(127, 0, 0, 1), true),
            // "This Network"
            (Ipv4Addr::new(0, 1, 2, 3), true), // 0.0.0.0/8
            // Private
            (Ipv4Addr::new(10, 0, 0, 1), true),
            (Ipv4Addr::new(172, 16, 0, 1), true),
            (Ipv4Addr::new(172, 31, 255, 255), true), // upper bound
            (Ipv4Addr::new(172, 32, 0, 0), false),    // just above
            (Ipv4Addr::new(192, 168, 1, 1), true),
            // Link local
            (Ipv4Addr::new(169, 254, 0, 1), true),
            // Multicast
            (Ipv4Addr::new(224, 0, 0, 1), true),
            // Broadcast
            (Ipv4Addr::new(255, 255, 255, 255), true),
            // CGNAT
            (Ipv4Addr::new(100, 64, 0, 0), true), // lower bound
            (Ipv4Addr::new(100, 127, 255, 255), true), // upper bound
            (Ipv4Addr::new(100, 63, 255, 255), false), // just below
            (Ipv4Addr::new(100, 128, 0, 0), false), // just above
        ]);
    }

    #[test]
    fn v6_ssrf() {
        check_v6(&[
            // Public
            ("2606:4700::".parse().unwrap(), false),
            // Loopback
            ("::1".parse().unwrap(), true),
            // Unspecified
            ("::".parse().unwrap(), true),
            // Multicast
            ("ff02::1".parse().unwrap(), true),
            // Teredo
            ("2001::1".parse().unwrap(), true),
            // 6to4
            ("2002::1".parse().unwrap(), true),
            // Link Local
            ("fe80::1".parse().unwrap(), true),
            // Unique Local
            ("fc00::1".parse().unwrap(), true),
            ("fd12:3456:789a::1".parse().unwrap(), true),
            // Documentation
            ("2001:db8::1".parse().unwrap(), true),
            ("3fff::1".parse().unwrap(), true),
            // NAT64
            ("64:ff9b::127.0.0.1".parse().unwrap(), true), // Loopback
            ("64:ff9b::10.0.0.1".parse().unwrap(), true),  // Private
            ("64:ff9b:1::1".parse().unwrap(), true),       // Local-use prefix
            // IPv4-mapped
            ("::ffff:127.0.0.1".parse().unwrap(), true), // Loopback
            ("::ffff:10.0.0.1".parse().unwrap(), true),  // Private
            ("::ffff:1.1.1.1".parse().unwrap(), false),  // Public
        ]);
    }
}
