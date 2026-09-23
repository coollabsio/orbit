# Orbit

> PRETTY ALPHA, DO NOT USE IT IN PROD.

Orbit is a self-hosted workspace application for projects, tasks, documents,
mail, and team chat. It uses a Rust server, a React web client, and SQLite. The
production binary serves both the API and the embedded web application.

## Development

You need Rust 1.97.1, Bun 1.3.14 or later, and [Just](https://just.systems/).

```bash
just setup
just dev
```

Open <http://127.0.0.1:8888>. Run `just test` for the test suites or `just
check` for all project checks.

For deployment and operator instructions, see [docs/operations.md](docs/operations.md).
For GitHub webhook setup and current limits, see [docs/github.md](docs/github.md).
The project is licensed under the [Apache License 2.0](LICENSE).
