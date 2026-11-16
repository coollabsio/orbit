//! Delivery of a released message (spec §5.3 steps 5–7, §5.4): each run of `mail.send` claims
//! the due recipients, sends to them and records each result, then says when to run again.
//!
//! - **No double send.** The recipients are `sending` with the attempt's ID and committed
//!   before anything is sent (`MailRepository::claim_recipients`); only then do they go out,
//!   and the results are recorded for that attempt only. All the calls of a run (adapter and
//!   intake) share one deadline, `SendJobState::send_timeout` (20 min,
//!   [`super::SEND_TIMEOUT_MS`]) after the claim: a call still running then is cut off, a call
//!   not started does not start, and their recipients are deferred with "Timed out". A worker
//!   that stops in between leaves them `sending` until [`SENDING_LEASE_MS`] (30 min, longer than
//!   the deadline) ends; they are then deferred and sent again (at least once), and a late
//!   result of the old attempt is ignored.
//! - **Local recipients** (D11): an address of a domain of this instance (verified, or
//!   suspended) whose stored MX status is OK (its last DNS check found the MX record right)
//!   goes straight to intake (`MailIntake::accept`, `Source::Local`) with the same signed bytes,
//!   so a copy to the sender's own mailbox is deduped like any other (`mailbox_id`,
//!   `raw_sha256`). Intake's recipient lookup decides as for mail from outside: a suspended
//!   domain takes mail in its grace period, and a refusal is recorded as the 5xx it is. A domain
//!   whose MX status is not OK goes by the route.
//! - **Routes.** The From domain's route (`MailRepository::route_for_domain`). `direct` and
//!   `smtp` send one envelope per recipient, with the VERP sender
//!   `bounce+{token}@{from domain}` (the token holds the recipient's ID,
//!   `MailRepository::bounce_key`) and the recipient's ID as correlation; `resend` one per
//!   recipient with the From address and the correlation `{outbound id}:{recipient id}` (its
//!   idempotency key); the other kinds one call for all recipients with the From address and
//!   the outbound's ID as correlation (their events name the address). Provider headers are put
//!   in front of the signed bytes. A `direct` route and a workspace's route never reach a
//!   private address; an instance route of another kind may (a relay on the instance's
//!   network).
//! - **Results.** Accepted → `delivered`; 5xx → `bounced` with `bounce_kind = 'hard'`, or
//!   `'rejected'` for Orbit's own refusals of the message (too large for the route or this
//!   server, local domain or mailbox unavailable); the thread goes back to the Inbox, unread.
//!   4xx, a network error, a timeout or a route error → `deferred` until the next retry step,
//!   then `bounced` with "Gave up after 5 days" (`gave_up`, or `route` when the last attempt was
//!   a route error). A sent message that cannot be read bounces as `unreadable` (it is deferred
//!   while the store may recover). The kinds are flags set by the code that decides them
//!   (`RecipientOutcome`'s `rejected` and `route_error`, `AttemptResult`), never read from a
//!   response's text, so a remote server cannot pass as Orbit or as the route. Only `hard`
//!   counts as a bounce of the recipient. Each change sends `thread.updated` on the `mail` topic.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use orbit_mail::{AuthOutcome, DmarcVerdict, Verdict, bounce_local_part};
use orbit_platform::{Id, JobError, TimestampMillis};
use tokio::time::{Instant, timeout_at};

use super::release::SendJobState;
use super::{
    LOCAL_DOMAIN_UNAVAILABLE, LOCAL_MAILBOX_SUSPENDED, LOCAL_TOO_LARGE, Outgoing, RecipientOutcome,
    SENDING_LEASE_MS, UNREADABLE, adapter_for, domain_of, provider_headers, with_provider_headers,
};
use crate::mail_intake::{InboundMessage, IntakeError, Recipient, Source};
use crate::repositories::mail::{
    AttemptResult, BounceKind, Claim, DeferCause, MailError, OutboundDelivery, RecipientLookup,
    RecipientRow, SendRouteKind,
};

/// The response of the recipients of a call that took longer than the send timeout.
const TIMED_OUT: &str = "Timed out";

/// At most this many single-recipient sends of one run go at the same time.
const PARALLEL_SENDS: usize = 8;

/// When the message's next `mail.send` run is due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextRun {
    /// No run: every recipient is final, or the message waits for something else (a pause).
    Done,
    At(TimestampMillis),
}

fn retry(error: MailError) -> JobError {
    JobError::Retryable(error.to_string())
}

/// One delivery run for released message `outbound_id` at `now` (see the module docs).
pub async fn deliver_due(
    state: &SendJobState,
    outbound_id: Id,
    now: TimestampMillis,
) -> Result<NextRun, JobError> {
    let mail = &state.mail;
    mail.requeue_stale_sending(SENDING_LEASE_MS, now)
        .await
        .map_err(retry)?;
    let Some(outbound) = mail
        .outbound_delivery(outbound_id, now)
        .await
        .map_err(retry)?
    else {
        return Ok(NextRun::Done);
    };
    let attempt_id = Id::new_v7();
    let recipients = match mail
        .claim_recipients(outbound_id, attempt_id, now)
        .await
        .map_err(retry)?
    {
        Claim::NotReleased => return Ok(NextRun::Done),
        // The Outbox shows why it waits.
        Claim::Paused => {
            announce(state, outbound_id).await;
            return Ok(NextRun::Done);
        }
        Claim::Limited(at) => {
            announce(state, outbound_id).await;
            return Ok(NextRun::At(at));
        }
        Claim::Claimed {
            recipients,
            changed,
        } => {
            if changed {
                announce(state, outbound_id).await;
            }
            recipients
        }
    };
    if recipients.is_empty() {
        let next = mail.next_delivery(outbound_id, now).await.map_err(retry)?;
        return Ok(next.map_or(NextRun::Done, NextRun::At));
    }
    // One deadline for every call of this run, so the run ends well inside the lease of the
    // recipients it claimed (a per-call timeout would add up over the calls).
    let deadline = Instant::now() + state.send_timeout;

    let outcomes: Vec<(Id, AttemptResult)> = match mail.read_sent_raw(outbound.receipt_id).await {
        Ok(raw) => send(state, &outbound, &recipients, &raw, now, deadline)
            .await?
            .into_iter()
            .map(|(id, outcome)| (id, outcome.into()))
            .collect(),
        Err(MailError::Unavailable(error)) => {
            // The claimed recipients stay `sending` until the lease ends, then go again.
            return Err(JobError::Retryable(error.to_string()));
        }
        Err(error) => {
            // Gone or not decryptable stays so; another store error may pass.
            let lasting = matches!(error, MailError::NotFound | MailError::Crypto);
            let response = format!("{UNREADABLE}: {error}");
            recipients
                .iter()
                .map(|recipient| {
                    let response = response.clone();
                    let outcome = if lasting {
                        AttemptResult::Bounced {
                            response,
                            kind: BounceKind::Unreadable,
                        }
                    } else {
                        AttemptResult::Deferred {
                            response,
                            cause: DeferCause::Unreadable,
                        }
                    };
                    (recipient.id, outcome)
                })
                .collect()
        }
    };
    let recorded = mail
        .record_outcomes(outbound_id, attempt_id, &outcomes, now)
        .await
        .map_err(retry)?;
    announce(state, outbound_id).await;
    Ok(match recorded.next_at {
        Some(at) if !recorded.done => NextRun::At(at),
        _ => NextRun::Done,
    })
}

/// Sends `raw` to the claimed recipients: the local ones through intake, the others by the
/// route. One outcome for each recipient.
async fn send(
    state: &SendJobState,
    outbound: &OutboundDelivery,
    recipients: &[RecipientRow],
    raw: &[u8],
    now: TimestampMillis,
    deadline: Instant,
) -> Result<Vec<(Id, RecipientOutcome)>, JobError> {
    let mut local_domains: HashMap<String, bool> = HashMap::new();
    let mut local = Vec::new();
    let mut remote = Vec::new();
    for recipient in recipients {
        let domain = domain_of(&recipient.address);
        let is_local = match local_domains.get(&domain) {
            Some(known) => *known,
            None => {
                let found = state.mail.is_local_domain(&domain).await.map_err(retry)?;
                local_domains.insert(domain, found);
                found
            }
        };
        if is_local {
            local.push(recipient);
        } else {
            remote.push(recipient);
        }
    }
    let mut outcomes = Vec::with_capacity(recipients.len());
    let (local_outcomes, by_route) =
        deliver_local(state, outbound, &local, raw, now, deadline).await?;
    outcomes.extend(local_outcomes);
    remote.extend(by_route);
    if !remote.is_empty() {
        outcomes.extend(deliver_by_route(state, outbound, &remote, raw, now, deadline).await?);
    }
    Ok(outcomes)
}

/// Local recipients go to intake in one message (all or nothing, as from the listener). Returns
/// their outcomes and the recipients that turned out not to be local after all.
async fn deliver_local<'r>(
    state: &SendJobState,
    outbound: &OutboundDelivery,
    recipients: &[&'r RecipientRow],
    raw: &[u8],
    now: TimestampMillis,
    deadline: Instant,
) -> Result<(Vec<(Id, RecipientOutcome)>, Vec<&'r RecipientRow>), JobError> {
    let mut outcomes = Vec::new();
    let mut not_local = Vec::new();
    let mut accepted = Vec::new();
    let mut intake_recipients = Vec::new();
    for recipient in recipients {
        let found = state
            .mail
            .lookup_recipient(&recipient.address, now)
            .await
            .map_err(retry)?;
        // The listener's answers at `RCPT TO`, as a remote sender would get them.
        let refused = match found {
            RecipientLookup::Accept {
                address_id,
                workspace_id,
                ..
            } => {
                intake_recipients.push(Recipient::Mailbox {
                    workspace_id,
                    address_id,
                    rcpt: recipient.address.clone(),
                });
                accepted.push(recipient.id);
                continue;
            }
            RecipientLookup::HealthTest { test_id } => {
                intake_recipients.push(Recipient::HealthTest {
                    test_id,
                    rcpt: recipient.address.clone(),
                });
                accepted.push(recipient.id);
                continue;
            }
            // A bounce address of this instance: intake reads it as a DSN and drops it.
            RecipientLookup::Bounce { recipient_id } => {
                intake_recipients.push(Recipient::Bounce {
                    recipient_id,
                    rcpt: recipient.address.clone(),
                });
                accepted.push(recipient.id);
                continue;
            }
            RecipientLookup::NotLocal => {
                not_local.push(*recipient);
                continue;
            }
            // A bad address.
            RecipientLookup::UnknownUser | RecipientLookup::BadBounce => {
                RecipientOutcome::bounced("550 5.1.1 No such user here")
            }
            RecipientLookup::Invalid => {
                RecipientOutcome::bounced("501 5.1.3 The address is not valid")
            }
            // The receiving side's state, not the address.
            RecipientLookup::DomainUnavailable => {
                RecipientOutcome::rejected(LOCAL_DOMAIN_UNAVAILABLE)
            }
            RecipientLookup::MailboxSuspended => {
                RecipientOutcome::rejected(LOCAL_MAILBOX_SUSPENDED)
            }
        };
        outcomes.push((recipient.id, refused));
    }
    if intake_recipients.is_empty() {
        return Ok((outcomes, not_local));
    }
    let message = InboundMessage {
        source: Source::Local,
        raw: raw.to_vec(),
        trace_len: 0,
        mail_from: Some(outbound.from_address.clone()),
        recipients: intake_recipients,
        peer_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        helo: state
            .delivery
            .hostname
            .clone()
            .unwrap_or_else(|| "orbit".to_owned()),
        tls: None,
        // Not stored for a local message (its receipt says `{"local":true}`); a health test
        // records it.
        auth: AuthOutcome {
            spf: Verdict::None,
            dkim: Verdict::Pass,
            dmarc: DmarcVerdict::None,
            aligned_domain: None,
            temp_error: false,
            spf_domain: None,
            dkim_domain: Some(outbound.from_domain.clone()),
            from_domain: Some(outbound.from_domain.clone()),
        },
    };
    let outcome = match timeout_at(deadline, state.intake.accept(message)).await {
        Err(_) => RecipientOutcome::deferred(TIMED_OUT),
        Ok(Ok(_)) => RecipientOutcome::Accepted {
            provider_message_id: None,
        },
        Ok(Err(IntakeError::TooLarge)) => RecipientOutcome::rejected(LOCAL_TOO_LARGE),
        Ok(Err(IntakeError::QuotaExceeded)) => RecipientOutcome::deferred(
            "452 4.2.2 The recipient's workspace is over its storage quota",
        ),
        Ok(Err(IntakeError::Temporary(error))) => RecipientOutcome::deferred(format!(
            "451 4.3.0 The message could not be stored: {error}"
        )),
    };
    outcomes.extend(accepted.into_iter().map(|id| (id, outcome.clone())));
    Ok((outcomes, not_local))
}

/// The route's outcomes for `recipients` (see the module docs for the envelopes).
async fn deliver_by_route(
    state: &SendJobState,
    outbound: &OutboundDelivery,
    recipients: &[&RecipientRow],
    raw: &[u8],
    now: TimestampMillis,
    deadline: Instant,
) -> Result<Vec<(Id, RecipientOutcome)>, JobError> {
    let all = |response: &str| -> Vec<(Id, RecipientOutcome)> {
        recipients
            .iter()
            .map(|recipient| (recipient.id, RecipientOutcome::route_error(response)))
            .collect()
    };
    let Some(domain_id) = outbound.domain_id else {
        return Ok(all("Route error: the sending domain was removed."));
    };
    let (route, secrets) = match state.mail.route_for_domain(domain_id).await {
        Ok(Some(found)) => found,
        Ok(None) => return Ok(all("Route error: the domain has no send route.")),
        Err(MailError::Unavailable(error)) => return Err(JobError::Retryable(error.to_string())),
        Err(error) => {
            return Ok(all(&format!(
                "Route error: the route cannot be read ({error})."
            )));
        }
    };
    // A relay of the instance may be on its own network; direct sending never is.
    let ctx = if route.workspace_id.is_none() && route.kind != SendRouteKind::Direct {
        &state.instance_delivery
    } else {
        &state.delivery
    };
    let adapter = adapter_for(&route, &secrets, ctx);
    let bounce_key = state.mail.bounce_key();
    let one_by_one = matches!(
        route.kind,
        SendRouteKind::Direct | SendRouteKind::Smtp | SendRouteKind::Resend
    );
    let mut by_address: Vec<(String, RecipientOutcome)> = Vec::with_capacity(recipients.len());
    if one_by_one {
        // Boxed one by one: a lazy iterator of borrowing futures is not `Send` for the job.
        let mut sends: Vec<BoxFuture<'_, Vec<(String, RecipientOutcome)>>> =
            Vec::with_capacity(recipients.len());
        for recipient in recipients {
            let (mail_from, correlation) = if route.kind == SendRouteKind::Resend {
                (
                    outbound.from_address.clone(),
                    format!("{}:{}", outbound.outbound_id, recipient.id),
                )
            } else {
                (
                    format!(
                        "{}@{}",
                        bounce_local_part(&recipient.id.to_bytes(), &bounce_key),
                        outbound.from_domain
                    ),
                    recipient.id.to_string(),
                )
            };
            let bytes = with_provider_headers(
                &provider_headers(route.kind, &route.settings, &correlation),
                raw,
            );
            let address = vec![recipient.address.clone()];
            let adapter = &adapter;
            let from_domain = outbound.from_domain.as_str();
            sends.push(Box::pin(async move {
                let outgoing = Outgoing {
                    raw: &bytes,
                    mail_from: &mail_from,
                    from_domain,
                    recipients: &address,
                    correlation: &correlation,
                };
                // A call that would start after the deadline does not start.
                if Instant::now() >= deadline {
                    return vec![(address[0].clone(), RecipientOutcome::deferred(TIMED_OUT))];
                }
                match timeout_at(deadline, adapter.deliver(&outgoing)).await {
                    Ok(results) => results,
                    Err(_) => vec![(address[0].clone(), RecipientOutcome::deferred(TIMED_OUT))],
                }
            }));
        }
        let mut stream = futures_util::stream::iter(sends).buffer_unordered(PARALLEL_SENDS);
        while let Some(results) = stream.next().await {
            by_address.extend(results);
        }
    } else {
        let correlation = outbound.outbound_id.to_string();
        let headers = provider_headers(route.kind, &route.settings, &correlation);
        let bytes = with_provider_headers(&headers, raw);
        let addresses: Vec<String> = recipients
            .iter()
            .map(|recipient| recipient.address.clone())
            .collect();
        let outgoing = Outgoing {
            raw: &bytes,
            mail_from: &outbound.from_address,
            from_domain: &outbound.from_domain,
            recipients: &addresses,
            correlation: &correlation,
        };
        match timeout_at(deadline, adapter.deliver(&outgoing)).await {
            Ok(results) => by_address.extend(results),
            Err(_) => by_address.extend(
                addresses
                    .iter()
                    .map(|address| (address.clone(), RecipientOutcome::deferred(TIMED_OUT))),
            ),
        }
    }

    let mut outcomes = Vec::with_capacity(recipients.len());
    for recipient in recipients {
        let outcome = by_address
            .iter()
            .position(|(address, _)| *address == recipient.address)
            .map_or_else(
                || RecipientOutcome::route_error("Route error: the route gave no result."),
                |index| by_address.swap_remove(index).1,
            );
        outcomes.push((recipient.id, outcome));
    }
    // The route's last send: its time when anything went, else the first temporary failure.
    let accepted = outcomes
        .iter()
        .any(|(_, outcome)| matches!(outcome, RecipientOutcome::Accepted { .. }));
    let failure = outcomes.iter().find_map(|(_, outcome)| match outcome {
        RecipientOutcome::Deferred { response, .. } => Some(response.as_str()),
        _ => None,
    });
    let error = if accepted { None } else { failure };
    if (accepted || error.is_some())
        && let Err(record) = state.mail.record_route_result(route.id, now, error).await
    {
        tracing::warn!(error = %record, route_id = %route.id, "recording a send route result");
    }
    Ok(outcomes)
}

/// `thread.updated` to the readers of the message's thread; the change is stored either way.
async fn announce(state: &SendJobState, outbound_id: Id) {
    state.mail.announce_outbounds(&[outbound_id]).await;
}
