# Self-hosting with one Docker container (SQLite)

This guide runs Orbit as one container with `docker run`, without Compose. Orbit keeps its data in SQLite, inside a
Docker volume. You put your own HTTPS reverse proxy (Caddy, nginx, Traefik) in front of it. For the Compose setup
with Caddy included, see [Self-hosting with Docker](self-hosting.md). For Postgres, see
[Docker and Postgres](self-hosting-postgres.md).

## Requirements

- A Linux server with Docker.
- A DNS name (for example `orbit.example.com`) that points to the server.
- An HTTPS reverse proxy on the server. Orbit in production accepts only HTTPS requests from a trusted proxy.

## 1. Make the settings file

```sh
sudo mkdir -p /etc/orbit
sudo tee /etc/orbit/orbit.env >/dev/null <<EOF
ORBIT_PUBLIC_ORIGIN=https://orbit.example.com
ORBIT_TRUSTED_PROXIES=127.0.0.1/32,172.17.0.1/32
ORBIT_APP_KEY=$(openssl rand -base64 32)
EOF
sudo chmod 600 /etc/orbit/orbit.env
```

| Variable | Value |
|---|---|
| `ORBIT_PUBLIC_ORIGIN` | The exact `https://` URL that users open. |
| `ORBIT_TRUSTED_PROXIES` | `127.0.0.1/32` for the image healthcheck, plus the Docker bridge gateway: a proxy on the host reaches the published port from this address. `172.17.0.1` is the default. Check yours with `docker network inspect bridge -f '{{(index .IPAM.Config 0).Gateway}}'`. |
| `ORBIT_APP_KEY` | Orbit encrypts saved secrets (SMTP password, S3 secret, GitHub App keys, 2FA) with it. Keep a copy of `/etc/orbit/orbit.env` off the server: without the key, Orbit cannot read these secrets. |

The image already sets `ORBIT_ENV=production`, `ORBIT_BIND=0.0.0.0`, the SQLite path
`/var/lib/orbit/orbit.sqlite`, attachments in `/var/lib/orbit/attachments` and backups in `/var/backups/orbit`.

Put more `ORBIT_*` variables in the same file, one per line, for example outgoing mail:

```sh
ORBIT_SMTP_HOST=smtp.example.com
ORBIT_SMTP_USERNAME=orbit@example.com
ORBIT_SMTP_PASSWORD=change-me
ORBIT_SMTP_FROM=orbit@example.com
```

Orbit refuses to start when it finds an unknown `ORBIT_*` variable. For all variables, see
[Configuration](operations.md#configuration) and
[Instance settings from environment variables](operations.md#instance-settings-from-environment-variables).

## 2. Start Orbit

```sh
docker run -d --name orbit --restart unless-stopped \
  --read-only --tmpfs /tmp:size=64m --cap-drop ALL --security-opt no-new-privileges \
  --env-file /etc/orbit/orbit.env \
  -p 127.0.0.1:8080:8080 \
  -v orbit-data:/var/lib/orbit \
  -v orbit-backups:/var/backups/orbit \
  ghcr.io/coollabsio/orbit:latest
```

- `-p 127.0.0.1:8080:8080` publishes the port only on loopback, so only the proxy on this server can reach it. Do not
  publish it on all addresses: the gateway address is trusted, so anyone who reaches the port could fake their
  address and HTTPS.
- The named volumes keep the database, attachments and backups. Use named volumes, not host directories: the
  container runs as UID 65532 and must own the files.
- Image tags: `latest`, a minor version such as `0.4`, or an exact version such as `0.4.1`.

Check that the container is healthy:

```sh
docker ps --filter name=orbit --format '{{.Status}}'
```

## 3. Set up the reverse proxy

The proxy must send `X-Forwarded-For` and `X-Forwarded-Proto` and forward WebSocket upgrades. Caddy does all of
this. A `Caddyfile` on the host:

```
orbit.example.com {
    reverse_proxy 127.0.0.1:8080
}
```

## 4. Create the root account

```sh
docker logs orbit 2>&1 | grep 'setup URL'
```

Keep the URL secret. Open it and create the root account (the only account that can open Admin). Orbit then asks for
your first workspace and project. If the URL is lost, stop Orbit and run
`docker run --rm --env-file /etc/orbit/orbit.env -v orbit-data:/var/lib/orbit ghcr.io/coollabsio/orbit:latest setup-token rotate`,
then start it again.

## Backups

Make backups in **Admin → Backups**, or turn on automatic backups in **Admin → Storage**. They are in the
`orbit-backups` volume, on the same disk: copy them off the server, or send backups to S3.

The CLI can make a backup only while Orbit is stopped, because the running server owns the SQLite file:

```sh
docker stop orbit
docker run --rm --read-only --tmpfs /tmp --env-file /etc/orbit/orbit.env \
  -v orbit-data:/var/lib/orbit -v orbit-backups:/var/backups/orbit \
  ghcr.io/coollabsio/orbit:latest backup create
docker start orbit
```

`docker exec orbit /orbit backup list` lists the backups while Orbit runs.

## Upgrade

1. Make a backup and check that it is complete.
2. Pull the new image, then replace the container with the same `docker run` command as in step 2:

   ```sh
   docker pull ghcr.io/coollabsio/orbit:latest
   docker stop orbit && docker rm orbit
   docker run -d --name orbit ...   # the same command as in step 2
   ```

Orbit runs the database migrations at startup. An older image cannot open a newer database, so keep the old image tag
and its backup until the new version works. Never run two Orbit containers on one SQLite volume.

## Useful commands

| Command | Does |
|---|---|
| `docker logs -f orbit` | Shows the log. |
| `docker exec orbit /orbit config check` | Checks the configuration. |
| `docker exec orbit /orbit config show` | Shows the effective configuration, secrets redacted. |
| `docker exec -it orbit /orbit db` | Opens a SQLite shell. Make a backup first. |
