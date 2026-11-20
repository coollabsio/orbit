# Orbit

A self-hosted workspace for teams: tasks, docs, chat, and mail in one app on
your own server. AI clients connect over MCP and get the full context of your
workspace without your data leaving your server.

## Features

- **Tasks**: projects with custom workflows, list, board and timeline views,
  sub-issues, labels, saved views, trash, GitHub sync, and Asana, Linear and Trello import.
- **Docs**: page tree with live collaborative editing, version history, search,
  and Notion import.
- **Chat**: channels, direct messages, threads, files, and search.
- **Mail**: shared mail client (in progress, mock data).
- **AI**: [MCP server](docs/mcp.md) with OAuth. Read and write tasks and docs,
  read chat. Set it up in **Account settings → Agentic**.
- **Admin**: workspaces, members, roles, API tokens, audit log, email, rate
  limits, storage, and workspace export.
- **Backups**: scheduled, verified backups to S3, with restore of single pages
  or tasks.
- **Mobile**: iOS and Android app (`apps/mobile`). Saved views, timeline, tasks
  trash, GitHub settings, export, API tokens, and Notion, Asana, Linear and Trello import are web-only.

## How it works

One binary. A Rust server serves the API and the embedded React app. Data is in
SQLite (default) or Postgres, files on local disk or S3. Put a reverse proxy in
front for HTTPS. Docker image: `ghcr.io/coollabsio/orbit:latest`.

Set instance settings (mail server, S3 storage, Google and GitHub sign-in, open
registration, automatic backups) in Admin, or with `ORBIT_*` environment
variables for declarative deployments such as Kubernetes. A setting from a
variable is read-only in Admin. See [Deployment and operations](docs/operations.md).

## Self-hosting with Docker

The simplest setup: the Docker image, with SQLite in a volume. Needs Docker and
an HTTPS reverse proxy (for example Caddy) on the server.

```bash
sudo mkdir -p /etc/orbit
sudo tee /etc/orbit/orbit.env >/dev/null <<EOF
ORBIT_PUBLIC_ORIGIN=https://orbit.example.com
ORBIT_TRUSTED_PROXIES=127.0.0.1/32,172.17.0.1/32
ORBIT_APP_KEY=$(openssl rand -base64 32)
EOF
docker run -d --name orbit --restart unless-stopped \
  --env-file /etc/orbit/orbit.env -p 127.0.0.1:8080:8080 \
  -v orbit-data:/var/lib/orbit -v orbit-backups:/var/backups/orbit \
  ghcr.io/coollabsio/orbit:latest
docker logs orbit 2>&1 | grep 'setup URL'
```

Point the proxy at `127.0.0.1:8080`, open the setup URL, and create the root
account. Keep a copy of `ORBIT_APP_KEY` off the server. Details, variables,
backups and upgrades: [one Docker container (SQLite)](docs/self-hosting-sqlite.md).
Other setups: [Docker and Postgres](docs/self-hosting-postgres.md) and
[Docker Compose with Caddy](docs/self-hosting.md).

## Development

Needs Rust 1.97.1, Bun 1.4.4+, and [Just](https://just.systems/).

```bash
just setup
just dev   # http://127.0.0.1:8888
```

`just test` runs the tests, `just check` runs all checks.

## Docs

- [Self-hosting with one Docker container (SQLite)](docs/self-hosting-sqlite.md)
- [Self-hosting with Docker and Postgres](docs/self-hosting-postgres.md)
- [Self-hosting with Docker Compose and Caddy](docs/self-hosting.md)
- [Deployment and operations](docs/operations.md)
- [MCP server and client setup](docs/mcp.md)
- [GitHub integration](docs/github.md)
- [Chat incoming webhooks](docs/chat-webhooks.md)
- [Outgoing task webhooks](docs/webhooks.md)

## License

[Apache 2.0](LICENSE)
