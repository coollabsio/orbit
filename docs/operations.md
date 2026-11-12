# Orbit milestone-one operations runbook

This runbook covers the failure paths that need an operator decision. Exact build, first-run, proxy, backup, and restore commands live in [`.ai/DEVELOPMENT.md`](../.ai/DEVELOPMENT.md).

## Configuration

Orbit reads its configuration only from environment variables that start with `ORBIT_`. Orbit refuses to start when it finds an unknown `ORBIT_` variable. Orbit ignores variables that start with `ORBIT_DEV_` or `ORBIT_TEST_`: scripts and tests use them. The CLI flags `--database`, `--attachments`, `--backups`, `--attachment-max-file-bytes` and `--attachment-max-request-bytes` override the related variables.

- `orbit config check` validates the configuration and prints `configuration is valid`, then a `warning:` line for each risky setting (a trusted proxy network of `/0`, a metrics listener that is not loopback). Warnings do not change the exit code.
- `orbit config show` prints the effective configuration as `ORBIT_…=value` lines. Secrets show as `[REDACTED]`. The password in the database URL is redacted.

| Variable | Default | Notes |
|---|---|---|
| `ORBIT_ENV` | none | Required: `development` or `production`. Orbit does not start without it. Production requires an HTTPS public origin and at least one trusted proxy. Development is loopback-only. |
| `ORBIT_BIND` | `127.0.0.1` | `0.0.0.0` works for containers and Tailscale interfaces. Firewall the port. |
| `ORBIT_PORT` | `8080` | |
| `ORBIT_PUBLIC_ORIGIN` | `http://<bind>:<port>` | The exact external origin. Production requires `https://`. |
| `ORBIT_TRUSTED_PROXIES` | none | Comma-separated CIDRs of the reverse proxies, for example `127.0.0.1/32,::1/128`. Production requires at least one, also in the image. Orbit reads the client IP and `https` from forwarding headers only on connections from these networks. In the image, add `127.0.0.1/32`: the container healthcheck connects from loopback. Do not trust `0.0.0.0/0` or `::/0`: anyone who reaches the port could then fake their IP address (and get around rate limits) and HTTPS. Orbit logs a warning at startup for such a network. |
| `ORBIT_SHUTDOWN_DELAY_SECONDS` | `0` | 0 to 300. Use it behind a load balancer: after SIGTERM, `/health/ready` reports 503 and the server continues to serve for this time. See [Orbit Cloud](#deployment). |
| `ORBIT_DATABASE_PATH` | `data/orbit.sqlite` | The SQLite database file. Keep the database, attachment directory and backup directory separate. |
| `ORBIT_DATABASE_URL` | none | Postgres instead of SQLite, for example `postgres://orbit:password@db.internal:5432/orbit`. See [Postgres](#postgres). |
| `ORBIT_ATTACHMENTS_PATH` | `data/attachments` | |
| `ORBIT_BACKUPS_PATH` | `backups` | Must be outside the attachment directory. |
| `ORBIT_JOBS_CONCURRENCY` | `4` | |
| `ORBIT_UPLOAD_MAX_FILE_BYTES` | `26214400` (25 MiB) | |
| `ORBIT_UPLOAD_MAX_REQUEST_BYTES` | `104857600` (100 MiB) | |
| `ORBIT_METRICS_LISTEN` | none (metrics off) | A private listener, for example `127.0.0.1:9090`. `/metrics` has no authentication: never send it through the public site. Orbit logs a warning at startup when the address is not loopback; then only the monitoring network may reach it. |
| `ORBIT_RATE_LIMIT_*` | see notes | Request limits and email caps. See [Request limits and email caps](#request-limits-and-email-caps). |
| `ORBIT_MAIL_*` | mail off | The SMTP listener that receives mail. See [Mail](#mail). |
| `ORBIT_APP_KEY` | none | **Required.** The key for saved secrets: base64 of 32 random bytes. |
| `ORBIT_APP_KEY_FILE` | none | The path of a file that contains the app key, instead of `ORBIT_APP_KEY`. Do not set both. |
| `ORBIT_CLOUD` | `false` | `true` turns on Stripe billing for Orbit Cloud and requires the three Stripe variables below. See [Billing (Orbit Cloud)](#billing-orbit-cloud). |
| `ORBIT_STRIPE_SECRET_KEY` | none | The Stripe secret key (`sk_live_…`, or `sk_test_…` for test mode). Only with `ORBIT_CLOUD=true`. |
| `ORBIT_STRIPE_SECRET_KEY_FILE` | none | The path of a file that contains the Stripe secret key, instead of `ORBIT_STRIPE_SECRET_KEY`. Do not set both. |
| `ORBIT_STRIPE_WEBHOOK_SECRET` | none | The signing secret (`whsec_…`) of the Stripe webhook endpoint. Only with `ORBIT_CLOUD=true`. |
| `ORBIT_STRIPE_WEBHOOK_SECRET_FILE` | none | The path of a file that contains the webhook signing secret, instead of `ORBIT_STRIPE_WEBHOOK_SECRET`. Do not set both. |
| `ORBIT_STRIPE_PRICES` | none | The plans, in display order: `plan:price_id` pairs separated by commas, for example `team:price_1Abc,business:price_2Def`. Plan keys are 1 to 32 lowercase letters, digits or hyphens, and keys and prices must be unique. Only with `ORBIT_CLOUD=true`. |
| `ORBIT_TRIAL_DAYS` | `7` | How many days a workspace without a subscription stays writable. The trial belongs to the owner account: it starts with the account's first workspace, and later workspaces of that account share its end. When an account is deleted or changes its email, Orbit keeps a SHA-256 of the old mailbox with the trial start (table `used_trials`), so a new account with that mailbox continues the trial. A whole number from 0 to 365. `0` means no trial: a new workspace is read-only until it subscribes. Only with `ORBIT_CLOUD=true`. |
| `ORBIT_TRIAL_TASKS` | `50` | The most tasks (not counting the trash) a workspace holds during its trial. A whole number, 0 or more. Only with `ORBIT_CLOUD=true`. |
| `ORBIT_TRIAL_PAGES` | `20` | The most pages (not counting the trash) a workspace holds during its trial. A whole number, 0 or more. Only with `ORBIT_CLOUD=true`. |
| `ORBIT_TRIAL_MEMBERS` | `3` | The most members, pending invitations included, a workspace holds during its trial. A whole number, 1 or more (the owner counts). Only with `ORBIT_CLOUD=true`. |
| `ORBIT_APNS_KEY` | none (iOS pushes off) | The text of the APNs auth key (`AuthKey_<key id>.p8`, PEM) from the Apple developer account, for native iOS push notifications. Set it with `ORBIT_APNS_KEY_ID` and `ORBIT_APNS_TEAM_ID`. See [iOS push notifications (APNs)](#ios-push-notifications-apns). |
| `ORBIT_APNS_KEY_FILE` | none | The path of a file that contains the APNs key, instead of `ORBIT_APNS_KEY`. Do not set both. |
| `ORBIT_APNS_KEY_ID` | none | The 10-character key ID of the APNs key. |
| `ORBIT_APNS_TEAM_ID` | none | The 10-character team ID of the Apple developer account. |
| `ORBIT_APNS_TOPIC` | `io.coollabs.orbit` | The bundle ID of the iOS app. |

Create the app key once with `openssl rand -base64 32` and keep a copy outside the server. Orbit does not start without it. See [Email, registration and the app key](#email-registration-and-the-app-key).

## Instance settings from environment variables

The root user saves outgoing email, S3 storage, sign-in providers, open registration and automatic backups in Admin. A declarative deployment (Kubernetes, GitOps, Docker Compose) can set them with the variables below instead. When a variable is set, it replaces the saved value, and Admin shows the setting read-only with the name of the variable. Admin answers a change of it with `409 set_by_environment`. Orbit does not write these values to the database: when you remove a variable, Orbit uses the saved value again after a restart. A setting without a variable keeps the value saved in Admin.

The variables of one group are set together. For example, `ORBIT_SMTP_HOST` needs `ORBIT_SMTP_FROM`. Orbit refuses to start when a group is not complete, and `orbit config check` shows the error. Each secret also has a `_FILE` variable that names a file with the secret, as `ORBIT_APP_KEY_FILE` does. Do not set both. `orbit config show` prints the set variables, with each secret as `[REDACTED]`.

| Variable | Default | Notes |
|---|---|---|
| `ORBIT_SMTP_HOST` | none | The mail server. Sets all of **Admin → Email**. Requires `ORBIT_SMTP_FROM`. |
| `ORBIT_SMTP_SECURITY` | `starttls` | `tls`, `starttls` or `none`. Use `none` only for a relay on the same host or private network. |
| `ORBIT_SMTP_PORT` | `465` for `tls`, `587` for `starttls`, `25` for `none` | |
| `ORBIT_SMTP_USERNAME` | none | |
| `ORBIT_SMTP_PASSWORD`, `ORBIT_SMTP_PASSWORD_FILE` | none | |
| `ORBIT_SMTP_FROM` | none | The sender address, for example `orbit@example.com`. |
| `ORBIT_SMTP_FROM_NAME` | none | The sender name, at most 120 characters. |
| `ORBIT_S3_ENDPOINT` | none | The bucket of **Admin → Storage**, for example `https://s3.eu-central-1.amazonaws.com`. Requires `ORBIT_S3_BUCKET`, `ORBIT_S3_ACCESS_KEY_ID` and `ORBIT_S3_SECRET_ACCESS_KEY`. |
| `ORBIT_S3_BUCKET` | none | 3 to 63 characters. |
| `ORBIT_S3_REGION` | `us-east-1` | |
| `ORBIT_S3_PREFIX` | none (bucket root) | The key prefix, for example `orbit`. |
| `ORBIT_S3_ACCESS_KEY_ID` | none | |
| `ORBIT_S3_SECRET_ACCESS_KEY`, `ORBIT_S3_SECRET_ACCESS_KEY_FILE` | none | |
| `ORBIT_S3_PATH_STYLE` | `false` | `true` for `endpoint/bucket/key` URLs (MinIO and most self-hosted servers). |
| `ORBIT_S3_ATTACHMENTS` | saved value | `true` or `false`: attachments in S3. `true` requires the `ORBIT_S3_*` bucket. |
| `ORBIT_S3_BACKUPS` | saved value | `true` or `false`: backups in S3. `true` requires the `ORBIT_S3_*` bucket. |
| `ORBIT_AUTOMATIC_BACKUPS` | saved value | `off`, `hourly` or `daily`. |
| `ORBIT_S3_KEEP_DELETED_DAYS` | saved value (40) | 1 to 365: the days a deleted or replaced attachment file stays in S3 before Orbit deletes it. Backups do not contain S3 files, so a restore from a backup older than this can miss files. |
| `ORBIT_OPEN_REGISTRATION` | saved value | `true` or `false`. `true` requires `ORBIT_SMTP_*` or a provider variable below. On Orbit Cloud, it requires `ORBIT_SMTP_*`. |
| `ORBIT_GOOGLE_CLIENT_ID`, `ORBIT_GOOGLE_CLIENT_SECRET`, `ORBIT_GOOGLE_CLIENT_SECRET_FILE` | none | The Google OAuth client. Set the ID and the secret together. See [Sign in with Google and GitHub](#sign-in-with-google-and-github). |
| `ORBIT_GITHUB_CLIENT_ID`, `ORBIT_GITHUB_CLIENT_SECRET`, `ORBIT_GITHUB_CLIENT_SECRET_FILE` | none | The GitHub OAuth client. Set the ID and the secret together. |
| `ORBIT_OIDC_ISSUER` | none | The issuer URL of an OpenID Connect provider, for example `https://auth.example.com/application/o/orbit/`. Requires `ORBIT_OIDC_CLIENT_ID` and `ORBIT_OIDC_CLIENT_SECRET`. See [Sign in with OpenID Connect](#sign-in-with-openid-connect). |
| `ORBIT_OIDC_CLIENT_ID`, `ORBIT_OIDC_CLIENT_SECRET`, `ORBIT_OIDC_CLIENT_SECRET_FILE` | none | The client that the provider issued to Orbit. |
| `ORBIT_OIDC_DISPLAY_NAME` | `SSO` | The name on the sign-in button ("Continue with SSO"), at most 50 characters. |

Orbit does not test the `ORBIT_S3_*` bucket at startup, as Admin does when it saves one. An unreachable bucket shows up as a move error on **Admin → Storage** and as failed backups. Orbit refuses to start when a workspace's own bucket overlaps the `ORBIT_S3_*` bucket.

### Changing the bucket that holds the attachment files

Orbit records the location (endpoint, bucket and prefix, not the keys) of the bucket that holds the attachment files: when attachments go to S3, from Admin or from `ORBIT_S3_*`. The mover clears the record when attachments are off and no file is left in that bucket. Upgrading to migration `20261111122359_files_bucket` records the bucket saved in Admin, if there is one. An empty one is cleared within 10 minutes.

At startup, Orbit compares the record with the bucket it is about to use (`ORBIT_S3_*`, else Admin → Storage). When they are different, Orbit starts only if it can prove that the recorded bucket has no attachment files:

- When the recorded bucket is the one saved in Admin, Orbit lists `blobs/` in it with the saved keys. An empty bucket is forgotten, and Orbit starts.
- Otherwise (the recorded bucket came from earlier `ORBIT_S3_*` values, whose keys Orbit never saves), or when the bucket cannot be reached, Orbit refuses to start. The error names the recorded bucket.

To change or remove the `ORBIT_S3_*` bucket while it holds the attachment files, do these steps:

1. Set `ORBIT_S3_ATTACHMENTS=false` and restart Orbit.
2. Wait until **Admin → Storage** shows no files left to move.
3. Change or remove the `ORBIT_S3_*` variables, and restart Orbit.

When the files in the recorded bucket are no longer needed (for example, the bucket was deleted), stop Orbit and run `orbit attachments forget-bucket --yes`. Orbit then forgets the bucket and does not read its files again. The audit log records `instance.files_bucket_forgotten`.

All servers that share one Postgres database must have the same variables. Each server reads only its own environment.

## Upgrade to migration 0066 (API token scopes)

Migration 0066 puts the scopes of each API token in one `scopes` column and removes the old `can_read` and `can_write` columns. The change cannot be undone in place:

- Before it applies migration 0066, Orbit creates a pre-migration backup and verifies it. If the backup or its check fails, Orbit does not migrate. Pre-migration backups stay local, in `ORBIT_BACKUPS_PATH`.
- Keep that backup. An older version cannot use the migrated database. To go back to an older version, stop Orbit, restore that backup (`orbit backup restore <id>`), then start the older version. Data written after the upgrade is lost.
- If several servers share one Postgres database, stop all of them before the upgrade. Then start the new servers. An old server that still runs after migration 0066 fails each request with an API token (public API, MCP with an API token, the Discord integration) and the **Settings → API tokens** page.

## Startup fails before HTTP binds

Orbit deliberately binds last. Read the first error and keep the process stopped.

1. Run `orbit config check`.
2. Run `orbit config show` and verify paths, origin, bind address, and trusted proxies. The database file must remain outside attachment and backup storage. Secrets remain redacted.
3. Run `orbit migrate status`.
4. Verify the newest backup with `orbit backup verify <id>`.
5. Fix ownership or capacity on the database, attachment, and backup paths.
6. Start Orbit again. Never delete SQLite WAL files while the process runs.

## Integrity or readiness fails

`/health/ready` returns 503 when a required check fails. Its database check is only a `SELECT 1`, so the public endpoint stays cheap to call. Orbit runs SQLite's quick check at startup, and a durable weekly full SQLite integrity and foreign-key check (on Postgres both only check the connection). A failed or interrupted full check stops normal HTTP traffic and exits the process; an unfinished integrity job forces another full check before a restart can bind. Remove the instance from proxy traffic and preserve the current data directory.

Do not attempt automatic repair. Verify a known-good backup, restore it with the documented stopped-server procedure, and retain the damaged copy for investigation. `/health/live` may remain 200 because it reports process liveness, not data safety.

## A background service fails

Retention, reconciliation, scheduler, or integrity supervision failure cancels normal serving. Weekly integrity checks and retention use durable schedules, so an overdue run is materialized immediately after restart. Backups are manual. The process stops accepting HTTP and tells workers to stop claiming jobs immediately, then bounds the combined drain to 30 seconds. Fix the reported database or storage condition, verify backups, and restart. Durable job leases recover after expiry; do not edit the jobs table.

## Restore rollback or ownership error

Before it replaces anything, `orbit backup restore <id>` backs up the current data (in S3 when backups go there) and prints the ID of that backup: restore that ID to undo the restore. If the current database cannot be backed up, nothing is restored; copy the data by hand, then add `--no-safety-backup`.

Restore refuses an active database ownership lock (the `<database>.lock` file next to the database; do not delete it while Orbit runs). Stop every Orbit process that uses that file. If restore reports rollback failure, do not start Orbit. Preserve the target and rollback paths named by the error and recover from a separate verified snapshot.

On Postgres (`ORBIT_DATABASE_URL`), the ownership lock is a Postgres advisory lock. Each server holds it shared; `backup restore`, `migrate run --reset` and `db-reset` need it exclusive, so stop every Orbit server that uses the database first. A server whose lock connection drops (network, Postgres failover) stops with `lost the database ownership lock`; start it again. A Postgres snapshot holds one zstd-compressed `COPY` file per table (`database/<table>.copy.zst`) instead of `database.sqlite`; restore also reads the plain `<table>.copy` files of older snapshots. Restore replaces every table in one transaction: if it fails, the database is unchanged. A snapshot restores only into the same kind of database it came from.

## Lost or expired setup token

Run `orbit setup-token rotate` (on SQLite, stop Orbit first; on Postgres it works while Orbit runs). Share the new setup URL directly with the installation owner; the account created there becomes the root user. Rotation invalidates the unused old token. Once setup completes, setup cannot run again.

## Root user and Admin area

The account created at setup is the root user. **Admin** in the sidebar (`/admin`) has accounts (suspend, reinstate, password recovery links, email changes), the global audit log with CSV export, backups, and instance settings (email, registration).

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

## Opening a SQL shell

`orbit db` opens the configured database in an interactive shell: `psql` for Postgres, `sqlite3` for SQLite. The Docker image includes both clients. Changes that you make in the shell skip all of Orbit's checks: make a backup first. With Compose: `docker compose --env-file deploy/.env -f deploy/compose.yaml exec orbit /orbit db`.

## Email, registration and the app key

Orbit sends email only for invitations, open-registration sign-up links, password reset links and email changes. The root user saves the SMTP server in **Admin → Settings → Email** and checks it with **Send test email**. Without a mail server, invitations are shared as links and password recovery works as described below. Orbit Cloud is different: it always needs a mail server (see [Billing](#billing-orbit-cloud)).

Registration is closed after setup: people join only by invitation. The root user can turn on **Open registration** in **Admin → Settings** once a mail server or a [sign-in provider](#sign-in-with-google-and-github) is saved. On Orbit Cloud, only a mail server counts. A new person then enters their email, opens the emailed link (60 minutes, one use) and chooses a name and password. Removing the mail server closes registration.

A person changes their email in **Account → Profile** (needs a mail server). Orbit asks for the current password (or a sign-in in the last 10 minutes for an account without one) and emails a one-use link, valid for 60 minutes, to the new address. The email changes only when the link is opened: the address is verified, every other session of the account is signed out, open password reset links stop working, and the old address gets a notice ("Your Orbit email was changed") that names the new address. Like registration, the request never tells whether another account has the new address: Orbit then sends nothing. If someone takes the address before the link is opened, the link answers `409 email_taken`. On Orbit Cloud, the Stripe customer of each workspace the person owns gets the new address (job `billing.stripe_customer_email`), so receipts follow the owner.

Without a mail server, Profile tells the person to ask an administrator. An admin opens **Admin → Users → Manage → Change email**, which works with or without a mail server. The new address is set at once and marked verified, and no confirmation email is sent, so confirm who is asking first. The person is signed out everywhere, and pending email change and password reset links stop working. If a mail server is saved, the old address gets the same notice. On Orbit Cloud this action needs a mail server (`409 smtp_required`), and both addresses get an email: the old one the notice, the new one "An Orbit administrator set this address as the email of your account". An admin can change their own email there too and stays signed in on the current device. The same rules as the other account actions apply: nobody can change the root account's email except the root user, and only the root user can change another admin's. A taken address answers `409 email_taken`, a deleted account `409 account_deleted`. The audit log records `account.email_changed` with the admin as the actor.

A person can also delete their account in **Account → Profile**, after typing their email and their password (or a fresh sign-in). Orbit refuses while they own a workspace outside the trash (`409 owns_workspaces`, delete those first) and for the root user (`409 root_account`). The account row stays so tasks, comments and messages keep their author, but it is anonymized: the name becomes "Deleted user", the email becomes `deleted-<id>@deleted.invalid` (so the address can register again), and the password, second factors, passkeys, connected accounts, sessions, OAuth grants, personal API tokens, push subscriptions and registered phones, avatar, profile fields and memberships are removed. **Admin → Users** shows it as **Deleted**; it can never be reinstated (`409 account_deleted`). The audit log has `account.deleted`.

Saved secrets (the SMTP password, GitHub App keys, Notion and Asana import tokens, Linear import API keys, Trello import API keys and tokens) are encrypted with the app key. `ORBIT_APP_KEY` (base64 of 32 random bytes, from `openssl rand -base64 32`) is required. Orbit does not create a key. Keep a copy outside the server and its backups: a database restored without the same key cannot read its saved secrets, and the root user must enter them again. Users with two-factor authentication must set it up again.

**Backups are encrypted with the app key.** Each new backup (manual, automatic and pre-migration, local and in S3) has its own random key, sealed with the app key in the backup's `manifest.json`; the database and every attachment file are encrypted. Without the app key a backup cannot be restored: `orbit backup restore` stops with "backup … is encrypted" when `ORBIT_APP_KEY` is not set, and with "encrypted with a different app key" when it is another key, before it changes anything. `orbit backup verify` checks the stored files and needs no key. So keep a copy of the app key outside the server and outside its backups, for example in a password manager. Backups made by older versions are not encrypted and still restore.

Older versions created an `app.key` file beside the SQLite database. If that file still exists, Orbit starts only when `ORBIT_APP_KEY` holds the same key. Set `ORBIT_APP_KEY` to the contents of the file, start Orbit, and then you can delete the file.

## iOS push notifications (APNs)

Browsers get Web Push without any setup. The iOS app gets notifications through Apple's push service (APNs) only when the server has an APNs auth key.

The Orbit app in the App Store accepts pushes only from coollabs' APNs key, so for now native push works only on Orbit Cloud (`app.orbit.so`). On a self-hosted server, the app says that this server does not send notifications to the app. People on an iPhone can still get notifications: open Orbit in Safari, add it to the Home Screen (iOS 16.4 or later), and turn on notifications there (Web Push). The steps below are for Orbit Cloud, or for a self-hosted server with its own build of the app (another bundle ID):

1. In the Apple developer account, open **Certificates, Identifiers & Profiles → Keys**, create a key with **Apple Push Notifications service (APNs)**, and download `AuthKey_<key id>.p8` (Apple lets you download it once).
2. Set `ORBIT_APNS_KEY_FILE` to the path of that file (or `ORBIT_APNS_KEY` to its text), `ORBIT_APNS_KEY_ID` to the key ID and `ORBIT_APNS_TEAM_ID` to the team ID. Set `ORBIT_APNS_TOPIC` only for a build with another bundle ID.
3. Restart Orbit. `orbit config show` lists the APNs variables (the key redacted). Orbit does not start when the key is not a `.p8` key, or when only some of the three variables are set.

One key sends to both APNs hosts: the app tells the server whether its device token is for `production` (App Store and TestFlight) or `sandbox` (development builds). Without a key, the app's registration (`POST /api/v1/push/apns`) answers `503 push_not_configured`, and Web Push works as before.

A phone gets pushes like a browser: only while the session that registered it is valid, and not while the person is active in an open tab or in the app (its live connection). Revoking or deleting the session stops them. APNs answers for a token that is no longer valid (`410`, `BadDeviceToken`, `DeviceTokenNotForTopic`) remove the phone; the app registers it again when it starts. Other refusals (for example `403 InvalidProviderToken` for a wrong key ID or team ID) are logged as `APNs push not sent` and keep the phone.

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
- If no account has that email, Orbit creates an account only when **Open registration** is on. Open registration does not need a mail server when a sign-in provider is configured (Orbit Cloud still needs one). A new account has no password. The person can add one in the profile settings within 10 minutes of a sign-in.
- An account with two-factor authentication must still enter its code after the provider sign-in.
- People connect and disconnect providers in their security settings. Orbit does not let a person remove the last sign-in method (password, passkey or provider).

## Sign in with OpenID Connect

People can sign in with your own identity provider, for example Authentik, Keycloak, Authelia, Zitadel or Pocket ID. Orbit supports one OpenID Connect provider. Only the `ORBIT_OIDC_*` variables configure it, so **Admin → Sign-in providers** shows it read-only.

1. At the identity provider, create a confidential client (an OAuth2/OpenID Connect application with a client secret) that uses the authorization code flow.
2. Add this redirect URI: `{ORBIT_PUBLIC_ORIGIN}/api/v1/auth/providers/oidc/callback`.
3. Allow the scopes `openid`, `email` and `profile`.
4. Set `ORBIT_OIDC_ISSUER`, `ORBIT_OIDC_CLIENT_ID` and `ORBIT_OIDC_CLIENT_SECRET` (or `ORBIT_OIDC_CLIENT_SECRET_FILE`). Optionally, set `ORBIT_OIDC_DISPLAY_NAME`. Restart Orbit.

`ORBIT_OIDC_ISSUER` must be the `issuer` value of the provider's `{issuer}/.well-known/openid-configuration` document; Orbit refuses a document that names another issuer. Orbit reads this document at each sign-in, so it gets the endpoints of the provider, and a provider that is not available at startup does not stop Orbit. The provider must have a userinfo endpoint. Orbit sends the client secret with HTTP Basic authentication (`client_secret_basic`). It uses `client_secret_post` only when the provider does not support Basic.

The rules of [Sign in with Google and GitHub](#sign-in-with-google-and-github) apply, with these details:

- Orbit identifies an account by the issuer and the `sub` claim together. If you change `ORBIT_OIDC_ISSUER`, the existing connections do not apply. At the next sign-in, people connect again with a verified email.
- A first sign-in connects to an existing account only when the provider sends `email_verified` as `true`. Some providers send `false` until you configure email verification. Then Orbit refuses the first sign-in because the email is not verified.
- A security confirmation asks the provider for a new sign-in (`prompt=login`, `max_age=0`). A normal sign-in uses the provider's session, so people who are signed in to the provider continue without a new sign-in.

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
- **Upgrades:** run `orbit migrate run` once (or let the first new server do it; a lock makes the others wait), then replace the servers one at a time. A migration must keep the previous release working until all servers run the new one: old servers stay ready on a newer schema, but an old server cannot start on it. Exception: stop every server before the upgrade to migration 0066 (see [Upgrade to migration 0066 (API token scopes)](#upgrade-to-migration-0066-api-token-scopes)).
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
- **Per job kind:** in **Admin → Background jobs**, the root user sets two limits for each job kind that customers start (Notion scans, Notion imports, Asana scans, Asana imports, Linear scans, Linear imports, Trello scans, Trello imports, Obsidian scans, Obsidian imports, Orbit fetches, Orbit imports and workspace exports; job kinds `notion.scan`, `notion.import`, `asana.scan`, `asana.import`, `linear.scan`, `linear.import`, `trello.scan`, `trello.import`, `obsidian.scan`, `obsidian.import`, `orbit.fetch`, `orbit.import`, `workspace.export`):
  - **All servers:** the number of jobs of the kind that run at the same time on all servers together. Default: 2.
  - **Per workspace:** the number of jobs of the kind that one workspace runs at the same time while a different workspace has a job of the kind waiting. Default: 1. When no other workspace waits, one workspace can use all the slots of the kind. Thus, one customer cannot stop the imports of the other customers.

  An empty field means no limit. **Reset to defaults** puts the values from the code back in the form. Save to apply them.

The limits are in the `job_limits` table. Each server reads the table each time it starts a job, so a change applies to all servers at the next job start. A restart is not necessary. A job that already runs continues. A saved value that is the same as the default removes the row of the kind, so a later change of the default in the code applies to it. Each save writes `instance.job_limits_updated` to the audit log. The API is `GET` and `PUT /api/v1/admin/jobs/limits` (root user only).

The maintenance jobs (retention, integrity check, scheduled backup, import cleanup) always run one at a time. The import cleanup (job kind `notion.cleanup`) expires and fails stale Notion, Asana, Linear, Trello, Obsidian and Orbit imports, and deletes the archives of finished Obsidian and Orbit imports. You cannot change their limit.

A Linear import sends the member's personal API key as `Authorization: <key>` to `https://api.linear.app/graphql`, and to `https://uploads.linear.app` to download uploaded files; the key is never sent to another host (a redirect elsewhere is followed without it). Linear allows 2,500 requests and 3,000,000 complexity points per hour for each key. The import spaces its requests (about 0.6 per second), waits when the rate-limit headers say the hourly budget is nearly spent, and waits for `Retry-After` when Linear answers `RATELIMITED`; a wait longer than five minutes makes the job retry later. A large team can take more than an hour.

A Trello import sends the member's API key and token in the header `Authorization: OAuth oauth_consumer_key="<key>", oauth_token="<token>"`, never in a URL, to `https://api.trello.com/1`, and to `https://trello.com` to download uploaded attachments from their `/download/` URL; neither is sent to another host (the redirect to the file store is followed without them). Trello allows 100 requests per 10 seconds for each token and 300 for each key. The import spaces its requests (8 per second), waits for the window when the `x-rate-limit-api-*-remaining` headers run low, and waits for `Retry-After` (or 10 seconds) on a `429`; a wait longer than five minutes makes the job retry later. A file download that Trello refuses (`401` or `403`, which some Enterprise workspaces enforce) is counted in the report and does not fail the import.

An Obsidian import receives the vault as one `.zip` upload (`POST /api/v1/workspaces/{workspace_id}/imports/obsidian`, multipart field `file`). The upload is streamed to the server's disk and then stored like a workspace export: in the workspace's own bucket, in the installation bucket when attachments go there, or on the server's disk under the backups directory (`imports/{workspace_id}/{import_id}.zip`), so any server can run the jobs. The archive is deleted when the import completes, fails, is cancelled or expires (24 hours after the scan if it was not started); the hourly import cleanup retries a deletion that failed. The archive is never extracted to disk. These limits apply, and an archive over one of them fails with a message for the member:

| Limit | Value |
|---|---|
| Upload size | `uploads.max_request_bytes` (default 100 MB) |
| Entries in the archive | 10,000 |
| Uncompressed size of all files | 1 GB |
| Compression ratio of one file over 1 MiB | 100 to 1 (zip-bomb guard) |
| Paths | No `..`, no absolute path, no symbolic link, no password protection |

A file larger than `uploads.max_file_bytes` is skipped and counted in the report. Linked files are attached as page files, with the same checks as uploads.

An Orbit import copies a workspace of another Orbit instance into this one. A member of the target workspace enters the other instance's address and an export API token of the source workspace (`POST /api/v1/workspaces/{workspace_id}/imports/orbit` with `{url, token}`), or uploads an export ZIP that was downloaded from the other instance (`POST …/imports/orbit/upload`, multipart field `file`). The token's scope decides the mode: `workspace:export` is a full import (the target member must be an owner or admin), `workspace:export:personal` a personal import (any member but a guest; only the exporter's private pages, private channels and DMs, into the importer's own account). An `orbit.fetch` job validates the token with `GET /api/public/v1/me`, starts the remote export (`POST /exports`, once; a retry reuses the stored export id), polls it with backoff for up to 24 hours, streams the ZIP to the server's disk and stores it like an Obsidian upload (`imports/{workspace_id}/{import_id}.zip`), deletes the token, reads `manifest.json` (only `orbit-workspace-export` version 1 is accepted) and builds the tree. After the member chooses projects, teamspaces, private pages and channels, an `orbit.import` job creates them. The token is sent only to the address that was entered, which must be `https` to a public host (no credentials, no redirects, no private, loopback or link-local address; the host name is resolved to public addresses only); `ORBIT_DEV_ORBIT_IMPORT_LOCAL=1` with `ORBIT_ENV=development` also allows `http://127.0.0.1:<port>`, for a second local server. These archive limits apply (they are not the Obsidian limits): the archive may be at most 5 GiB and hold at most 1,000,000 entries; each JSON entry may be at most 64 MiB; a file over 1 MiB may have a compression ratio of at most 100 to 1; the path checks of Obsidian imports apply. There is no limit on the total uncompressed size. A file over `uploads.max_file_bytes` is left out.

Every record the import creates is remembered in `orbit_import_links` (target workspace, source workspace id from the manifest, scope, kind, source id), so importing the same source again (a URL fetch or a ZIP upload, in any order) reuses what exists and adds only what is new: teamspaces are matched by name, public channels by name, everything else by its id in the source. The scope keeps imports apart: full imports share one scope, and each member's personal imports have their own, because the archive (and so every source id in it) is under the importer's control. A personal import never writes into anything a full import created (such a channel or page is counted as existing and skipped), and a full import never reuses what a personal import created. So a private channel or DM that a member imports before the admin exists twice afterward: the member's copy and the admin's. Two members who both import a shared private channel or DM each get their own copy. In a personal import every message and comment is written by the importer with a bold name and time header, and only the importer's own reactions are kept. Members are matched by email: a task's creator and assignees, a comment's or message's author and a reaction's user. A person without an account here is listed in the report; their tasks keep a "Created by" or "Assignees" line, and their comments and messages are written by the importer with a bold name and time header. A DM whose participants (the importer among them) all exist becomes a real DM, otherwise a private channel named after the people. Imported messages are history: they fire no notification, mention, push, bot, webhook or live event, and every member's read marker ends at the last message. A full import never holds other members' private pages, private channels or DMs; the report says so (`members_must_import_private`) so the admin can ask each member to run a personal import with their own token.

 (5 minutes without a heartbeat). If a server stops suddenly, its jobs hold their slots until their leases expire.

### Limits

- **Rate limits** (`ORBIT_RATE_LIMIT_*`, see [Request limits and email caps](#request-limits-and-email-caps)): the authentication, recovery/registration and invitation limits hold for the cluster. The upload, chat, general and public API/MCP limits and the co-editing connection limits count on each server: with N servers the effective limit is N times the setting. A database write for each of those requests would put every request of the cluster through the one writer (see **Write throughput**), so they stay in memory. Add a request limit at the load balancer if you need a cluster-wide one. The sign-in lockout and the email caps are shared.
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

- **Access.** A workspace whose subscription is `active`, `trialing` or `past_due` (Stripe still retries the card) has full access. Without one, a workspace is on a trial for `ORBIT_TRIAL_DAYS` days (default 7) from its creation: it is writable, but holds at most `ORBIT_TRIAL_TASKS` tasks (default 50) and `ORBIT_TRIAL_PAGES` pages (default 20), not counting the trash, and `ORBIT_TRIAL_MEMBERS` members (default 3, pending invitations count). A change applies to every workspace without a subscription at the next restart, including ones already on a trial. Anything that would add tasks or pages past a limit fails with 402 `trial_limit_reached`: creating them, restoring them from the trash, and copying pages (a copy with sub-pages counts every page it copies); these are counted inside the write's transaction, so concurrent creates cannot pass a limit. An invitation past the members limit also fails with 402 `trial_limit_reached`. At the tasks limit, a GitHub `issues` or `pull_request` webhook that would create or restore a task is answered with 202 and dropped, and a Discord event gets 402 `trial_limit_reached`. A Notion or Obsidian import stops at the pages limit with a message, and an Asana, Linear or Trello import at the tasks limit. After the trial, without a subscription or with any other status (`canceled`, `unpaid`, `paused`, `incomplete`…), the workspace is read-only: members can read and search everything, and owners and admins can still export the whole workspace (see [Workspace export](#workspace-export)), but a member's write fails with 402 `workspace_read_only` (anyone else gets the usual 401 or 404). Co-editing sockets still show documents and drop edits; a socket that was editing when the workspace became read-only closes, on every server, and the editor reconnects read-only. Writes that stay open: subscribing (**Settings → Billing**), starting a workspace export, deleting or restoring the workspace, removing members or leaving, revoking invitations, creating and revoking API tokens, disconnecting GitHub, cancelling a Notion, Asana, Linear, Trello or Obsidian import, and the reader's own read markers and list preferences. Integrations do not write: a GitHub `issues` or `pull_request` webhook is answered with 202 and dropped (installation events still update the connected repositories), a Discord event gets 402 `workspace_read_only`, and a running Notion, Asana, Linear, Trello or Obsidian import fails with a read-only message.
- **Subscribing.** The workspace owner chooses a plan in **Settings → Billing**. Orbit creates a Stripe customer for the workspace and sends the owner to Stripe Checkout. Afterwards the owner opens the Stripe Billing Portal from the same page to change the card, the plan, or cancel. Other members see the status only.
- **Duplicate checkouts.** Before it opens a new Checkout for a workspace that already has a Stripe customer, Orbit asks Stripe for the customer's subscriptions. If one is `active`, `trialing` or `past_due`, the checkout fails with 409 `subscription_active` and Orbit queues a `billing.stripe_sync` job for that subscription, so a missed or late webhook cannot lead to a second subscription. Otherwise Orbit expires the customer's open Checkout Sessions first, so an older Checkout tab cannot subscribe the workspace again.
- **Deleting a workspace.** When the owner moves a workspace with a subscription to the trash, Orbit cancels the subscription at Stripe at once, with no proration credit and no final invoice. The delete queues a `billing.stripe_cancel` job in the same transaction, with the subscription id in its payload, so the cancel still runs after a Stripe outage (it retries with backoff for about two days) and after the workspace is purged. A subscription that Stripe has already cancelled, or no longer knows, counts as done. After the cancel, the job queues `billing.stripe_sync`, which stores the `canceled` status. The workspace stays in the trash for 30 days. Restoring it does not resume billing: after its trial, the restored workspace is read-only until the owner subscribes again.
- **Orbit Cloud needs a mail server.** Save one in **Admin → Settings → Email** right after setup. The mail settings live in the database, so the server starts without them, but until one is saved email change, invitations by email, password reset and sign-up do not work. Meanwhile the server logs a warning at startup, and installation admins see a red banner in the web and mobile apps that links to Admin → Email. Customers see nothing. Removing the mail server fails with 409 `smtp_required` (save a different one instead), and **Open registration** needs it even when a sign-in provider is configured (409 `email_not_configured`).
- **Admin actions on customer accounts are never silent.** In **Admin → Users**, Change email, the password reset and Reset two-factor need the mail server (`409 smtp_required`, and the apps turn the actions off with the reason) and email the person: both addresses for an email change, the reset link to the account's address, and a notice for a two-factor reset. The admin never sees a password reset link: the action is **Send password reset email** and answers `202` with `sent_to` instead of `url`. A failed send does not undo the action; the server logs a warning.
- **Ownership transfer.** Orbit Cloud does not support ownership transfer yet: the request fails with 409 `transfer_unavailable`, and the clients hide the action. A self-hosted server keeps it.
- **Webhook.** In the Stripe Dashboard (Workbench → Webhooks), add a destination with the **Snapshot** payload for `https://<public origin>/api/v1/billing/stripe/webhook` and these events: `checkout.session.completed`, `customer.subscription.created`, `customer.subscription.updated`, `customer.subscription.deleted`. Put its signing secret in `ORBIT_STRIPE_WEBHOOK_SECRET`. Orbit checks the signature (at most 5 minutes old), records each event once, and queues a `billing.stripe_sync` job that reads the subscription again from Stripe. Events that arrive late, twice or out of order therefore all end in Stripe's current state. A failed job retries with backoff for several hours; see **Admin → Background jobs**.
- **Billing Portal.** Turn on the customer portal in the Stripe Dashboard (Settings → Billing → Customer portal) in each mode you use. Without a saved portal configuration, Stripe refuses to open the portal and Orbit answers 502 `stripe_unavailable`.
- **Your own workspace.** The root user's first workspace needs no subscription: it always has full access, with no trial and no limits. Orbit marks it when the root user creates it (at setup, or later if the root user had none). On an installation that existed before this rule, the upgrade marks the oldest workspace the root user owns, including one in the trash. Other workspaces of the root user pay like any other workspace. Changing the root user (`orbit root set`) does not move the mark.
- **Products and prices.** `STRIPE_SECRET_KEY=sk_… scripts/stripe-setup.sh` creates the Orbit product with a monthly and an annual price per workspace, and the default Billing Portal configuration (customers can switch between monthly and annual, update their card, and cancel), and prints `ORBIT_STRIPE_PRICES`. Run it once in each mode: the price ids differ, and a second run creates nothing new. With `ORBIT_WEBHOOK_URL` set, it also creates the webhook and prints its signing secret.
- **Local testing.** Use test-mode keys (`sk_test_…`) and forward events with the Stripe CLI: `stripe listen --forward-to http://127.0.0.1:8080/api/v1/billing/stripe/webhook`. The CLI prints the signing secret (`whsec_…`) to put in `ORBIT_STRIPE_WEBHOOK_SECRET`. Pay with the test card `4242 4242 4242 4242`.
- **API version.** Orbit sends `Stripe-Version: 2026-09-30.endive` with each request. Webhook events use the endpoint's own API version; Orbit only reads ids from them, so that version does not matter.

## Workspace export

A workspace owner or admin exports the whole workspace in **Settings → Export** (`POST /api/v1/workspaces/{id}/exports`). The export is for data portability and for customers who leave, so it also works on a read-only workspace. A `workspace.export` background job writes one ZIP file. Only one export of a workspace is queued or running at a time; a second start fails with 409 `export_in_progress`. The job kind has the usual limits in **Admin → Background jobs** (default: 2 on all servers, 1 per workspace while another waits).

- **Who sees what.** The export uses the view of the member who starts it. It contains every task (also tasks in the trash), every live teamspace page, every public chat channel, the member's own private pages, and the private channels and direct messages that the member is in. Other members' private pages, private channels and direct messages are not in it. The job checks the permission again when it runs.
- **Download.** `GET /api/v1/workspaces/{id}/exports` lists the exports that have not expired. `GET …/exports/{export_id}/download` streams the ZIP to an owner or admin of the workspace; there are no public links. Orbit writes `workspace.export_created` and `workspace.export_downloaded` to the audit log.
- **Storage.** The job writes the ZIP to a temporary file under the backups directory (`ORBIT_BACKUPS_PATH`), then stores it as `exports/{workspace_id}/{export_id}.zip` where new attachments of the workspace go: the workspace's own bucket, else the bucket of **Admin → Storage** when attachments are in S3, else `<backups>/exports/` on the server's disk. On Orbit Cloud (attachments in S3) every server can serve the download. Backups do not include exports. The server needs free disk space for one ZIP while an export runs. A ZIP in S3 is one PUT, so it can be at most 5 GiB.
- **Expiry.** An export is deleted with its ZIP 7 days after it is ready, by the daily retention job. An export that is still queued or running after 24 hours is marked failed. A failed export is removed after 7 days.
- **Archive layout.** Each ZIP has a `README.txt` that describes it. JSON field names are snake_case and stable; times are RFC 3339 in UTC.

| Path | Content |
|---|---|
| `workspace.json` | Name, creation time, who exported it, members (name, email, role), teamspaces, projects with their statuses and labels |
| `tasks/<KEY>-<number>.json` | One task: title, Markdown description, status, priority, dates, creator, assignees, labels, relations, comments, attachments, activity (its audit events), `deleted` for the trash |
| `pages/<Teamspace>/<Page>.md`, `pages/Private/<Page>.md` | Each live page as Markdown (the same converter as the page export); sub-pages in `<Page>/`, page files in `<Page>/assets/` |
| `pages/comments.json` | The comment threads of the exported pages |
| `chat/<channel>.json`, `chat/DM - <names>.json` | One conversation: members, messages with reactions, files and thread replies |
| `files/<file-id>/<name>` | Each file of a task, comment or chat message, once; the JSON refers to it by `path` |

Not in the export: pages in the trash, page version history, notifications, saved views, API tokens, integration settings, custom emoji and stickers. Orbit has no custom task fields. A file that is missing from storage is listed at the end of `README.txt` and the export continues.

## Object storage (S3), backups and a lost server

The root user saves an S3-compatible bucket (AWS S3, Cloudflare R2, MinIO, …) in **Admin → Storage**. Orbit writes, reads and deletes a test object before it saves the bucket. The secret key is encrypted with the app key. Then three settings apply:

- **Files in S3**: all uploaded files (task and comment attachments, doc and chat files, custom emoji, stickers, mail attachments, profile pictures). New files go to the bucket, and existing files move there in the background (the page shows how many are left). Turning it off moves them back to this server. Uploads are still staged on the local disk first. The bucket can be removed only when no attachment file is left in it.
- **Backups in S3**: each new backup is uploaded to `backups/snapshots/{id}/` in the bucket (manifest last) and removed from this server. The Backups page lists local and S3 backups; download and restore fetch an S3 backup into `<backups>/downloads/` and verify its checksums first. Pre-migration backups stay local.
- **Automatic backups**: off, every hour or every day. Retention keeps the newest backup of each of the last 7 days and 4 weeks, local and in S3.

With attachments in S3, a backup holds the database (and the files still on this server), not the bucket's files. So Orbit does not delete an unused file from the bucket at once: it keeps it 40 days, longer than any kept backup, so a restored older database still finds its files.

The `orbit backup` commands use the `ORBIT_S3_*` bucket, or else the bucket saved in Admin → Storage, and decrypt its secret with `ORBIT_APP_KEY`. With Postgres, or with SQLite on a server that still has its database, stop the servers and restore:

```sh
orbit backup list            # the last column says local or s3
orbit backup restore <id>
```

When the database is lost too (an SQLite server is gone), restore on a new server and give the bucket in environment variables. They come before the saved bucket:

```sh
export ORBIT_BACKUP_S3_ENDPOINT=https://s3.eu-central-1.amazonaws.com ORBIT_BACKUP_S3_BUCKET=orbit-files \
  ORBIT_BACKUP_S3_ACCESS_KEY_ID=… ORBIT_BACKUP_S3_SECRET_ACCESS_KEY=…   # optional: _REGION, _PREFIX, _PATH_STYLE=true
orbit backup list            # the last column says local or s3
orbit backup restore <id>
```

Set the same `ORBIT_APP_KEY` on the new server before you start Orbit: without it, Orbit cannot read the saved S3 secret and does not start while a bucket is saved. Data written after the last backup is lost, so choose the schedule by how much data you can lose.

### Restore one page or one task

To get back one page or one task, you do not need to stop the server or restore the whole database. An instance admin opens a backup, picks a workspace and an item, and Orbit copies the item into the live workspace as a new item. Nothing else in the live database changes. The API is `GET /api/v1/admin/backups/{id}/workspaces`, `GET …/workspaces/{workspace_id}/items?kind=page|task&q=` and `POST …/workspaces/{workspace_id}/restore`.

- Orbit verifies the backup's checksums first. It checks each backup at most once in 10 minutes per server. A backup in S3 is fetched into `<backups>/downloads/` as for a download.
- Orbit reads the snapshot and never writes to it: the SQLite file opens read-only, and Postgres `COPY` files (`.copy.zst`, or `.copy` in older backups) are parsed.
- The workspace must still exist and must not be in the trash.
- A page gets the title "<title> (restored)". With sub-pages, the copy keeps their structure and titles. It goes under its old parent when that page is live in the same space, else to the top level of its teamspace. If the teamspace is gone, the restore fails (`teamspace_missing`). A private page goes back only to its owner's private space, and only while the owner is a member.
- A task gets the title "<title> (restored)" and a new number in its old project. If the project is gone or in the trash, the restore fails (`project_missing`). The status and labels are matched by name in the live project; without a match the task gets the first status and no label. Assignees who left the workspace are dropped.
- Files are shared with the live workspace when it still has the same bytes. Otherwise Orbit uploads them again from the backup's `attachments/` folder, or from the blob store (S3 keeps unused files for 40 days). The response counts the files it could not find in `missing_files`. Restored files count toward the storage quota.
- Not restored: comments, page versions, favorites, visits, task relations, sub-tasks and activity.
- Each restore writes the audit event `backup.item_restored` with the backup id, the kind, the old id and the new id.

### A workspace's own bucket

Workspace owners and admins can connect their own bucket in **Settings → File storage** (permission `storage.manage`). The workspace's attachment files (`blobs/{workspace_id}/…` under the bucket's prefix) then go to that bucket, whatever Admin → Storage says. Backups stay with the installation.

- The endpoint must be `https` on a public internet address. Orbit refuses local and private addresses, also when a host name resolves to one, so a workspace cannot make the server reach its own network. A team with a MinIO on a private network cannot use it here; the root user can use it in Admin → Storage instead.
- Orbit tests the bucket like the installation's bucket. The installation's bucket and a workspace's bucket must not overlap: on the same endpoint and bucket, neither prefix can be equal to the other or inside it (`orbit` and `orbit/files` overlap, and the empty prefix overlaps every prefix). Orbit refuses such a save in both places (422 `invalid_storage_settings`), also against a removed bucket whose files still move back. Orbit compares the endpoint host and port, so it cannot see two names for one server (for example, `localhost` and `127.0.0.1`). Do not use two names for one bucket.
- After the save, existing files move from this server's disk and the installation's bucket to the workspace's bucket in the background, as when the root user turns on attachments in S3. The page says "Checking which files to move…" until the first pass counted them, then shows how many are left. Files stay readable during the move.
- **Remove** sends new files to the installation's storage again. The files in the workspace's bucket move back and are deleted from it. When none are left, the bucket disappears from the settings. To change the endpoint, bucket or prefix, remove the bucket first and wait for that.
- As with the installation's bucket, Orbit keeps an unused file in the workspace's bucket for 40 days. If the bucket fails (for example, revoked keys), other workspaces are not affected: reads try the other tiers, and cleanup skips that bucket until it works again.
- Files in a workspace's bucket still count toward the workspace's storage quota.
- When a workspace is deleted for good, its files in its own bucket stay there.
## Mail

Orbit is its own mail server for incoming mail. Mail for the verified domains of the workspaces arrives in one of two ways: at Orbit's own SMTP listener, or through a mail provider that sends each message to Orbit over HTTPS (no port 25 on this server; see [Receive through a provider](#receive-through-a-provider)). Members read it in **Mail**. Outgoing mail is not in this release.

### Settings

| Variable | Default | Notes |
|---|---|---|
| `ORBIT_MAIL_ENABLED` | `false` | Starts the listener. |
| `ORBIT_MAIL_HOSTNAME` | none | Required when mail is on. The name in the SMTP greeting, the MX target and the name the certificate must cover, for example `mx.example.com`. |
| `ORBIT_MAIL_SMTP_BIND` / `ORBIT_MAIL_SMTP_PORT` | `0.0.0.0` / `2525` | The listener address. |
| `ORBIT_MAIL_MODE` | `relay` | `relay` or `direct`. See [Modes](#modes). |
| `ORBIT_MAIL_RELAY_CIDRS` | none | Relay mode: required. The relay's addresses. Orbit refuses `/0` and any range that contains the Docker gateway `172.17.0.1`. |
| `ORBIT_MAIL_RELAY_AUTHSERV_ID` | none | Relay mode: required. The `authserv-id` of the relay's `Authentication-Results` header. |
| `ORBIT_MAIL_PROXY_CIDRS` | none | Sources (a TCP load balancer) that can send a PROXY protocol v2 header. Orbit then uses the client address from the header. |
| `ORBIT_MAIL_TLS_CERT` / `ORBIT_MAIL_TLS_KEY` | none | PEM files. Direct mode: required. Relay mode: optional; with them, Orbit offers STARTTLS. |
| `ORBIT_MAIL_MAX_MESSAGE_BYTES` | `52428800` (50 MB) | The raw size limit, at least 1 MiB. 50 MB holds about 37 MB of attachments. |
| `ORBIT_MAIL_DNS_RESOLVER` | system | A resolver address (`ip:port`) for the SPF, DKIM, DMARC and domain checks. |
| `ORBIT_MAIL_CONTENT_ORIGIN` | public origin | An optional separate origin for mail HTML and its images. When set, only this host serves them; the public origin answers 404. Point it at the same server and keep the original `Host` header in the proxy. |

Mail is off by default. The root user turns it on with the **Mail** switch in **Admin → Inbound mail**. Until then, no one sees the Mail app, its menu items, its shortcuts or **Settings → Mail**, and the workspace mail API answers `403 mail_disabled`. When the switch is on, workspace admins add domains and mailboxes in **Settings → Mail**. If you turn it off again, mail for the domains that were added before still arrives, and it shows again when you turn Mail on.

### Modes

- **Relay (default):** a mail service you trust (a spam filter or a mail gateway) receives the mail and sends it to Orbit. Only `ORBIT_MAIL_RELAY_CIDRS` can connect; other clients get `554 5.7.1`. Orbit does not check SPF, DKIM and DMARC itself: it uses only the relay's `Authentication-Results` header with `ORBIT_MAIL_RELAY_AUTHSERV_ID` and removes the others. A message without that header gets no results, and Orbit logs a warning. Use relay mode when the server cannot receive on port 25.
- **Direct:** the MX record points to Orbit, and Orbit checks SPF, DKIM and DMARC itself. The certificate must cover `ORBIT_MAIL_HOSTNAME`.

In both modes Orbit removes incoming `Authentication-Results` headers with its own ID and adds its own. A DMARC failure does not refuse a message: Orbit puts it in Spam.

Many cloud providers block port 25 (inbound, outbound or both), or open it only on request. Ask your provider, use relay mode, or receive through a provider.

### Receive through a provider

The domain's MX points to a mail provider, and the provider sends each message to `https://<public origin>/api/v1/mail/inbound/<kind>/<id>`. This server needs no listener, no port 25, no certificate and no PTR record for it. The public origin must be reachable from the internet.

- **Instance providers:** the root user adds them in **Admin → Mail → Mail providers**. Every workspace can route a domain to them.
- **Workspace providers:** owners and admins add their own in **Settings → Mail → Providers**, with their own provider account. The root user can turn this off (**Workspaces can add their own providers**). Off stops new ones; existing ones keep receiving. Orbit Cloud keeps it off and receives all mail at its own MX.
- In **Settings → Mail → Domains**, pick where each domain's mail arrives. The MX record on the page then shows the provider's hosts, and the domain check expects them. The verification TXT record stays required.
- Add the domain at the provider yourself; Orbit shows the steps for each provider after you add it. Orbit makes the secrets of Cloudflare and Postmark and shows them once. **New secret** on the provider makes new ones: the old ones stop working at once, so set the new ones on the Worker or in Postmark right away.
- Each request must carry the provider's signature or credentials. Orbit refuses requests older than 5 minutes and Mailgun tokens it has seen.
- Orbit checks DKIM and DMARC itself and takes SPF from the provider (its `Received-SPF` header, or the verdict of PostShiba or SES).
- Mail for an address that no mailbox has: Cloudflare bounces it to the sender. The other providers have already accepted it, so Orbit lists it under **Settings → Mail → Failed messages → Not delivered** for 30 days, without its body.

| Provider | MX | At the provider | Notes |
|---|---|---|---|
| PostShiba (recommended) | `inbound.postshiba.com` | A sending domain, and an inbox on it: **Root domain** (or a subdomain, which you then add to Orbit), address `*`, webhook URL Orbit's URL, format **PostShiba JSON**. Enter the inbox's signing secret and an API key of a team admin. | Orbit reads the raw message with the API key while the webhook waits (PostShiba keeps it until the webhook succeeds). Receiving is free at PostShiba. |
| Cloudflare Email Routing | `route1/2/3.mx.cloudflare.net` (Cloudflare adds them) | An Email Worker with Orbit's script; variables `ORBIT_WEBHOOK_URL` and the secret `ORBIT_SIGNING_SECRET`; the catch-all rule sends to the Worker. | Messages up to 25 MiB. The Worker runs while the sender is connected, so unknown addresses bounce and Orbit errors make the sender retry. |
| Postmark | `inbound.postmarkapp.com` | Inbound webhook URL with Orbit's credentials in it, and **Include raw email content in JSON payload** on. | Postmark retries until Orbit stores the message. |
| Mailgun | `mxa.mailgun.org`, `mxb.mailgun.org` (EU: `mxa/mxb.eu.mailgun.org`) | A receiving route that matches `.*@<domain>` and forwards to Orbit's URL (it ends in `/mime`); the HTTP webhook signing key. | |
| Resend | the host Resend shows (enter it in the provider) | A webhook for `email.received`; its signing secret and an API key that can read received emails. | Resend keeps its own copy of each message; Orbit cannot delete it. |
| Amazon SES | `inbound-smtp.<region>.amazonaws.com` | A receipt rule that stores the message in an S3 bucket and notifies an SNS topic with an HTTPS subscription to Orbit's URL. Enter the region, bucket, prefix and topic ARN. | Orbit confirms the subscription, refuses other topics, and deletes each message from the bucket once stored. The IAM user needs `s3:GetObject` and `s3:DeleteObject` on the bucket. As a backstop, add a bucket lifecycle rule that deletes objects after a few days. |

### Port 25 and the client address

The container runs without capabilities, so the listener uses port 2525. The Compose file maps host port 25 to it (`ports: ['25:2525']`, commented out in [`deploy/compose.yaml`](../deploy/compose.yaml)).

Orbit must see the real client address: relay mode, SPF and the connection limits use it. With the default Docker port proxy, clients can arrive from the Docker gateway address. Do one of these:

- Set `"userland-proxy": false` in `/etc/docker/daemon.json` and restart Docker.
- Run the Orbit service with `network_mode: host`.

Orbit logs a warning and the health card shows an error when clients connect from a Docker bridge address. Behind a TCP load balancer, turn on PROXY protocol v2 on the balancer and set `ORBIT_MAIL_PROXY_CIDRS` to its addresses.

### Certificate

Get a certificate for `ORBIT_MAIL_HOSTNAME` (for example with certbot) and mount it read-only. UID 65532 must be able to read both files. Orbit checks the files every 60 seconds and loads them again when they change, without a restart. If a new file cannot be read, Orbit keeps the last good certificate. Orbit logs a warning 7 days before the certificate expires.

### DNS records

**Settings → Mail → Domains** shows each record with a copy button and its status:

| Record | Name | Value | Need |
|---|---|---|---|
| TXT | `_orbit-verify.<domain>` | `orbit-verify=<token>` | Required. Proves that the workspace owns the domain. |
| MX | `<domain>` | `10 <ORBIT_MAIL_HOSTNAME>` | Required in direct mode. In relay mode, the MX points to the relay, and Orbit accepts any MX. |
| TXT | `<selector>._domainkey.<domain>` | `v=DKIM1; k=rsa; p=…` | Recommended. For sending in a later release. The value is longer than 255 bytes: zone files need it as several strings, and the page shows them. |
| TXT | `<domain>` | `v=spf1 mx ~all` | Recommended. |
| TXT | `_dmarc.<domain>` | `v=DMARC1; p=none` | Recommended. |

The domain check runs every 10 minutes for a pending domain (for 72 hours) and once a day for a verified domain. **Check now** runs it at once.

- **Pending → verified:** the verify record has the token.
- **Verified → suspended:** the verify record is gone. A suspended domain receives mail for 7 days, then refuses it. When the record is back, the domain is verified again.
- A DNS timeout shows `unknown` and changes nothing.

A domain belongs to one workspace. Orbit refuses a domain when it, a parent domain or a subdomain belongs to a different workspace. An instance admin can move a domain without mailboxes to a different workspace.

Also set the PTR (reverse DNS) record of the server's public address to `ORBIT_MAIL_HOSTNAME`. Your hosting provider sets it.

### Health card and test message

**Admin → Mail** shows a health card: the listener runs, the certificate covers the hostname and when it expires, the real client address, and the PTR of each address of the hostname. Each failed item shows a short fix.

To test the whole path, click **Test message**. The card shows a one-time address `check-<random>@<verified domain>`, valid for 1 hour. Send a mail to it from an outside account (for example Gmail). When the mail arrives, the card shows the client address, the TLS version, and the SPF, DKIM and DMARC results. The test message does not go to a mailbox. This checks MX, port 25, TLS and the checks without an external service.

### Failed messages

Orbit stores each accepted message first and then files it into the mailboxes in a background job. If that job fails on its last attempt, the message is **failed**. **Settings → Mail → Failed messages** lists failed messages, and messages that wait for more than one hour without a job, with the sender, the size and the error. Fix the cause, then click **Retry**.

### Cluster

Each server runs a listener. Point the MX records to every server, or put a TCP load balancer in front of them (with PROXY protocol v2, see above). Any server can file any message, because the message goes into the shared database first. The connection limits count on each server. A cluster needs S3 for files (see [Orbit Cloud](#orbit-cloud-one-postgres-several-orbit-servers)), so that each server can read each stored message.

### Data at rest and backups

Orbit encrypts each stored message with a key for each workspace, and seals the DKIM keys with the app key. The database holds the headers and the message text without encryption, for search. Protect the database at rest yourself: use disk encryption or a managed Postgres service with encryption.

Backups contain the mail and are encrypted with the app key. See [Email, registration and the app key](#email-registration-and-the-app-key).

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

If the person also lost access to their email address, first change it in **Manage → Change email** (see [Email, registration and the app key](#email-registration-and-the-app-key)), then create the recovery link. Changing the email deletes recovery links created before it.

On Orbit Cloud the admin never gets the link. **Manage → Send password reset email** emails it to the account's address (same 30 minutes, the email says an administrator started it), and the request answers `202` with `sent_to` and no `url`. It needs the mail server (`409 smtp_required`), like Change email and Reset two-factor. A person who lost access to their address gets the email changed first, which also emails the new address.

If the root user lost their own password, use (on SQLite, after you stop Orbit) `orbit recovery-link --email <email> --origin <https-origin>`. Never log a link. Unknown or suspended accounts do not receive a link.

## Security checks after proxy changes

- An HTTPS request through a trusted proxy must receive HSTS.
- A login response must use `__Host-orbit_session` with `Secure`, `HttpOnly`, `SameSite=Lax`, and no `Domain`.
- Requests from untrusted peers cannot supply trusted request IDs or forwarded transport.
- A trusted proxy's `X-Request-Id` is used only when it is 1–128 characters of `A-Z a-z 0-9 - . _ : /`. Orbit
  generates a new ID for any other value; it does not reject the request.
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
| `general_per_minute` | 600 | Every other API request (chat: 3 times this). The public API (`/api/public/…`) and MCP (`/mcp`) have their own budget with this limit, apart from the web app's. Per server. |
| `workspace_multiplier` | 10 | A workspace may make this many times each limit above. |
| `account_email_cooldown_seconds` | 60 | Time between two registration, password reset or email change emails to one address. |
| `account_emails_per_day` | 5 | Registration, password reset and email change emails to one address in 24 hours. |
| `invitation_emails_per_workspace_per_day` | 100 | Invitation emails of one workspace in 24 hours. |
| `invitation_emails_per_inviter_per_day` | 50 | Invitation emails one person sends in 24 hours, in all workspaces. |

- **Who a request counts against:** first its client IP address, then its credential (the session cookie or the `Authorization` header: a session, an API token, an MCP token), then, for a request with a credential, the workspace in its path (`/api/v1/workspaces/{id}/…` or `/api/public/v1/workspaces/{id}/…`). The first one over its limit refuses the request with `429 rate_limit_exceeded`, and the ones after it do not count it. So a client without a credential cannot use up a workspace's budget, and one address can use at most its own limit of it. The workspace is charged only for a response below 400, because an error does not show that the credential was accepted: a made-up cookie or token gets `405` or `400` before it is checked. So a client cannot use up a workspace's budget without a valid credential of the workspace. Each class has its own budgets, so public API and MCP traffic cannot use up the web app's.
- **Account emails:** registration, password reset and email change links to one address share the cooldown and the daily cap, on every server. Over the cap, Orbit sends nothing and gives the usual `202` answer, so the answer does not tell whether an account exists. Requests for which no email goes out (an unknown address for a reset, an existing account for registration or an email change) do not count. The notice to the old address after an email change has no cap.
- **Invitation emails:** an email invitation over a cap is taken back and answered with `429 invitation_email_limit` and `Retry-After`. Invitation links (`"delivery": "manual"`) have no cap. The subject is always "You have been invited to Orbit"; the body names the workspace on one line, in quotes, without control or direction characters, and at most 60 characters long. The inviter's name is not in the email.
- Values: the per-minute limits 1 to 1,000,000; `workspace_multiplier` 1 to 1000; the cooldown 0 to 86,400 seconds; the daily caps 1 to 100,000.
- **Fixed HTTP limits:** a client must send a request's headers within 30 seconds, or Orbit closes the connection. A request that has no response after 60 seconds gets `408 request_timeout`; WebSockets, uploads, downloads, exports and the Admin routes have no such limit. A request body may be at most 2 MiB, except a `multipart/form-data` upload, which may be up to `ORBIT_UPLOAD_MAX_REQUEST_BYTES`. Give the reverse proxy timeouts at least this long.

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
