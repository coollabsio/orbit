# Orbit milestone-one operations runbook

This runbook covers the failure paths that need an operator decision. Exact build, first-run, proxy, backup, and restore commands live in [`.ai/DEVELOPMENT.md`](../.ai/DEVELOPMENT.md).

## Configuration

Orbit reads its configuration only from environment variables that start with `ORBIT_`. Orbit refuses to start when it finds an unknown `ORBIT_` variable. Orbit ignores variables that start with `ORBIT_DEV_` or `ORBIT_TEST_`: scripts and tests use them. The CLI flags `--database`, `--attachments`, `--backups`, `--attachment-max-file-bytes` and `--attachment-max-request-bytes` override the related variables.

- `orbit config check` validates the configuration and prints `configuration is valid`.
- `orbit config show` prints the effective configuration as `ORBIT_…=value` lines. Secrets show as `[REDACTED]`. The password in the database URL is redacted.

| Variable | Default | Notes |
|---|---|---|
| `ORBIT_ENV` | none | Required: `development` or `production`. Orbit does not start without it. Production requires an HTTPS public origin and at least one trusted proxy. Development is loopback-only. |
| `ORBIT_BIND` | `127.0.0.1` | `0.0.0.0` works for containers and Tailscale interfaces. Firewall the port. |
| `ORBIT_PORT` | `8080` | |
| `ORBIT_PUBLIC_ORIGIN` | `http://<bind>:<port>` | The exact external origin. Production requires `https://`. |
| `ORBIT_TRUSTED_PROXIES` | none (image: `0.0.0.0/0,::/0`) | Comma-separated CIDRs, for example `127.0.0.1/32,::1/128`. Production requires at least one. Orbit reads the client IP and `https` from forwarding headers only on connections from these networks. The image default trusts every address: use it only when the reverse proxy is the only way to reach port 8080. |
| `ORBIT_SHUTDOWN_DELAY_SECONDS` | `0` | 0 to 300. Use it behind a load balancer: after SIGTERM, `/health/ready` reports 503 and the server continues to serve for this time. See [Orbit Cloud](#deployment). |
| `ORBIT_DATABASE_PATH` | `data/orbit.sqlite` | The SQLite database file. Keep the database, attachment directory and backup directory separate. |
| `ORBIT_DATABASE_URL` | none | Postgres instead of SQLite, for example `postgres://orbit:password@db.internal:5432/orbit`. See [Postgres](#postgres). |
| `ORBIT_ATTACHMENTS_PATH` | `data/attachments` | |
| `ORBIT_BACKUPS_PATH` | `backups` | Must be outside the attachment directory. |
| `ORBIT_JOBS_CONCURRENCY` | `4` | |
| `ORBIT_UPLOAD_MAX_FILE_BYTES` | `26214400` (25 MiB) | |
| `ORBIT_UPLOAD_MAX_REQUEST_BYTES` | `104857600` (100 MiB) | |
| `ORBIT_METRICS_LISTEN` | none (metrics off) | A private listener, for example `127.0.0.1:9090`. Never send it through the public site. |
| `ORBIT_RATE_LIMIT_*` | see notes | Request limits and email caps. See [Request limits and email caps](#request-limits-and-email-caps). |
| `ORBIT_APP_KEY` | none | **Required.** The key for saved secrets: base64 of 32 random bytes. |
| `ORBIT_APP_KEY_FILE` | none | The path of a file that contains the app key, instead of `ORBIT_APP_KEY`. Do not set both. |
| `ORBIT_CLOUD` | `false` | `true` turns on Stripe billing for Orbit Cloud and requires the three Stripe variables below. See [Billing (Orbit Cloud)](#billing-orbit-cloud). |
| `ORBIT_STRIPE_SECRET_KEY` | none | The Stripe secret key (`sk_live_…`, or `sk_test_…` for test mode). Only with `ORBIT_CLOUD=true`. |
| `ORBIT_STRIPE_SECRET_KEY_FILE` | none | The path of a file that contains the Stripe secret key, instead of `ORBIT_STRIPE_SECRET_KEY`. Do not set both. |
| `ORBIT_STRIPE_WEBHOOK_SECRET` | none | The signing secret (`whsec_…`) of the Stripe webhook endpoint. Only with `ORBIT_CLOUD=true`. |
| `ORBIT_STRIPE_WEBHOOK_SECRET_FILE` | none | The path of a file that contains the webhook signing secret, instead of `ORBIT_STRIPE_WEBHOOK_SECRET`. Do not set both. |
| `ORBIT_STRIPE_PRICES` | none | The plans, in display order: `plan:price_id` pairs separated by commas, for example `team:price_1Abc,business:price_2Def`. Plan keys are 1 to 32 lowercase letters, digits or hyphens, and keys and prices must be unique. Only with `ORBIT_CLOUD=true`. |
| `ORBIT_TRIAL_DAYS` | `7` | How many days a new workspace without a subscription stays writable. A whole number from 0 to 365. `0` means no trial: a new workspace is read-only until it subscribes. Only with `ORBIT_CLOUD=true`. |
| `ORBIT_TRIAL_TASKS` | `50` | The most tasks (not counting the trash) a workspace holds during its trial. A whole number, 0 or more. Only with `ORBIT_CLOUD=true`. |
| `ORBIT_TRIAL_PAGES` | `20` | The most pages (not counting the trash) a workspace holds during its trial. A whole number, 0 or more. Only with `ORBIT_CLOUD=true`. |
| `ORBIT_TRIAL_MEMBERS` | `3` | The most members, pending invitations included, a workspace holds during its trial. A whole number, 1 or more (the owner counts). Only with `ORBIT_CLOUD=true`. |

Create the app key once with `openssl rand -base64 32` and keep a copy outside the server. Orbit does not start without it. See [Email, registration and the app key](#email-registration-and-the-app-key).

The root user configures outgoing email in **Admin → Settings** and S3 storage in **Admin → Storage**, not with variables.

## Startup fails before HTTP binds

Orbit deliberately binds last. Read the first error and keep the process stopped.

1. Run `orbit config check`.
2. Run `orbit config show` and verify paths, origin, bind address, and trusted proxies. The database file must remain outside attachment and backup storage. Secrets remain redacted.
3. Run `orbit migrate status`.
4. Verify the newest backup with `orbit backup verify <id>`.
5. Fix ownership or capacity on the database, attachment, and backup paths.
6. Start Orbit again. Never delete SQLite WAL files while the process runs.

## Integrity or readiness fails

`/health/ready` returns 503 when a required check fails. Orbit also runs a durable weekly full SQLite integrity and foreign-key check (on Postgres it only checks the connection). A failed or interrupted full check stops normal HTTP traffic and exits the process; an unfinished integrity job forces another full check before a restart can bind. Remove the instance from proxy traffic and preserve the current data directory.

Do not attempt automatic repair. Verify a known-good backup, restore it with the documented stopped-server procedure, and retain the damaged copy for investigation. `/health/live` may remain 200 because it reports process liveness, not data safety.

## A background service fails

Retention, reconciliation, scheduler, or integrity supervision failure cancels normal serving. Weekly integrity checks and retention use durable schedules, so an overdue run is materialized immediately after restart. Backups are manual. The process stops accepting HTTP and tells workers to stop claiming jobs immediately, then bounds the combined drain to 30 seconds. Fix the reported database or storage condition, verify backups, and restart. Durable job leases recover after expiry; do not edit the jobs table.

## Restore rollback or ownership error

Restore refuses an active database ownership lock (the `<database>.lock` file next to the database; do not delete it while Orbit runs). Stop every Orbit process that uses that file. If restore reports rollback failure, do not start Orbit. Preserve the target and rollback paths named by the error and recover from a separate verified snapshot.

On Postgres (`ORBIT_DATABASE_URL`), the ownership lock is a Postgres advisory lock. Each server holds it shared; `backup restore`, `migrate run --reset` and `db-reset` need it exclusive, so stop every Orbit server that uses the database first. A server whose lock connection drops (network, Postgres failover) stops with `lost the database ownership lock`; start it again. A Postgres snapshot holds one `COPY` file per table (`database/<table>.copy`) instead of `database.sqlite`. Restore replaces every table in one transaction: if it fails, the database is unchanged. A snapshot restores only into the same kind of database it came from.

## Lost or expired setup token

Run `orbit setup-token rotate` (on SQLite, stop Orbit first; on Postgres it works while Orbit runs). Share the new setup URL directly with the installation owner; the account created there becomes the root user. Rotation invalidates the unused old token. Once setup completes, setup cannot run again.

## Root user and Admin area

The account created at setup is the root user. **Admin** in the sidebar (`/admin`) has accounts (suspend, reinstate, password recovery links), the global audit log with CSV export, backups, and instance settings (email, registration).

The root user can make other accounts **instance admins** (Admin → Users → Manage → Make admin). Admins see the same Admin area, but they cannot suspend or reset the root account or another admin, and they cannot choose admins. Nobody can suspend, reset or demote the root account. Removing an admin takes their Admin access away at once.

## Choosing the root user from the server

When nobody can fix the root user in the web app (the root account is suspended, the wrong account became root after an upgrade, or the root person left), use the CLI on the server. Like `recovery-link`, on SQLite it needs Orbit stopped, because a running Orbit owns the database file (on Postgres it works while Orbit runs):

```sh
orbit root show
orbit root set --email new-root@example.com
```

With the Compose deployment:

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml stop orbit
docker compose --env-file deploy/.env -f deploy/compose.yaml run --rm orbit root set --email new-root@example.com
docker compose --env-file deploy/.env -f deploy/compose.yaml start orbit
```

`root set` makes the account root, gives it Admin access and reinstates it if it was suspended. The previous root user stays an instance admin; remove that in Admin → Users if needed. The change is in the audit log as `instance.root_changed`. On upgrade, Orbit makes the oldest account with Admin access the root user, which is the account created at setup.

## Email, registration and the app key

Orbit sends email only for invitations, open-registration sign-up links and password reset links. The root user saves the SMTP server in **Admin → Settings → Email** and checks it with **Send test email**. Without a mail server, invitations are shared as links and password recovery works as described below.

Registration is closed after setup: people join only by invitation. The root user can turn on **Open registration** in **Admin → Settings** once a mail server or a [sign-in provider](#sign-in-with-google-and-github) is saved. A new person then enters their email, opens the emailed link (60 minutes, one use) and chooses a name and password. Removing the mail server closes registration.

Saved secrets (the SMTP password, GitHub App keys, Notion import tokens) are encrypted with the app key. `ORBIT_APP_KEY` (base64 of 32 random bytes, from `openssl rand -base64 32`) is required. Orbit does not create a key. Keep a copy outside the server and its backups: a database restored without the same key cannot read its saved secrets, and the root user must enter them again. Users with two-factor authentication must set it up again.

Older versions created an `app.key` file beside the SQLite database. If that file still exists, Orbit starts only when `ORBIT_APP_KEY` holds the same key. Set `ORBIT_APP_KEY` to the contents of the file, start Orbit, and then you can delete the file.

## Sign in with Google and GitHub

People can sign in with a Google or GitHub account when the root user configures the provider. A provider that is not configured does not show on the sign-in pages.

Each provider needs an OAuth client. Register this callback URL at the provider. Replace `{ORBIT_PUBLIC_ORIGIN}` with the public origin of the server:

- Google: `{ORBIT_PUBLIC_ORIGIN}/api/v1/auth/providers/google/callback`
- GitHub: `{ORBIT_PUBLIC_ORIGIN}/api/v1/auth/providers/github/callback`

To create the Google client:

1. Open the Google Cloud console and select or create a project.
2. Go to **APIs & Services → OAuth consent screen**. Configure the app name and the support email. Add the scopes `openid`, `email` and `profile`.
3. Go to **APIs & Services → Credentials → Create credentials → OAuth client ID**.
4. Select **Web application**. Add the Google callback URL to **Authorized redirect URIs**.
5. Copy the client ID and the client secret.

To create the GitHub client:

1. Open GitHub **Settings → Developer settings → OAuth Apps → New OAuth App**. For an organization, use the organization settings.
2. Set **Homepage URL** to the public origin. Set **Authorization callback URL** to the GitHub callback URL.
3. Register the app. Generate a client secret.
4. Copy the client ID and the client secret.

Then open **Admin → Sign-in providers**. Enter the client ID and the client secret, and save. Orbit encrypts the secret with the app key. To change only the client ID, leave the secret empty: Orbit keeps the saved secret. **Remove** stops the provider. Accounts keep their connections, so if you save the provider again, these accounts can sign in with it again.

Orbit uses these rules:

- Orbit finds an account by the provider account ID, not by the email. A changed provider email does not change the account.
- A first sign-in connects to the Orbit account with the same email only if the provider verified that email. Google must report the email as verified. GitHub must have a verified primary email.
- If no account has that email, Orbit creates an account only when **Open registration** is on. Open registration does not need a mail server when a sign-in provider is configured. A new account has no password. The person can add one in the profile settings within 10 minutes of a sign-in.
- An account with two-factor authentication must still enter its code after the provider sign-in.
- People connect and disconnect providers in their security settings. Orbit does not let a person remove the last sign-in method (password, passkey or provider).

### Native app redirects

The mobile app receives the result at `orbit://auth/callback`. Orbit accepts no other redirect in production. For Expo Go in development, set `ORBIT_DEV_NATIVE_REDIRECTS` to the exact Expo URLs, separated by commas, for example `exp://100.64.0.5:8082/--/auth/callback`. Orbit compares each URL exactly. Orbit does not start when `ORBIT_ENV=production` and this variable is set.

## Postgres

With `ORBIT_DATABASE_URL`, the database is on another host. `ORBIT_APP_KEY` encrypts saved secrets, the S3 secret among them, so every server and every new server needs the same key. Keep it with the database credentials.

Files and backups still go to `ORBIT_ATTACHMENTS_PATH` and `ORBIT_BACKUPS_PATH` on this server until the root user saves a bucket in **Admin → Storage** and turns on attachments and backups in S3. Do that to make the server replaceable; pre-migration backups always stay local.

Several servers can share one Postgres database; see [Orbit Cloud](#orbit-cloud-one-postgres-several-orbit-servers).

## Orbit Cloud: one Postgres, several Orbit servers

Orbit Cloud is one shared installation: one Postgres database and several Orbit servers behind a load balancer. Each server is the same binary (backend and embedded frontend) with the same configuration. All customers use the same database:

- A customer's data is kept apart by workspace. Accounts are global, so one person can be a member of workspaces of several customers with one login.
- The root user and the instance admins are the Orbit Cloud operators, not customers.
- Instance settings (email, open registration, storage, automatic backups) apply to every customer. Customers join by open registration or by invitation.

A request, a live socket or a co-editing socket can go to any server; sticky sessions are not necessary. SQLite stays one server.

### Deployment

- **Postgres:** one database with failover (a managed service, or Patroni). Connect directly or through a pooler in session mode: Orbit uses advisory locks and `LISTEN`/`NOTIFY`, which a pooler in transaction mode (PgBouncer `pool_mode = transaction`) breaks. Give each server its share of `max_connections` (each server uses its pools plus two connections: the ownership lock and the cluster listener).
- **Configuration on every server:** the same `ORBIT_DATABASE_URL`, `ORBIT_APP_KEY` and `ORBIT_PUBLIC_ORIGIN`. `ORBIT_TRUSTED_PROXIES` holds the load balancer addresses.
- **Storage:** save a bucket in **Admin → Storage** and turn on attachments and backups in S3 before the second server starts. The local disk of a server then holds only temporary files (upload staging, backup downloads), so it can be an ephemeral volume. While other servers run, Orbit refuses to move attachments back to local disks (409 `storage_shared`), and a server that hears other servers while attachments are local logs a warning.
- **Load balancer:** check `/health/ready` and remove a server on 503. Keep WebSockets open for a long time (idle timeout of 1 hour or more) for `/api/v1/workspaces/{id}/events`, the live socket (`/api/v1/workspaces/{id}/live`) and the co-editing socket of a page.
- **Stopping a server:** set `ORBIT_SHUTDOWN_DELAY_SECONDS` (0 to 300, default 0) to a little more than the load balancer's health check interval. After SIGTERM, `/health/ready` reports 503 (`accepting`) at once and the server still serves for that long, then drains for up to 30 s. Give the orchestrator a stop timeout of at least the delay plus 30 s.
- **Upgrades:** run `orbit migrate run` once (or let the first new server do it; a lock makes the others wait), then replace the servers one at a time. A migration must keep the previous release working until all servers run the new one: old servers stay ready on a newer schema, but an old server cannot start on it.
- **CLI on a running cluster:** `root`, `recovery-link`, `setup-token`, `migrate status` and `backup create` work while the servers run. They refuse a database that needs migrations (run `orbit migrate run`). `backup restore`, `migrate run --reset`, `db-reset` and `attachments compress` need every server stopped.
- **Restore:** stop every server before `backup restore` (see [Restore rollback or ownership error](#restore-rollback-or-ownership-error)). This is a full outage; choose the backup schedule for it.
- **Write throughput:** every write transaction takes one Postgres advisory lock, so writes run one at a time for the whole database. More servers give more read and socket capacity and availability, not more write capacity.

### How the servers share state

| Area | How |
|---|---|
| Ownership | Each server holds the ownership advisory lock shared, checks its lock connection every 5 s and stops when it is lost. Restore and reset hold it exclusive. |
| Cluster messages | `LISTEN`/`NOTIFY` on the channel `orbit_cluster` (`apps/server/src/cluster.rs`). A server subscribes before it accepts connections, beats every 5 s, and counts as stopped after 20 s without a beat. A lost listener connection makes the server read the database again. |
| Live socket (chat, presence, typing) | Numbered events are in `live_events` (the newest 1000 for each workspace), so a client can reconnect to any server and get a replay. Signals and presence go through cluster messages. |
| Co-editing | Each server has its own room for an open page; small updates and cursors go to the others at once. The update log is the truth: before a server writes, projects or compacts a document, it reads what the others logged, in the same serialized write transaction. |
| Sign-in | 2FA and passkey steps are in `auth_pending`; failed sign-ins and registrations in `auth_throttle_emails` and `auth_throttle_ips`, so a lockout holds on every server. |
| Request limits | The authentication, recovery/registration and invitation classes count in `rate_limit_windows` (one atomic upsert per request and subject, without the write lock), so their limits hold for the cluster. The other classes count in each server's memory (see [Limits](#limits)). |
| Email caps | Registration, password reset and invitation emails are logged in `email_sends` for 24 hours; the per-address, per-workspace and per-inviter caps hold for the cluster. |
| Jobs | Durable jobs are claimed in the database; a job kind's limits hold for all servers (see [Background job limits](#background-job-limits)). Schedules are unique per job kind. Push notifications are claimed once. |
| Storage settings | A save in **Admin → Storage** tells the other servers; each also compares the saved settings every 5 s. |
| One server at a time | Image compression takes a cluster-wide lock. A Notion import cancel checks the jobs table for a running import. |

### Background job limits

Two settings control how many background jobs run at the same time:

- **Per server:** `jobs.concurrency` (`ORBIT__JOBS__CONCURRENCY`, default 4) is the number of jobs one server runs at the same time. It sets the size of the server, so it stays in the configuration file and a change needs a restart.
- **Per job kind:** in **Admin → Background jobs**, the root user sets two limits for each job kind that customers start (Notion scans and Notion imports):
  - **All servers:** the number of jobs of the kind that run at the same time on all servers together. Default: 2.
  - **Per workspace:** the number of jobs of the kind that one workspace runs at the same time while a different workspace has a job of the kind waiting. Default: 1. When no other workspace waits, one workspace can use all the slots of the kind. Thus, one customer cannot stop the imports of the other customers.

  An empty field means no limit. **Reset to defaults** puts the values from the code back in the form. Save to apply them.

The limits are in the `job_limits` table. Each server reads the table each time it starts a job, so a change applies to all servers at the next job start. A restart is not necessary. A job that already runs continues. A saved value that is the same as the default removes the row of the kind, so a later change of the default in the code applies to it. Each save writes `instance.job_limits_updated` to the audit log. The API is `GET` and `PUT /api/v1/admin/jobs/limits` (root user only).

The maintenance jobs (retention, integrity check, scheduled backup, Notion cleanup) always run one at a time. You cannot change their limit.

A job counts against a limit until it ends or its lease expires (5 minutes without a heartbeat). If a server stops suddenly, its jobs hold their slots until their leases expire.

### Limits

- **Rate limits** (`ORBIT_RATE_LIMIT_*`, see [Request limits and email caps](#request-limits-and-email-caps)): the authentication, recovery/registration and invitation limits hold for the cluster. The upload, chat and general limits and the co-editing connection limits count on each server: with N servers the effective limit is N times the setting. A database write for each of those requests would put every request of the cluster through the one writer (see **Write throughput**), so they stay in memory. Add a request limit at the load balancer if you need a cluster-wide one. The sign-in lockout and the email caps are shared.
- **Event order:** the events of one server's writes keep their commit order. Two writes to the same workspace on two servers in the same moment can reach clients in the other order; the next event or a reload corrects the view.
- **A server that stops without a beat:** its members show as online for up to 20 s more.
- **Page lock:** a lock or unlock closes the page's sockets on every server; an edit that another server receives in the same moment can still be saved.

### Limits per account and workspace

The root user and the instance admins set two limits in **Admin → Settings → Limits**. Workspace owners and workspace admins cannot change them (403 `installation_admin_required`). They apply to every installation, also SQLite. Empty or 0 is unlimited. A lower limit refuses only new work: nothing that exists is removed.

- **Workspaces per account** (default 10): how many workspaces one account may own, not counting workspaces in the trash. Creating a workspace, restoring one from the trash and transferring ownership to an account at the limit fail with 409 `workspace_limit_reached`. The root user and the instance admins are not limited.
- **Storage per workspace** (default 1 GB): the bytes of files one workspace stores. This includes task and comment attachments, page and chat files, custom emoji and stickers. A file is counted once per workspace, however often it is attached. Profile pictures belong to the account and do not count. An upload that would go over the quota fails with 413 `storage_quota_exceeded` before Orbit stores the file. Members see the use and the quota in **Settings → General**.

Both checks run in the serialized write transaction, so they hold for concurrent requests on every server. Orbit calculates storage use from the stored files (`attachment_blobs`), so existing data counts at once. A deleted file stops counting when Orbit removes it from storage: about a day after its last reference goes. Files of trashed tasks, pages and messages count until the trash is emptied or purged.

1 GB is enough for a small team: it holds 40 files at the largest upload size (25 MB). With 10 workspaces per account, it limits one open-registration account to 10 GB. Raise the quota for customers who need more. Installations that kept the old 5 GB default got 1 GB with migration 0059; a quota the admin chose stays.

## Billing (Orbit Cloud)

With `ORBIT_CLOUD=true`, every workspace needs a Stripe subscription to stay writable after its trial (7 days by default, `ORBIT_TRIAL_DAYS`). Each workspace pays one flat price for its plan; the plan is the Stripe price in `ORBIT_STRIPE_PRICES`. There is no free plan. A self-hosted server (`ORBIT_CLOUD` not set) has no billing routes, no billing page and no read-only check.

- **Access.** A workspace whose subscription is `active`, `trialing` or `past_due` (Stripe still retries the card) has full access. Without one, a workspace is on a trial for `ORBIT_TRIAL_DAYS` days (default 7) from its creation: it is writable, but holds at most `ORBIT_TRIAL_TASKS` tasks (default 50) and `ORBIT_TRIAL_PAGES` pages (default 20), not counting the trash, and `ORBIT_TRIAL_MEMBERS` members (default 3, pending invitations count). A change applies to every workspace without a subscription at the next restart, including ones already on a trial. A member's create past a limit (a task, a page, a page copy, an invitation) fails with 402 `trial_limit_reached`. The limits are checked before the write, so concurrent creates can pass them by a few. Integrations and Notion imports are not limited. After the trial, without a subscription or with any other status (`canceled`, `unpaid`, `paused`, `incomplete`…), the workspace is read-only: members can read, search and export everything, but a member's write fails with 402 `workspace_read_only` (anyone else gets the usual 401 or 404). Co-editing sockets still show documents and drop edits; a socket that was editing when the workspace became read-only closes, on every server, and the editor reconnects read-only. Writes that stay open: subscribing (**Settings → Billing**), deleting or restoring the workspace, transferring ownership, removing members or leaving, revoking invitations, creating and revoking API tokens, disconnecting GitHub, cancelling a Notion import, and the reader's own read markers and list preferences. Integrations do not write: a GitHub `issues` or `pull_request` webhook is answered with 202 and dropped (installation events still update the connected repositories), a Discord event gets 402 `workspace_read_only`, and a running Notion import fails with a read-only message.
- **Subscribing.** The workspace owner chooses a plan in **Settings → Billing**. Orbit creates a Stripe customer for the workspace and sends the owner to Stripe Checkout. Afterwards the owner opens the Stripe Billing Portal from the same page to change the card, the plan, or cancel. Other members see the status only.
- **Webhook.** In the Stripe Dashboard (Workbench → Webhooks), add a destination with the **Snapshot** payload for `https://<public origin>/api/v1/billing/stripe/webhook` and these events: `checkout.session.completed`, `customer.subscription.created`, `customer.subscription.updated`, `customer.subscription.deleted`. Put its signing secret in `ORBIT_STRIPE_WEBHOOK_SECRET`. Orbit checks the signature (at most 5 minutes old), records each event once, and queues a `billing.stripe_sync` job that reads the subscription again from Stripe. Events that arrive late, twice or out of order therefore all end in Stripe's current state. A failed job retries with backoff for several hours; see **Admin → Background jobs**.
- **Billing Portal.** Turn on the customer portal in the Stripe Dashboard (Settings → Billing → Customer portal) in each mode you use. Without a saved portal configuration, Stripe refuses to open the portal and Orbit answers 502 `stripe_unavailable`.
- **Your own workspace.** The root user's first workspace needs no subscription: it always has full access, with no trial and no limits. Orbit marks it when the root user creates it (at setup, or later if the root user had none). On an installation that existed before this rule, the upgrade marks the oldest workspace the root user owns, including one in the trash. Other workspaces of the root user pay like any other workspace. Changing the root user (`orbit root set`) does not move the mark.
- **Products and prices.** `STRIPE_SECRET_KEY=sk_… scripts/stripe-setup.sh` creates the Orbit product with a monthly and an annual price per workspace, and the default Billing Portal configuration (customers can switch between monthly and annual, update their card, and cancel), and prints `ORBIT_STRIPE_PRICES`. Run it once in each mode: the price ids differ, and a second run creates nothing new. With `ORBIT_WEBHOOK_URL` set, it also creates the webhook and prints its signing secret.
- **Local testing.** Use test-mode keys (`sk_test_…`) and forward events with the Stripe CLI: `stripe listen --forward-to http://127.0.0.1:8080/api/v1/billing/stripe/webhook`. The CLI prints the signing secret (`whsec_…`) to put in `ORBIT_STRIPE_WEBHOOK_SECRET`. Pay with the test card `4242 4242 4242 4242`.
- **API version.** Orbit sends `Stripe-Version: 2026-09-30.endive` with each request. Webhook events use the endpoint's own API version; Orbit only reads ids from them, so that version does not matter.

## Object storage (S3), backups and a lost server

The root user saves an S3-compatible bucket (AWS S3, Cloudflare R2, MinIO, …) in **Admin → Storage**. Orbit writes, reads and deletes a test object before it saves the bucket. The secret key is encrypted with the app key. Then three settings apply:

- **Attachments in S3**: new files go to the bucket, and existing files move there in the background (the page shows how many are left). Turning it off moves them back to this server. Uploads are still staged on the local disk first. The bucket can be removed only when no attachment file is left in it.
- **Backups in S3**: each new backup is uploaded to `backups/snapshots/{id}/` in the bucket (manifest last) and removed from this server. The Backups page lists local and S3 backups; download and restore fetch an S3 backup into `<backups>/downloads/` and verify its checksums first. Pre-migration backups stay local.
- **Automatic backups**: off, every hour or every day. Retention keeps the newest backup of each of the last 7 days and 4 weeks, local and in S3.

With attachments in S3, a backup holds the database (and the files still on this server), not the bucket's files. So Orbit does not delete an unused file from the bucket at once: it keeps it 40 days, longer than any kept backup, so a restored older database still finds its files.

When the server is lost, restore on a new server from the bucket. The command line reads the bucket from environment variables, not from the (lost) database:

```sh
export ORBIT_BACKUP_S3_ENDPOINT=https://s3.eu-central-1.amazonaws.com ORBIT_BACKUP_S3_BUCKET=orbit-files \
  ORBIT_BACKUP_S3_ACCESS_KEY_ID=… ORBIT_BACKUP_S3_SECRET_ACCESS_KEY=…   # optional: _REGION, _PREFIX, _PATH_STYLE=true
orbit backup list            # the last column says local or s3
orbit backup restore <id>
```

Set the same `ORBIT_APP_KEY` on the new server before you start Orbit: without it, Orbit cannot read the saved S3 secret and does not start while a bucket is saved. Data written after the last backup is lost, so choose the schedule by how much data you can lose.

### A workspace's own bucket

Workspace owners and admins can connect their own bucket in **Settings → File storage** (permission `storage.manage`). The workspace's attachment files (`blobs/{workspace_id}/…` under the bucket's prefix) then go to that bucket, whatever Admin → Storage says. Backups stay with the installation.

- The endpoint must be `https` on a public internet address. Orbit refuses local and private addresses, also when a host name resolves to one, so a workspace cannot make the server reach its own network. A team with a MinIO on a private network cannot use it here; the root user can use it in Admin → Storage instead.
- Orbit tests the bucket like the installation's bucket. It cannot use the installation's bucket and prefix.
- After the save, existing files move from this server's disk and the installation's bucket to the workspace's bucket in the background, as when the root user turns on attachments in S3. The page says "Checking which files to move…" until the first pass counted them, then shows how many are left. Files stay readable during the move.
- **Remove** sends new files to the installation's storage again. The files in the workspace's bucket move back and are deleted from it. When none are left, the bucket disappears from the settings. To change the endpoint, bucket or prefix, remove the bucket first and wait for that.
- As with the installation's bucket, Orbit keeps an unused file in the workspace's bucket for 40 days. If the bucket fails (for example, revoked keys), other workspaces are not affected: reads try the other tiers, and cleanup skips that bucket until it works again.
- Files in a workspace's bucket still count toward the workspace's storage quota.
- When a workspace is deleted for good, its files in its own bucket stay there.

## Large images

The web client makes a JPEG, PNG or WebP of 512 KiB or more smaller before upload: at most 2560 px on the long edge, re-encoded as WebP. GIF, SVG and animated images stay as they are.

To do the same to images that were uploaded before, the root user clicks **Compress images** in **Admin → Storage**. It runs in the background while Orbit serves, for files on this server and in S3, and the page shows its progress. Each file keeps its task, page and chat references; the file names change to `.webp`. The originals are replaced: make a backup first if you want to keep them. The old file is released like any unused file, so in S3 it stays 40 days for older backups. Running it again does not change the result.

While Orbit is stopped, the same works from the command line for files on this server (files in S3 are skipped):

```sh
orbit attachments compress --dry-run   # reports the savings, changes nothing
orbit attachments compress
```

## Password recovery without SMTP

The root user opens **Admin → Users**, selects **Manage → Create password recovery link** for the account and shares the 30-minute link through an authenticated channel. The root user changes their own password in Profile instead.

If the root user lost their own password, use (on SQLite, after you stop Orbit) `orbit recovery-link --email <email> --origin <https-origin>`. Never log a link. Unknown or suspended accounts do not receive a link.

## Security checks after proxy changes

- An HTTPS request through a trusted proxy must receive HSTS.
- A login response must use `__Host-orbit_session` with `Secure`, `HttpOnly`, `SameSite=Lax`, and no `Domain`.
- Requests from untrusted peers cannot supply trusted request IDs or forwarded transport.
- Unsafe API calls with a missing or different `Origin` must return `origin_forbidden`.
- Unknown `/api/*` paths must return `application/problem+json`, never SPA HTML.
- `/api` itself and wrong methods on known API routes must return Problem Details.
- Production mode requires an HTTPS public origin and at least one explicitly trusted proxy. Requests without a verified HTTPS transport are rejected. Development mode is loopback-only.
- Tune the `ORBIT_RATE_LIMIT_*` variables only after observing the endpoint-class defaults (see [Request limits and email caps](#request-limits-and-email-caps)). Limit responses are correlated `429` Problem Details with `Retry-After`; IPv6 clients are grouped by `/64`. The in-memory limiter keeps at most 16,384 entries; when it is full it forgets the entry with the fewest requests, so a flood of new addresses never locks out new clients and does not reset a client at its limit.
- If metrics are enabled, the configured private `/metrics` listener must be reachable only by the monitoring network and must not appear on the public listener.

## Request limits and email caps

Set them with `ORBIT_RATE_LIMIT_<KEY>`, the key in upper case (for example `ORBIT_RATE_LIMIT_GENERAL_PER_MINUTE`).

| Key | Default | What it limits |
|---|---|---|
| `authentication_per_minute` | 30 | Sign-in, sessions, setup and OAuth requests. Cluster-wide. |
| `recovery_per_minute` | 10 | Password reset and open registration requests. Cluster-wide. |
| `invitation_per_minute` | 60 | Invitation requests. Cluster-wide. |
| `upload_per_minute` | 120 | Attachment and chat file uploads. Per server. |
| `general_per_minute` | 600 | Every other API and MCP request (chat: 3 times this). Per server. |
| `workspace_multiplier` | 10 | A workspace may make this many times each limit above. |
| `account_email_cooldown_seconds` | 60 | Time between two registration or password reset emails to one address. |
| `account_emails_per_day` | 5 | Registration and password reset emails to one address in 24 hours. |
| `invitation_emails_per_workspace_per_day` | 100 | Invitation emails of one workspace in 24 hours. |
| `invitation_emails_per_inviter_per_day` | 50 | Invitation emails one person sends in 24 hours, in all workspaces. |

- **Who a request counts against:** first its client IP address, then its credential (the session cookie or the `Authorization` header: a session, an API token, an MCP token), then, for a request with a credential, the workspace in its path (`/api/v1/workspaces/{id}/…`). The first one over its limit refuses the request with `429 rate_limit_exceeded`, and the ones after it do not count it. So a client without a credential cannot use up a workspace's budget, and one address can use at most its own limit of it.
- **Account emails:** registration and password reset links to one address share the cooldown and the daily cap, on every server. Over the cap, Orbit sends nothing and gives the usual `202` answer, so the answer does not tell whether an account exists. Requests for which no email goes out (an unknown address for a reset, an existing account for registration) do not count.
- **Invitation emails:** an email invitation over a cap is taken back and answered with `429 invitation_email_limit` and `Retry-After`. Invitation links (`"delivery": "manual"`) have no cap. The subject is always "You have been invited to Orbit"; the body names the workspace on one line, in quotes, without control or direction characters, and at most 60 characters long. The inviter's name is not in the email.
- Values: the per-minute limits 1 to 1,000,000; `workspace_multiplier` 1 to 1000; the cooldown 0 to 86,400 seconds; the daily caps 1 to 100,000.

## Monitoring and operational ownership

Existing endpoints:

- `GET /health/live` — process liveness
- `GET /health/ready` — serving readiness; treat 503 as take-out-of-proxy
- Optional private `GET /metrics` when `ORBIT_METRICS_LISTEN` is set

Alerting should live in the operator's existing monitor (systemd, Caddy, Prometheus, host disk checks). Do not scrape `/metrics` through the public origin.

Suggested thresholds:

- Readiness 503 for more than 2 minutes → page the primary
- Backup directory missing a verified snapshot newer than 36 hours → page the primary
- Filesystem for database/attachments/backups above 85% → ticket the fallback owner

Ownership template (fill with real names before rollout):

| Area | Primary | Fallback |
|---|---|---|
| Account recovery / invitations | | |
| Backups and restore rehearsal | | |
| Upgrades and rollback | | |
| Incident response | | |

Automated subset: `ORBIT_VERIFY_ORIGIN=https://… deploy/verify-production.sh`. Restore rehearsal: `deploy/backup-rehearsal.sh <backup-id> /path/to/scratch` (with the same `ORBIT_*` variables as the server). Both require an operator-chosen host and off-host backup destination; they do not complete Tasks 10, 11, or 14 by themselves.
