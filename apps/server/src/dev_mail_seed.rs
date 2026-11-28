//! Development mail: a verified `orbit.test` domain, a personal mailbox for each seed user, a shared
//! Support mailbox, and a few conversations in each, so the mail screens can be tried at once.
//! Safe to run again: the domain, the route and the mailboxes are found by name, and mail goes only
//! to an empty mailbox. The domain sends through an instance SMTP route to the local Mailpit
//! (AGENTS.md), so sent mail shows in its inbox.

use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::sync::Arc;

use orbit_mail::{AuthOutcome, DmarcVerdict, Verdict};
use orbit_platform::{
    AttachmentMutationCoordinator, Database, Id, LocalBlobStore, TimestampMillis, UploadLimits,
    UploadService,
};

use crate::mail_intake::{InboundMessage, MailIntake, Recipient, Source};
use crate::repositories::instance_settings::InstanceSettingsRepository;
use crate::repositories::mail::{
    DnsReport, MailRepository, MailboxKind, NewMailbox, NewRoute, ProviderScope, RecordStatus,
    RouteSecrets, RouteSettings, SendRouteKind, SmtpSecurity,
};

pub const DOMAIN: &str = "orbit.test";
const REQUEST_ID: &str = "development-seed";
const MAX_MESSAGE_BYTES: u64 = 1024 * 1024;
const ROUTE_NAME: &str = "Mailpit (development)";
/// Mailpit's SMTP port as AGENTS.md starts it.
const MAILPIT_PORT: u16 = 11025;

type SeedError = Box<dyn std::error::Error + Send + Sync>;

/// One mailbox of the seed: its primary address and what arrives in it.
struct SeedMailbox {
    address_id: Id,
    local_part: &'static str,
}

/// Installs the mail seed. `attachments` is the server's local file directory: the raw messages are
/// stored there, where the server reads them too.
pub async fn seed_mail(
    database: &Database,
    app_key: [u8; 32],
    attachments: &Path,
    workspace_id: Id,
    developer: Id,
    member: Id,
) -> Result<(), SeedError> {
    let now = TimestampMillis::now();
    InstanceSettingsRepository::new(database.clone())
        .set_mail_enabled(developer, true, REQUEST_ID, now)
        .await?;
    let uploads = UploadService::new(
        database.clone(),
        Arc::new(LocalBlobStore::new(attachments)),
        AttachmentMutationCoordinator::default(),
        UploadLimits::default(),
    );
    let mail = Arc::new(MailRepository::new(database.clone(), uploads, app_key));

    let domain_id = match id_of(
        database,
        "SELECT id FROM mail_domains WHERE workspace_id = $1 AND name = $2",
        &[&workspace_id.to_string(), DOMAIN],
    )
    .await?
    {
        Some(id) => id,
        None => {
            mail.create_domain(workspace_id, developer, DOMAIN, REQUEST_ID)
                .await?
                .id
        }
    };
    // `.test` has no DNS: the domain, its records and its DKIM key count as checked. Every run sets
    // it again, because the daily DNS check marks it failing.
    let report = DnsReport {
        verify: RecordStatus::Ok,
        mx: RecordStatus::Ok,
        dkim: RecordStatus::Ok,
        spf: RecordStatus::Ok,
        dmarc: RecordStatus::Ok,
        ..DnsReport::unknown()
    };
    sqlx::query(
        "UPDATE mail_domains SET status = 'verified', dns_report = $1, \
         verified_at = COALESCE(verified_at, $2), last_verified_at = $2, checked_at = $2, \
         suspended_at = NULL, updated_at = $2 WHERE id = $3",
    )
    .bind(serde_json::to_string(&report)?)
    .bind(now.as_millis())
    .bind(domain_id.to_string())
    .execute(database.pool())
    .await?;
    sqlx::query(
        "UPDATE mail_dkim_keys SET status = 'active', updated_at = $1 \
         WHERE domain_id = $2 AND status = 'pending'",
    )
    .bind(now.as_millis())
    .bind(domain_id.to_string())
    .execute(database.pool())
    .await?;

    let route_id = match id_of(
        database,
        "SELECT id FROM mail_send_routes WHERE workspace_id IS NULL AND name = $1",
        &[ROUTE_NAME],
    )
    .await?
    {
        Some(id) => id,
        None => {
            mail.create_route(
                ProviderScope::Instance,
                developer,
                NewRoute {
                    id: None,
                    kind: SendRouteKind::Smtp,
                    name: ROUTE_NAME.to_owned(),
                    provider_id: None,
                    secrets: RouteSecrets::None,
                    settings: RouteSettings {
                        host: Some("127.0.0.1".to_owned()),
                        port: Some(MAILPIT_PORT),
                        security: SmtpSecurity::None,
                        ..RouteSettings::default()
                    },
                },
                None,
                REQUEST_ID,
            )
            .await?
            .id
        }
    };
    let own_route = id_of(
        database,
        "SELECT send_route_id FROM mail_domains WHERE id = $1 AND send_route_id IS NOT NULL",
        &[&domain_id.to_string()],
    )
    .await?;
    if own_route != Some(route_id) {
        mail.set_domain_send_route(
            workspace_id,
            developer,
            domain_id,
            Some(route_id),
            REQUEST_ID,
        )
        .await?;
    }

    let developer_membership = membership_of(database, workspace_id, developer).await?;
    let member_membership = membership_of(database, workspace_id, member).await?;
    let mailbox = async |kind, name: &str, local_part: &'static str, owner, members| {
        let address_id = match id_of(
            database,
            "SELECT id FROM mail_addresses WHERE domain_id = $1 AND local_part = $2",
            &[&domain_id.to_string(), local_part],
        )
        .await?
        {
            Some(id) => id,
            None => {
                mail.create_mailbox(
                    workspace_id,
                    developer,
                    NewMailbox {
                        kind,
                        name: name.to_owned(),
                        domain_id,
                        local_part: local_part.to_owned(),
                        owner_membership_id: owner,
                        member_ids: members,
                    },
                    REQUEST_ID,
                )
                .await?
                .addresses[0]
                    .id
            }
        };
        Ok::<_, SeedError>(SeedMailbox {
            address_id,
            local_part,
        })
    };
    let dev = mailbox(
        MailboxKind::Personal,
        "Orbit Developer",
        "dev",
        Some(developer_membership),
        Vec::new(),
    )
    .await?;
    let member_box = mailbox(
        MailboxKind::Personal,
        "Orbit Member",
        "member",
        Some(member_membership),
        Vec::new(),
    )
    .await?;
    let support = mailbox(
        MailboxKind::Shared,
        "Support",
        "support",
        None,
        vec![developer_membership, member_membership],
    )
    .await?;

    let intake = MailIntake::new(Arc::clone(&mail), MAX_MESSAGE_BYTES);
    for (target, messages) in [
        (&dev, developer_messages()),
        (&member_box, member_messages()),
        (&support, support_messages()),
    ] {
        if has_mail(database, target.address_id).await? {
            continue;
        }
        for (from, raw) in messages {
            deliver(&intake, &mail, workspace_id, target, from, raw).await?;
        }
    }
    Ok(())
}

async fn id_of(
    database: &Database,
    sql: &'static str,
    binds: &[&str],
) -> Result<Option<Id>, SeedError> {
    let mut query = sqlx::query_scalar::<_, String>(sql);
    for bind in binds {
        query = query.bind(*bind);
    }
    Ok(match query.fetch_optional(database.pool()).await? {
        Some(id) => Some(id.parse()?),
        None => None,
    })
}

async fn membership_of(
    database: &Database,
    workspace_id: Id,
    user_id: Id,
) -> Result<Id, SeedError> {
    id_of(
        database,
        "SELECT id FROM memberships WHERE workspace_id = $1 AND user_id = $2",
        &[&workspace_id.to_string(), &user_id.to_string()],
    )
    .await?
    .ok_or_else(|| "the seed user is not in the workspace".into())
}

async fn has_mail(database: &Database, address_id: Id) -> Result<bool, SeedError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mail_threads WHERE mailbox_id = \
         (SELECT mailbox_id FROM mail_addresses WHERE id = $1)",
    )
    .bind(address_id.to_string())
    .fetch_one(database.pool())
    .await?;
    Ok(count > 0)
}

/// Stores the message as the SMTP listener does (it passed SPF, DKIM and DMARC) and files it.
async fn deliver(
    intake: &MailIntake,
    mail: &MailRepository,
    workspace_id: Id,
    target: &SeedMailbox,
    from: &str,
    raw: String,
) -> Result<(), SeedError> {
    let sender_domain = from.rsplit('@').next().unwrap_or_default().to_owned();
    let rcpt = format!("{}@{DOMAIN}", target.local_part);
    let receipts = intake
        .accept(InboundMessage {
            source: Source::Smtp,
            raw: raw
                .replace("{rcpt}", &rcpt)
                .replace('\n', "\r\n")
                .into_bytes(),
            trace_len: 0,
            mail_from: Some(from.to_owned()),
            recipients: vec![Recipient::Mailbox {
                workspace_id,
                address_id: target.address_id,
                rcpt,
            }],
            peer_ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
            helo: format!("mx.{sender_domain}"),
            tls: None,
            auth: AuthOutcome {
                spf: Verdict::Pass,
                dkim: Verdict::Pass,
                dmarc: DmarcVerdict::Pass,
                aligned_domain: Some(sender_domain.clone()),
                temp_error: false,
                spf_domain: Some(sender_domain.clone()),
                dkim_domain: Some(sender_domain.clone()),
                from_domain: Some(sender_domain),
            },
        })
        .await?;
    for receipt in receipts {
        mail.ingest(receipt, TimestampMillis::now()).await?;
    }
    Ok(())
}

/// (envelope sender, raw message): `{rcpt}` is the mailbox's address. Oldest first.
fn developer_messages() -> Vec<(&'static str, String)> {
    vec![
        (
            "news@updates.example",
            "From: Orbit Weekly <news@updates.example>
To: {rcpt}
Subject: This week: keyboard shortcuts everywhere
Date: Mon, 16 Nov 2026 08:00:00 +0000
Message-ID: <weekly-46@updates.example>
MIME-Version: 1.0
Content-Type: text/html; charset=utf-8

<h1>Keyboard shortcuts everywhere</h1>
<p>Press <b>?</b> on any page to see what you can do without the mouse.</p>
<p><a href=\"https://updates.example/46\">Read the full issue</a></p>
"
            .to_owned(),
        ),
        (
            "billing@vendor.example",
            "From: Vendor Billing <billing@vendor.example>
To: {rcpt}
Subject: Invoice 2041
Date: Tue, 17 Nov 2026 10:15:00 +0000
Message-ID: <invoice-2041@vendor.example>
MIME-Version: 1.0
Content-Type: multipart/mixed; boundary=\"seed\"

--seed
Content-Type: text/plain; charset=utf-8

Hello,

the invoice for November is attached. Payment is due in 30 days.

Vendor Billing
--seed
Content-Type: application/pdf; name=\"invoice-2041.pdf\"
Content-Disposition: attachment; filename=\"invoice-2041.pdf\"
Content-Transfer-Encoding: base64

JVBERi0xLjQKJcTl8uXrp/Og0MTGCjEgMCBvYmoKPDwgL1R5cGUgL0NhdGFsb2cgPj4KZW5kb2JqCnRyYWlsZXIKPDwgL1Jvb3QgMSAwIFIgPj4KJSVFT0YK
--seed--
"
            .to_owned(),
        ),
        (
            "dana@acme.example",
            "From: Dana Whitfield <dana@acme.example>
To: {rcpt}
Cc: Lee Park <lee@acme.example>, Orbit Member <member@orbit.test>
Subject: Quarterly report draft
Date: Thu, 19 Nov 2026 09:30:00 +0000
Message-ID: <report-1@acme.example>

Hi,

the draft of the quarterly report is ready. Can you check the numbers in
section 3 before Friday? Lee has the source data if you need it.

Thanks,
Dana
"
            .to_owned(),
        ),
        (
            "lee@acme.example",
            "From: Lee Park <lee@acme.example>
To: Dana Whitfield <dana@acme.example>, {rcpt}
Cc: Orbit Member <member@orbit.test>
Subject: Re: Quarterly report draft
Date: Thu, 19 Nov 2026 11:05:00 +0000
Message-ID: <report-2@acme.example>
In-Reply-To: <report-1@acme.example>
References: <report-1@acme.example>

The source data is in the shared folder, file q3-final.xlsx.
One note: the March revenue line still has the old exchange rate.

Lee

On Thu, 19 Nov 2026, Dana Whitfield wrote:
> the draft of the quarterly report is ready. Can you check the numbers in
> section 3 before Friday?
"
            .to_owned(),
        ),
        (
            "sam@customer.example",
            "From: Sam Rivera <sam@customer.example>
To: {rcpt}
Subject: Demo next week?
Date: Fri, 20 Nov 2026 14:20:00 +0000
Message-ID: <demo-1@customer.example>

Hello,

could we do a 30 minute demo next Tuesday or Wednesday afternoon?

Best,
Sam
"
            .to_owned(),
        ),
    ]
}

fn member_messages() -> Vec<(&'static str, String)> {
    vec![(
        "dana@acme.example",
        "From: Dana Whitfield <dana@acme.example>
To: {rcpt}
Subject: Welcome aboard
Date: Fri, 20 Nov 2026 09:00:00 +0000
Message-ID: <welcome-1@acme.example>

Welcome to the project! Your access to the shared folder is ready.
"
        .to_owned(),
    )]
}

fn support_messages() -> Vec<(&'static str, String)> {
    vec![(
        "sam@customer.example",
        "From: Sam Rivera <sam@customer.example>
To: {rcpt}
Subject: Cannot sign in after the update
Date: Sat, 21 Nov 2026 07:45:00 +0000
Message-ID: <support-1@customer.example>

Hi,

since this morning's update the sign-in page says my password is wrong.
I reset it twice. Can you help?

Sam
"
        .to_owned(),
    )]
}
