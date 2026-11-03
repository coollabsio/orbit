//! Guards for connections to hosts that a user picks (SSRF): only public internet addresses may be reached.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

/// An address on the public internet: not loopback, private, link-local, shared, reserved,
/// multicast or documentation space.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return is_public_v4(mapped);
            }
            let segments = ip.segments();
            let embedded = |high: u16, low: u16| {
                let [a, b] = high.to_be_bytes();
                let [c, d] = low.to_be_bytes();
                Ipv4Addr::new(a, b, c, d)
            };
            // NAT64 (64:ff9b::/96) carries an IPv4 address in its last 32 bits.
            if segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                return is_public_v4(embedded(segments[6], segments[7]));
            }
            // IPv4-compatible (::a.b.c.d, deprecated). Also `::` and `::1`, which embed 0.0.0.x.
            if segments[..6] == [0; 6] {
                return is_public_v4(embedded(segments[6], segments[7]));
            }
            // 6to4 (2002::/16) carries an IPv4 address in bits 16..48.
            if segments[0] == 0x2002 {
                return is_public_v4(embedded(segments[1], segments[2]));
            }
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || (segments[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (segments[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
                || (segments[0] & 0xffc0) == 0xfec0 // site-local fec0::/10 (deprecated)
                || (segments[0] == 0x64 && segments[1] == 0xff9b && segments[2] == 1) // local-use NAT64 64:ff9b:1::/48
                || segments[..4] == [0x100, 0, 0, 0] // discard 100::/64
                || (segments[0] == 0x2001 && segments[1] == 0) // Teredo 2001::/32
                || (segments[0] == 0x3fff && segments[1] < 0x1000) // documentation 3fff::/20
                || (segments[0] == 0x2001 && segments[1] == 0x0db8) // documentation
                || ip == Ipv6Addr::LOCALHOST)
        }
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_documentation()
        || a == 0
        || (a == 100 && (b & 0xc0) == 64) // shared address space 100.64.0.0/10
        || (a == 192 && b == 0 && c == 0) // protocol assignments 192.0.0.0/24
        || (a == 192 && b == 88 && c == 99) // 6to4 relay anycast 192.88.99.0/24
        || (a == 198 && (b & 0xfe) == 18) // benchmarks 198.18.0.0/15
        || a >= 240)
}

/// Resolves a host name to its public addresses only. The connection uses what this returns, so
/// a name that resolves to a local address (also after a redirect) cannot be reached.
pub struct PublicResolver;

impl Resolve for PublicResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let addresses: Vec<SocketAddr> = tokio::net::lookup_host((name.as_str(), 0))
                .await?
                .filter(|address| is_public_ip(address.ip()))
                .collect();
            if addresses.is_empty() {
                return Err("the host has no public address".into());
            }
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::is_public_ip;

    #[test]
    fn local_addresses_are_not_public() {
        for local in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.10",
            "169.254.1.1",
            "100.64.0.1",
            "::1",
            "fc00::2",
            "::ffff:127.0.0.1",
            "64:ff9b::7f00:1",
            "64:ff9b::a00:1",
            "64:ff9b:1::1",
            "64:ff9b:1::101:101",
            "::127.0.0.1",
            "::10.0.0.1",
            "::",
            "::0.0.0.2",
            "2002:7f00:1::1",
            "2002:a9fe:a9fe::1",
            "2002:c0a8:1::",
            "fec0::1",
            "feff::1",
            "100::1",
            "2001::1",
            "3fff::1",
            "192.88.99.1",
        ] {
            assert!(!is_public_ip(local.parse().unwrap()), "{local}");
        }
        for public in [
            "1.1.1.1",
            "93.184.216.34",
            "2606:4700::1111",
            "64:ff9b::101:101",
            "::1.1.1.1",
            "2002:101:101::1",
        ] {
            assert!(is_public_ip(public.parse().unwrap()), "{public}");
        }
    }
}
