# Self-hosting with Docker

This guide runs Orbit with Docker Compose behind [Caddy](https://caddyserver.com/), which gets and renews the HTTPS
certificate. To run one container with `docker run` and your own proxy, see
[one Docker container (SQLite)](self-hosting-sqlite.md) or [Docker and Postgres](self-hosting-postgres.md). For all variables and operator tasks, see [Deployment and operations](operations.md).

## Requirements

- A Linux server with Docker and Docker Compose.
- A DNS name (for example `orbit.example.com`) that points to the server.
- Open inbound TCP ports 80 and 443.
- Free Docker subnet `172.30.0.0/24`. If your host uses it, change the subnet, the proxy address and
  `ORBIT_TRUSTED_PROXIES` in `deploy/compose.yaml` together.

## Install

1. Get the deployment files:

   ```sh
   git clone https://github.com/coollabsio/orbit.git
   cd orbit
   cp deploy/.env.example deploy/.env
   ```

2. Edit `deploy/.env`:

   | Variable | Required | Value |
   |---|---|---|
   | `ORBIT_DOMAIN` | yes | Your domain, without `https://`. Caddy and `ORBIT_PUBLIC_ORIGIN` use it. |
   | `ORBIT_APP_KEY` | yes | The output of `openssl rand -base64 32`. Orbit encrypts saved secrets (SMTP password, S3 secret, GitHub App keys, 2FA) with it. Keep a copy off the server: without it, Orbit cannot read these secrets. |
   | `ORBIT_IMAGE` | no | `ghcr.io/coollabsio/orbit:latest` to use the published image. Without it, Compose builds the image from source (slow). |

   Image tags: `latest` (the newest release), a minor version such as `0.4`, or an exact version such as `0.4.1`.
   `main` and a seven-character commit SHA are builds of the `main` branch, not releases.

3. Start Orbit:

   ```sh
   docker compose --env-file deploy/.env -f deploy/compose.yaml pull
   docker compose --env-file deploy/.env -f deploy/compose.yaml up -d --no-build
   ```

   Without `ORBIT_IMAGE`, use `up -d --build` instead.

4. Get the one-time setup URL from the log:

   ```sh
   docker compose --env-file deploy/.env -f deploy/compose.yaml logs orbit
   ```

   Keep the URL secret. Open it and create the root account (the only account that can open Admin). Orbit then asks
   for your first workspace and project. If the URL is lost or expired, see
   [Lost or expired setup token](operations.md#lost-or-expired-setup-token).

5. Check that `https://<your domain>/health/ready` answers.

## What runs

| Service | Image | Ports | Volumes |
|---|---|---|---|
| `orbit` | `ORBIT_IMAGE` or a local build | 8080 (HTTP, only on the internal network), 2525 (SMTP, off) | `orbit-data` → `/var/lib/orbit` (SQLite database and attachments), `orbit-backups` → `/var/backups/orbit` |
| `proxy` | `caddy:2-alpine` | 80, 443 | `caddy-data`, `caddy-config` (certificates) |

The Orbit container runs as UID 65532, with a read-only root filesystem and no Linux capabilities. Only Caddy can
reach port 8080.

## Configuration

Orbit reads its configuration only from `ORBIT_*` environment variables. It refuses to start when it finds an unknown
`ORBIT_*` variable, so do not give it the whole `deploy/.env` file. The image and `deploy/compose.yaml` already set
these variables:

| Variable | Value | Set by |
|---|---|---|
| `ORBIT_ENV` | `production` | image, compose |
| `ORBIT_BIND` | `0.0.0.0` | image |
| `ORBIT_PORT` | `8080` | default |
| `ORBIT_DATABASE_PATH` | `/var/lib/orbit/orbit.sqlite` | image |
| `ORBIT_ATTACHMENTS_PATH` | `/var/lib/orbit/attachments` | image |
| `ORBIT_BACKUPS_PATH` | `/var/backups/orbit` | image |
| `ORBIT_PUBLIC_ORIGIN` | `https://${ORBIT_DOMAIN}` | compose |
| `ORBIT_TRUSTED_PROXIES` | `127.0.0.1/32,172.30.0.2/32` (healthcheck and Caddy) | compose |
| `ORBIT_APP_KEY` | from `deploy/.env` | compose |

To add more variables, make `deploy/compose.override.yaml` (Git ignores it) and give it as a second `-f` file in
every command:

```yaml
services:
  orbit:
    environment:
      # Outgoing mail (Admin → Email)
      ORBIT_SMTP_HOST: smtp.example.com
      ORBIT_SMTP_USERNAME: orbit@example.com
      ORBIT_SMTP_PASSWORD: change-me
      ORBIT_SMTP_FROM: orbit@example.com
      # Let people sign up without an invitation
      ORBIT_OPEN_REGISTRATION: 'true'
```

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml -f deploy/compose.override.yaml up -d --no-build
```

You can also set these settings in Admin. A setting that comes from a variable is read-only in Admin. Common
variables:

| Group | Variables | Details |
|---|---|---|
| Outgoing mail | `ORBIT_SMTP_HOST`, `ORBIT_SMTP_PORT`, `ORBIT_SMTP_SECURITY`, `ORBIT_SMTP_USERNAME`, `ORBIT_SMTP_PASSWORD`, `ORBIT_SMTP_FROM`, `ORBIT_SMTP_FROM_NAME` | [Instance settings](operations.md#instance-settings-from-environment-variables) |
| S3 storage and backups | `ORBIT_S3_ENDPOINT`, `ORBIT_S3_BUCKET`, `ORBIT_S3_REGION`, `ORBIT_S3_ACCESS_KEY_ID`, `ORBIT_S3_SECRET_ACCESS_KEY`, `ORBIT_S3_PATH_STYLE`, `ORBIT_S3_ATTACHMENTS`, `ORBIT_S3_BACKUPS`, `ORBIT_AUTOMATIC_BACKUPS` | [Object storage](operations.md#object-storage-s3-backups-and-a-lost-server) |
| Sign-in | `ORBIT_OPEN_REGISTRATION`, `ORBIT_GOOGLE_CLIENT_*`, `ORBIT_GITHUB_CLIENT_*`, `ORBIT_OIDC_*` | [Google and GitHub](operations.md#sign-in-with-google-and-github), [OpenID Connect](operations.md#sign-in-with-openid-connect) |
| Database | `ORBIT_DATABASE_URL`, `ORBIT_DATABASE_MAX_CONNECTIONS` | [Postgres](#postgres) |
| Limits | `ORBIT_UPLOAD_MAX_FILE_BYTES`, `ORBIT_UPLOAD_MAX_REQUEST_BYTES`, `ORBIT_RATE_LIMIT_*` | [Configuration](operations.md#configuration) |
| Incoming mail | `ORBIT_MAIL_*` | [Mail](operations.md#mail) and the comments in `deploy/compose.yaml` |

Each secret also has a `_FILE` variable (for example `ORBIT_SMTP_PASSWORD_FILE`) that names a file with the secret.
To check the configuration, run:

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml run --rm orbit config check
```

## Postgres

SQLite is the default and needs no setup. To use Postgres, set `ORBIT_DATABASE_URL`. The database must use the
`builtin` locale provider with `C.UTF-8` (Postgres 17 or later); Orbit refuses other databases. This override adds a
Postgres service:

```yaml
services:
  orbit:
    environment:
      ORBIT_DATABASE_URL: postgres://orbit:change-me@postgres:5432/orbit
    depends_on: [postgres]
  postgres:
    image: postgres:17
    restart: unless-stopped
    environment:
      POSTGRES_USER: orbit
      POSTGRES_PASSWORD: change-me
      POSTGRES_DB: orbit
      POSTGRES_INITDB_ARGS: --encoding=UTF8 --locale-provider=builtin --builtin-locale=C.UTF-8 --locale=C.UTF-8
    volumes: [postgres-data:/var/lib/postgresql/data]
    networks: [backend]
volumes:
  postgres-data:
```

`POSTGRES_INITDB_ARGS` takes effect only when the volume is empty. Files and backups stay in the Orbit volumes until
you turn on S3 in **Admin → Storage**. See [Postgres](operations.md#postgres).

## Backups and upgrades

Make backups in **Admin → Backups**, or turn on automatic backups in **Admin → Storage**. A backup in the
`orbit-backups` volume is on the same disk: copy it off the server, or send backups to S3.

To upgrade:

1. Make a backup and check that it is complete.
2. Pull the new image and restart:

   ```sh
   docker compose --env-file deploy/.env -f deploy/compose.yaml pull
   docker compose --env-file deploy/.env -f deploy/compose.yaml up -d --no-build
   ```

Orbit runs the database migrations at startup. An older image cannot open a newer database, so keep the old image
tag and its backup until the new version works. To stay on one version, set an exact tag in `ORBIT_IMAGE`.

Never run two Orbit containers on one SQLite database.

## Useful commands

Each command starts with `docker compose --env-file deploy/.env -f deploy/compose.yaml`:

| Command | Does |
|---|---|
| `logs -f orbit` | Shows the log. |
| `exec orbit /orbit config show` | Shows the effective configuration, secrets redacted. |
| `exec orbit /orbit backup list` | Lists backups. |
| `exec orbit /orbit db` | Opens a SQL shell. Make a backup first. |
| `run --rm orbit setup-token rotate` | Makes a new setup URL. On SQLite, stop `orbit` first. |

## Your own reverse proxy

To use another proxy than Caddy (Traefik, nginx, Tailscale), remove the `proxy` service, put Orbit on the proxy's
network, and set `ORBIT_TRUSTED_PROXIES` to `127.0.0.1/32` plus the exact address of the proxy. The proxy must send
`X-Forwarded-For` and `X-Forwarded-Proto` and forward WebSocket upgrades. Never trust `0.0.0.0/0`: then anyone who
reaches port 8080 can fake their address and HTTPS. `ORBIT_PUBLIC_ORIGIN` must be the exact `https://` URL that
users open.

More details: [`deploy/README.md`](../deploy/README.md).
