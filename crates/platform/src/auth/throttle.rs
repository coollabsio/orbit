use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::time::Duration;

use crate::TimestampMillis;

const WINDOW_MILLIS: i64 = 15 * 60 * 1_000;
const IP_FAILURE_LIMIT: usize = 50;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThrottleDecision {
    Allowed,
    RetryAfter(Duration),
}

#[derive(Clone, Copy, Debug)]
struct EmailFailures {
    count: u32,
    last_failure_at: TimestampMillis,
    blocked_until: TimestampMillis,
}

#[derive(Debug, Default)]
pub struct LoginThrottler {
    emails: HashMap<String, EmailFailures>,
    ips: HashMap<IpAddr, VecDeque<TimestampMillis>>,
}

impl LoginThrottler {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn check(&mut self, email: &str, ip: IpAddr, now: TimestampMillis) -> ThrottleDecision {
        self.expire_old(now);
        let normalized = normalize_email(email);
        if let Some(failures) = self.emails.get(&normalized)
            && now < failures.blocked_until
        {
            return retry_after(now, failures.blocked_until);
        }
        if let Some(failures) = self.ips.get(&ip)
            && failures.len() >= IP_FAILURE_LIMIT
            && let Some(first) = failures.front()
        {
            return retry_after(
                now,
                TimestampMillis::from_millis(first.as_millis() + WINDOW_MILLIS),
            );
        }
        ThrottleDecision::Allowed
    }

    pub fn record_failure(&mut self, email: &str, ip: IpAddr, now: TimestampMillis) {
        self.expire_old(now);
        self.ips.entry(ip).or_default().push_back(now);
        let failures = self
            .emails
            .entry(normalize_email(email))
            .or_insert(EmailFailures {
                count: 0,
                last_failure_at: now,
                blocked_until: now,
            });
        failures.count += 1;
        failures.last_failure_at = now;
        if failures.count >= 5 {
            let exponent = (failures.count - 5).min(31);
            let seconds = 5_u64.saturating_mul(1_u64 << exponent).min(15 * 60);
            failures.blocked_until = TimestampMillis::from_millis(
                now.as_millis()
                    .saturating_add(i64::try_from(seconds * 1_000).unwrap_or(i64::MAX)),
            );
        }
    }

    pub fn record_success(&mut self, email: &str) {
        self.emails.remove(&normalize_email(email));
    }

    fn expire_old(&mut self, now: TimestampMillis) {
        let cutoff = now.as_millis() - WINDOW_MILLIS;
        self.emails.retain(|_, failure| {
            failure.last_failure_at.as_millis() > cutoff || failure.blocked_until >= now
        });
        self.ips.retain(|_, failures| {
            while failures
                .front()
                .is_some_and(|failure| failure.as_millis() <= cutoff)
            {
                failures.pop_front();
            }
            !failures.is_empty()
        });
    }
}

#[must_use]
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

fn retry_after(now: TimestampMillis, until: TimestampMillis) -> ThrottleDecision {
    let milliseconds = (until.as_millis() - now.as_millis()).max(1);
    ThrottleDecision::RetryAfter(Duration::from_millis(milliseconds as u64))
}
