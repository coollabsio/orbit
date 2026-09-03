# Orbit backend platform design

**Date:** 2026-09-03  
**Status:** Living decision document; discovery in progress

## Purpose

Orbit currently has a functional React frontend backed by an in-memory mock store. This document records the decisions for replacing that mock boundary with a real backend and, where justified by Orbit's needs, extracting reusable infrastructure for future internal applications.

This is not yet an implementation specification. Unresolved areas remain explicitly listed, and implementation must not begin until the design is complete and approved.

## Decision principles

- Record only decisions that have been explicitly accepted.
- Include rationale, alternatives, and consequences for each decision.
- Revisit a decision when implementation evidence invalidates an assumption.
- Build abstractions from real Orbit requirements rather than copying framework APIs speculatively.
- Prefer the simplest design that meets the agreed deployment model.

## Accepted decisions

### D-001: Use Rust for the backend

**Decision:** Implement the backend in Rust.

**Rationale:** Rust supports a compact, predictable application artifact with low runtime overhead. Developing the backend also provides an opportunity to establish reusable conventions for future internal applications.

**Alternatives considered:**

- **Laravel:** Excellent development experience and a mature set of integrated facilities. It remains the benchmark for the developer experience we want, but it does not match the chosen Rust and compact-runtime direction.
- **Other managed runtimes:** Not explored because the current preference is specifically between Laravel's integrated experience and a Rust application platform.

**Consequences:**

- The team owns more integration and convention work than it would with Laravel.
- Proven Rust crates should provide infrastructure primitives wherever possible.
- AI can reduce implementation effort, but it does not replace production semantics, maintenance, security review, documentation, or operational ownership.

### D-002: Build a modular monolith

**Decision:** Build one modular application rather than microservices.

**Rationale:** Orbit has many connected product areas, but there is no demonstrated need for independently deployed services. A modular monolith keeps transactions, development, deployment, and debugging straightforward.

**Alternatives considered:**

- **Microservices:** Rejected for the initial system because they would add network, deployment, consistency, and observability complexity without a demonstrated scaling requirement.

**Consequences:**

- Modules need clear interfaces and ownership boundaries inside one codebase.
- Modules may share a process and database but should not bypass established domain boundaries.
- Deployment initially consists of one application artifact plus its durable files.

### D-003: Treat reusable infrastructure as an internal application platform

**Decision:** Develop reusable backend conventions for Orbit and future applications owned by the same team. Do not design a public general-purpose framework.

**Rationale:** Internal reuse provides the desired framework-building benefits without requiring public API stability, a plugin ecosystem, universal configuration, or support for unknown applications.

**Alternatives considered:**

- **Orbit-only code with no reuse:** Rejected because reusable infrastructure is an explicit project goal.
- **Public Rust framework:** Deferred because it would require substantially stronger compatibility, documentation, extension, release, and support guarantees.

**Consequences:**

- Breaking platform APIs is acceptable while the design matures.
- Documentation can focus on team workflows and concrete recipes.
- No public plugin system or third-party extension contract is required.
- The platform should optimize for the team's preferred conventions rather than every possible application architecture.

### D-004: Extract platform capabilities from proven application needs

**Decision:** Build capabilities against real Orbit use cases first. Promote them into reusable platform modules only after their boundary is demonstrated or another internal application needs them.

**Rationale:** Recreating Laravel's surface API in advance would risk producing abstractions without validated semantics. Orbit should supply the production requirements that shape each reusable capability.

**Alternatives considered:**

- **Design the complete framework before Orbit:** Rejected because it encourages speculative abstractions and delays product feedback.
- **Never extract reusable modules:** Rejected because internal reuse is a project objective.

**Consequences:**

- Orbit-specific implementations may precede reusable platform modules.
- Initial module boundaries can change as evidence accumulates.
- Jobs, files, mail, and realtime behavior remain application concerns until reuse is justified.

### D-005: Use SQLite as the initial database

**Decision:** Use SQLite rather than PostgreSQL for the initial backend.

**Rationale:** SQLite supports the desired small, self-hosted deployment and is sufficient when one application instance owns a database. It removes a required external database service while retaining transactions, indexes, constraints, and reliable persistence.

**Alternatives considered:**

- **PostgreSQL:** More capable for concurrent writers, horizontal application replicas, row-level locking patterns, and database-native notifications. Deferred until those capabilities are required by a real deployment.

**Consequences:**

- The application must use WAL mode, a busy timeout, and short write transactions.
- Network filesystems and multiple live application instances sharing one SQLite file are unsupported.
- Long-running file or network operations must occur outside database transactions.
- Backup, restore, schema migration, and integrity-check procedures are part of the product design.
- Database-facing application boundaries should not gratuitously depend on SQLite-specific behavior when a future PostgreSQL implementation is plausible.

### D-006: Allow one running application instance per database

**Decision:** Support exactly one live Orbit server instance for each SQLite database in the initial deployment model.

**Rationale:** This constraint makes SQLite operation and durable job claiming substantially simpler and matches the compact self-hosted goal.

**Alternatives considered:**

- **Multiple active replicas sharing a database:** Rejected for SQLite because write contention, coordination, and shared-file deployment would undermine the simplicity being sought.
- **PostgreSQL-backed replicas:** Deferred until horizontal scaling or high-availability requirements justify them.

**Consequences:**

- In-process coordination is allowed where durable recovery is still preserved.
- Operational failover means stopping or losing the old instance before starting another against the same database.
- Horizontal scaling is not an initial requirement.

### D-007: Provide a durable SQLite-backed job system

**Decision:** Run durable background jobs using a SQLite table, with the HTTP server, scheduler, and job worker able to live in the same application binary.

**Rationale:** Orbit will need reliable asynchronous work for activities such as outbound mail, webhook delivery, notification fan-out, maintenance, and file processing. A database-backed queue avoids introducing Redis or another service.

**Alternatives considered:**

- **In-memory tasks only:** Rejected because queued work would be lost on shutdown or crash.
- **Redis or a separate queue service:** Rejected initially because it conflicts with the compact deployment goal and is not required by the single-instance model.
- **PostgreSQL queue:** Deferred with PostgreSQL itself.

**Consequences:**

- Jobs require explicit states, attempt counts, availability times, leases, retry backoff, and failure information.
- Claiming a job happens in a short transaction; execution happens after that transaction commits.
- Expired leases allow recovery after a crash.
- Handlers must tolerate at-least-once execution and therefore be idempotent where side effects matter.
- Initial coordination can use one job claimant with bounded concurrent execution.
- The application API should enqueue typed jobs without exposing queue-table details to domain code.

## Current architectural direction, not yet accepted

The following ideas have been discussed but are not decisions:

- Axum for HTTP and WebSocket handling.
- SQLx for database access.
- A reusable platform divided into core, HTTP, database, authentication, and testing capabilities.
- REST plus a generated TypeScript API client.
- Local filesystem storage with a possible S3-compatible adapter.
- A single artifact that may embed the built frontend.

These choices require explicit evaluation before implementation.

## Decision queue

Decisions will be resolved in dependency order:

1. Product scope and first backend milestone
2. Tenancy and workspace isolation
3. Authentication and session lifecycle
4. Authorization model
5. Core data conventions
6. SQLite access, migrations, backup, and recovery
7. Durable jobs and scheduling semantics
8. API contract, validation, errors, and client generation
9. Realtime delivery and reconnection
10. File and attachment storage
11. Mail responsibilities and providers
12. Platform/module boundaries
13. Configuration, secrets, deployment, and observability
14. Security controls
15. Testing and local developer experience
16. Incremental frontend migration

## Open decision 1: Product scope and first backend milestone

The first milestone must be small enough to validate the database, API, authentication, authorization, testing, and frontend integration conventions without attempting to replace every mock feature simultaneously.
