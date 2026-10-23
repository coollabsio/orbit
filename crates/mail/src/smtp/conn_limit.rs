//! Concurrent connection limits for the listener: per IP, per IPv6 /64 and global (spec §4.5). The counts live in
//! this process, so in a cluster each server enforces them on its own, like the HTTP limits.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::Hash;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Counts open connections. Cloning shares the counts.
#[derive(Debug, Clone)]
pub struct ConnLimiter {
    per_ip: usize,
    per_v6_64: usize,
    global: usize,
    counts: Arc<Mutex<Counts>>,
}

#[derive(Debug, Default)]
struct Counts {
    total: usize,
    by_ip: HashMap<IpAddr, usize>,
    /// The upper 64 bits of native IPv6 addresses.
    by_v6_64: HashMap<u64, usize>,
}

/// One open connection. Dropping it frees the slot.
#[derive(Debug)]
pub struct ConnPermit {
    counts: Arc<Mutex<Counts>>,
    ip: IpAddr,
}

impl Default for ConnLimiter {
    /// 10 per IP, 20 per IPv6 /64, 500 in total.
    fn default() -> Self {
        Self::new(10, 20, 500)
    }
}

impl ConnLimiter {
    pub fn new(per_ip: usize, per_v6_64: usize, global: usize) -> Self {
        Self {
            per_ip,
            per_v6_64,
            global,
            counts: Arc::default(),
        }
    }

    /// Takes a slot for `ip`, or `None` when one of the limits is reached. IPv4-mapped IPv6 addresses count as
    /// their IPv4 address.
    pub fn try_acquire(&self, ip: IpAddr) -> Option<ConnPermit> {
        let ip = ip.to_canonical();
        let mut counts = lock(&self.counts);
        if counts.total >= self.global || count(&counts.by_ip, &ip) >= self.per_ip {
            return None;
        }
        let net = v6_64(ip);
        if net.is_some_and(|net| count(&counts.by_v6_64, &net) >= self.per_v6_64) {
            return None;
        }
        counts.total += 1;
        *counts.by_ip.entry(ip).or_default() += 1;
        if let Some(net) = net {
            *counts.by_v6_64.entry(net).or_default() += 1;
        }
        Some(ConnPermit {
            counts: self.counts.clone(),
            ip,
        })
    }

    /// Open connections now.
    pub fn active(&self) -> usize {
        lock(&self.counts).total
    }
}

impl Drop for ConnPermit {
    fn drop(&mut self) {
        let mut counts = lock(&self.counts);
        counts.total -= 1;
        release(&mut counts.by_ip, self.ip);
        if let Some(net) = v6_64(self.ip) {
            release(&mut counts.by_v6_64, net);
        }
    }
}

fn lock(counts: &Mutex<Counts>) -> MutexGuard<'_, Counts> {
    // The counts stay consistent under a panic elsewhere: every update is complete before the guard drops.
    counts.lock().unwrap_or_else(PoisonError::into_inner)
}

fn count<K: Eq + Hash>(map: &HashMap<K, usize>, key: &K) -> usize {
    map.get(key).copied().unwrap_or(0)
}

fn release<K: Eq + Hash>(map: &mut HashMap<K, usize>, key: K) {
    if let Entry::Occupied(mut entry) = map.entry(key) {
        *entry.get_mut() -= 1;
        if *entry.get() == 0 {
            entry.remove();
        }
    }
}

fn v6_64(ip: IpAddr) -> Option<u64> {
    match ip {
        IpAddr::V6(v6) => Some((u128::from(v6) >> 64) as u64),
        IpAddr::V4(_) => None,
    }
}
