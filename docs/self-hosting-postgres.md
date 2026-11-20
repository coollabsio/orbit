# Self-hosting with Docker and Postgres

This guide runs Orbit and Postgres as two containers with `docker run`, without Compose. You put your own HTTPS
reverse proxy (Caddy, nginx, Traefik) in front of Orbit. For SQLite in one container, see
[one Docker container (SQLite)](self-hosting-sqlite.md). For Compose with Caddy included, see
[Self-hosting with Docker](self-hosting.md).

Use Postgres when the database must be on another host or several Orbit servers share one database. Otherwise
SQLite is simpler.

## Requirements

- A Linux server with Docker.
- A DNS name (for example `orbit.example.com`) that points to the server.
- An HTTPS reverse proxy on the server. Orbit in production accepts only HTTPS requests from a trusted proxy.

## 1. Make the network

```sh
docker network create --subnet 172.31.0.0/24 orbit
```

The fixed subnet gives the network a known gateway, `172.31.0.1`. A proxy on the host reaches the published Orbit
port from this address. If your host uses this subnet, choose another one and change the gateway in step 3.

## 2. Start Postgres

```sh
docker run -d --name orbit-postgres --restart unless-stopped --network orbit \
  -e POSTGRES_USER=orbit \
  -e POSTGRES_PASSWORD=change-me \
  -e POSTGRES_DB=orbit \
  -e POSTGRES_INITDB_ARGS='--encoding=UTF8 --locale-provider=builtin --builtin-locale=C.UTF-8 --locale=C.UTF-8' \
  -v orbit-postgres:/var/lib/postgresql/data \
  postgres:17
```

- Orbit refuses a database that does not use the `builtin` locale provider with `C.UTF-8` (Postgres 17 or later).
  `POSTGRES_INITDB_ARGS` takes effect only when the volume is empty.
- Use a strong password, for example from `openssl rand -hex 24`.
- Do not publish the Postgres port. Orbit reaches it on the `orbit` network.

To use a Postgres server that you already have, create the database there:

```sql
CREATE DATABASE orbit TEMPLATE template0 ENCODING 'UTF8'
  LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8' LOCALE 'C.UTF-8';
```

## 3. Make the settings file

```sh
sudo mkdir -p /etc/orbit
sudo tee /etc/orbit/orbit.env >/dev/null <<EOF
ORBIT_PUBLIC_ORIGIN=https://orbit.example.com
ORBIT_TRUSTED_PROXIES=127.0.0.1/32,172.31.0.1/32
ORBIT_APP_KEY=$(openssl rand -base64 32)
ORBIT_DATABASE_URL=postgres://orbit:change-me@orbit-postgres:5432/orbit
EOF
sudo chmod 600 /etc/orbit/orbit.env
```

| Variable | Value |
|---|---|
| `ORBIT_PUBLIC_ORIGIN` | The exact `https://` URL that users open. |
| `ORBIT_TRUSTED_PROXIES` | `127.0.0.1/32` for the image healthcheck, plus the gateway of the `orbit` network. |
| `ORBIT_APP_KEY` | Orbit encrypts saved secrets (SMTP password, S3 secret, GitHub App keys, 2FA) with it. Every Orbit server of this database needs the same key. Keep a copy of `/etc/orbit/orbit.env` off the server: without the key, Orbit cannot read these secrets. |
| `ORBIT_DATABASE_URL` | The Postgres URL. `orbit-postgres` is the container name from step 2. |
| `ORBIT_DATABASE_MAX_CONNECTIONS` | Optional. The size of each pool, default `20`. One server opens two pools. |

Put more `ORBIT_*` variables in the same file, one per line (for example `ORBIT_SMTP_*` for outgoing mail). Orbit
refuses to start when it finds an unknown `ORBIT_*` variable. For all variables, see
[Configuration](operations.md#configuration) and
[Instance settings from environment variables](operations.md#instance-settings-from-environment-variables).

## 4. Start Orbit

```sh
docker run -d --name orbit --restart unless-stopped --network orbit \
  --read-only --tmpfs /tmp:size=64m --cap-drop ALL --security-opt no-new-privileges \
  --env-file /etc/orbit/orbit.env \
  -p 127.0.0.1:8080:8080 \
  -v orbit-data:/var/lib/orbit \
  -v orbit-backups:/var/backups/orbit \
  ghcr.io/coollabsio/orbit:latest
```

- `-p 127.0.0.1:8080:8080` publishes the port only on loopback. Do not publish it on all addresses: the gateway
  address is trusted, so anyone who reaches the port could fake their address and HTTPS.
- The database is in Postgres, but attachments and backups stay in the `orbit-data` and `orbit-backups` volumes until
  you turn on S3 in **Admin → Storage**. Turn it on to make the Orbit server replaceable.
- Image tags: `latest`, a minor version such as `0.4`, or an exact version such as `0.4.1`.

Check that the container is healthy:

```sh
docker ps --filter name=orbit --format '{{.Names}} {{.Status}}'
```

## 5. Set up the reverse proxy

The proxy must send `X-Forwarded-For` and `X-Forwarded-Proto` and forward WebSocket upgrades. Caddy does all of
this. A `Caddyfile` on the host:

```
orbit.example.com {
    reverse_proxy 127.0.0.1:8080
}
```

## 6. Create the root account

```sh
docker logs orbit 2>&1 | grep 'setup URL'
```

Keep the URL secret. Open it and create the root account (the only account that can open Admin). Orbit then asks for
your first workspace and project. If the URL is lost, run `docker exec orbit /orbit setup-token rotate`. On Postgres
it works while Orbit runs.

## Backups

Make backups in **Admin → Backups**, or turn on automatic backups in **Admin → Storage**. On Postgres the CLI also
works while Orbit runs:

```sh
docker exec orbit /orbit backup create
docker exec orbit /orbit backup list
```

Backups go to the `orbit-backups` volume, on the same disk: copy them off the server, or send backups to S3. An
Orbit backup is the supported way to restore Orbit. A `pg_dump` of the database alone does not contain the
attachment files.

## Upgrade

1. Make a backup and check that it is complete.
2. Pull the new image, then replace the Orbit container with the same `docker run` command as in step 4:

   ```sh
   docker pull ghcr.io/coollabsio/orbit:latest
   docker stop orbit && docker rm orbit
   docker run -d --name orbit ...   # the same command as in step 4
   ```

Orbit runs the database migrations at startup. An older image cannot open a newer database, so keep the old image tag
and its backup until the new version works. Upgrade the Postgres major version separately, with a backup first.

## Useful commands

| Command | Does |
|---|---|
| `docker logs -f orbit` | Shows the log. |
| `docker exec orbit /orbit config check` | Checks the configuration. |
| `docker exec orbit /orbit config show` | Shows the effective configuration, secrets and the database password redacted. |
| `docker exec -it orbit /orbit db` | Opens a `psql` shell on the Orbit database. Make a backup first. |

For several Orbit servers on one database, see
[Orbit Cloud: one Postgres, several Orbit servers](operations.md#orbit-cloud-one-postgres-several-orbit-servers).
