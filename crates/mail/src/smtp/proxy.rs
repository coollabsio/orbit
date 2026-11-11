//! The client address behind a TCP proxy: PROXY protocol v2 (binary only) and the Docker bridge check (spec §4.5).
//!
//! The listener calls [`read_proxy_v2`] only for peers in `ORBIT_MAIL_PROXY_CIDRS`; it never guesses whether a
//! header is present. Format: <https://www.haproxy.org/download/3.1/doc/proxy-protocol.txt> §2.2.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};

const SIGNATURE: [u8; 12] = [
    0x0D, 0x0A, 0x0D, 0x0A, 0x00, 0x0D, 0x0A, 0x51, 0x55, 0x49, 0x54, 0x0A,
];
const CMD_LOCAL: u8 = 0x0;
const CMD_PROXY: u8 = 0x1;
const TLV_CRC32C: u8 = 0x03;

/// The gateway of Docker's default bridge network (`docker0`).
pub const DOCKER_DEFAULT_GATEWAY: Ipv4Addr = Ipv4Addr::new(172, 17, 0, 1);

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("no PROXY v2 header before the timeout")]
    Timeout,
    #[error("connection error while reading the PROXY header: {0}")]
    Io(#[from] io::Error),
    #[error("not a PROXY v2 header")]
    Signature,
    #[error("unsupported PROXY protocol version {0}")]
    Version(u8),
    #[error("unknown PROXY command {0}")]
    Command(u8),
    #[error("unknown PROXY address family or protocol {0:#04x}")]
    Family(u8),
    #[error("PROXY header shorter than its addresses or TLVs")]
    Truncated,
    #[error("PROXY header CRC32C mismatch")]
    Checksum,
}

/// Reads exactly one PROXY v2 header (16 bytes plus its length) from `stream`, waiting at most `timeout` for all of
/// it, and returns the client's address. `LOCAL` (a proxy health check), `UNSPEC` and UNIX sockets return `None`: the
/// caller keeps the socket's peer address. Bytes after the header stay in `stream`.
pub async fn read_proxy_v2<S>(
    stream: &mut S,
    timeout: Duration,
) -> Result<Option<SocketAddr>, ProxyError>
where
    S: AsyncRead + Unpin,
{
    tokio::time::timeout(timeout, read_header(stream))
        .await
        .map_err(|_| ProxyError::Timeout)?
}

async fn read_header<S: AsyncRead + Unpin>(
    stream: &mut S,
) -> Result<Option<SocketAddr>, ProxyError> {
    let mut fixed = [0u8; 16];
    stream.read_exact(&mut fixed).await?;
    if fixed[..12] != SIGNATURE {
        return Err(ProxyError::Signature);
    }
    let version = fixed[12] >> 4;
    if version != 2 {
        return Err(ProxyError::Version(version));
    }
    let command = fixed[12] & 0x0F;
    if command != CMD_LOCAL && command != CMD_PROXY {
        return Err(ProxyError::Command(command));
    }
    let family = fixed[13];
    let address_len = match family {
        0x00 => 0,
        0x11 | 0x12 => 12,
        0x21 | 0x22 => 36,
        0x31 | 0x32 => 216,
        other => return Err(ProxyError::Family(other)),
    };
    let len = usize::from(u16::from_be_bytes([fixed[14], fixed[15]]));
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;

    if command == CMD_LOCAL {
        // The spec says to skip the block of a LOCAL header without parsing it.
        return Ok(None);
    }
    if body.len() < address_len {
        return Err(ProxyError::Truncated);
    }
    check_tlvs(&fixed, &body, address_len)?;

    let addr = match family {
        0x11 => {
            let ip = Ipv4Addr::new(body[0], body[1], body[2], body[3]);
            let port = u16::from_be_bytes([body[8], body[9]]);
            Some(SocketAddr::V4(SocketAddrV4::new(ip, port)))
        }
        0x21 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&body[..16]);
            let port = u16::from_be_bytes([body[32], body[33]]);
            Some(SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::from(octets),
                port,
                0,
                0,
            )))
        }
        // UNSPEC, UDP and UNIX: the spec's fallback is the real connection endpoints.
        _ => None,
    };
    Ok(addr)
}

/// Walks the TLVs after the address block; a CRC32C TLV must match the whole header (spec §2.2.5).
fn check_tlvs(fixed: &[u8; 16], body: &[u8], address_len: usize) -> Result<(), ProxyError> {
    let mut at = address_len;
    while at < body.len() {
        let Some(tlv) = body.get(at..at + 3) else {
            return Err(ProxyError::Truncated);
        };
        let kind = tlv[0];
        let value = at + 3;
        let end = value + usize::from(u16::from_be_bytes([tlv[1], tlv[2]]));
        if end > body.len() {
            return Err(ProxyError::Truncated);
        }
        if kind == TLV_CRC32C {
            let Ok(expected) = <[u8; 4]>::try_from(&body[value..end]) else {
                return Err(ProxyError::Truncated);
            };
            let mut crc = Crc32c::new();
            crc.update(fixed);
            crc.update(&body[..value]);
            crc.update(&[0; 4]);
            crc.update(&body[end..]);
            if crc.finish() != u32::from_be_bytes(expected) {
                return Err(ProxyError::Checksum);
            }
        }
        at = end;
    }
    Ok(())
}

/// CRC-32C (Castagnoli, reflected polynomial `0x82F63B78`). Bitwise: a header is at most a few hundred bytes.
struct Crc32c(u32);

impl Crc32c {
    fn new() -> Self {
        Self(!0)
    }

    fn update(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u32::from(byte);
            for _ in 0..8 {
                self.0 = (self.0 >> 1) ^ (0x82F6_3B78 & (self.0 & 1).wrapping_neg());
            }
        }
    }

    fn finish(self) -> u32 {
        !self.0
    }
}

/// Whether `ip` is a Docker bridge gateway: the default `172.17.0.1`, or one of `extra_gateways` (Compose network
/// gateways the server found). Connections from such an address went through Docker's userland proxy, so the real
/// client address is lost; the listener warns about it. IPv4-mapped IPv6 addresses compare as IPv4.
pub fn is_docker_bridge(ip: IpAddr, extra_gateways: &[IpAddr]) -> bool {
    let ip = ip.to_canonical();
    ip == IpAddr::V4(DOCKER_DEFAULT_GATEWAY)
        || extra_gateways.iter().any(|gw| gw.to_canonical() == ip)
}
