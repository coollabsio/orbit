# Orbit

A self-hosted workspace for teams: tasks, docs, chat, and mail in one app on
your own server. AI clients connect over MCP and get the full context of your
workspace without your data leaving your server.

## Features

- **Tasks**: projects with custom workflows, list, board and timeline views,
  sub-issues, labels, saved views, trash, GitHub sync, and Asana import.
- **Docs**: page tree with live collaborative editing, version history, search,
  and Notion import.
- **Chat**: channels, direct messages, threads, files, and search.
- **Mail**: shared mail client (in progress, mock data).
- **AI**: [MCP server](docs/mcp.md) with OAuth. Read and write tasks, read docs
  and chat. Set it up in **Account settings → Agentic**.
- **Admin**: workspaces, members, roles, API tokens, audit log, email, rate
  limits, storage, and workspace export.
- **Backups**: scheduled, verified backups to S3, with restore of single pages
  or tasks.
- **Mobile**: iOS and Android app (`apps/mobile`). Saved views, timeline, tasks
  trash, GitHub settings, export, API tokens, and Notion and Asana import are web-only.

## How it works

One binary. A Rust server serves the API and the embedded React app. Data is in
SQLite (default) or Postgres, files on local disk or S3. Put a reverse proxy in
front for HTTPS. Docker image: `ghcr.io/coollabsio/orbit:latest`.

## Development

Needs Rust 1.97.1, Bun 1.4.4+, and [Just](https://just.systems/).

```bash
just setup
just dev   # http://127.0.0.1:8888
```

`just test` runs the tests, `just check` runs all checks.

## Docs

- [Deployment and operations](docs/operations.md)
- [MCP server and client setup](docs/mcp.md)
- [GitHub integration](docs/github.md)

## License

[Apache 2.0](LICENSE)
