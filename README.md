# Orbit

> PRETTY ALPHA, DO NOT USE IT IN PROD.

Orbit is a self-hosted workspace for teams. It puts tasks, documents, mail, and
team chat in one application, so your team's work stays in one place and on
your own server. The goal is an AI assistant that has the full context of every
part of the workspace.

## What is in Orbit

- **Tasks**: projects with their own workflows, list and board views, sub-issues,
  labels, comments, attachments, saved views, and GitHub sync.
- **Docs**: a page tree with a rich editor, search, and trash.
- **Chat**: channels, direct messages, threads, files, and search.
- **Mail**: a shared mail client. Work in progress; it uses mock data for now.
- **AI assistant**: planned. Today, AI clients can read tasks through the
  [MCP server](docs/mcp.md).
- **Administration**: workspaces, members, invitations, roles, sessions, API
  tokens, audit records, and instance settings.

## License

Orbit is licensed under the [Apache License 2.0](LICENSE).

## Technical details

### How it works

Orbit is one binary. A Rust server serves the API and the embedded React web
application from one HTTP listener. Data is kept in SQLite. Attachments can be
kept on local disk or in S3-compatible storage. Orbit can make scheduled,
verified backups. Put a reverse proxy in front of Orbit for HTTPS.

### Development

You need Rust 1.97.1, Bun 1.3.14 or later, and [Just](https://just.systems/).

```bash
just setup
just dev
```

Open <http://127.0.0.1:8888>. Run `just test` for the test suites or `just
check` for all project checks.

### Connect an AI client

Orbit has a read-only MCP endpoint at `/mcp` with built-in OAuth. To connect
Claude, Codex, or Grok, see [docs/mcp.md](docs/mcp.md).

### Documentation

- [Deployment and operations](docs/operations.md)
- [MCP server and client setup](docs/mcp.md)
- [GitHub integration](docs/github.md)
