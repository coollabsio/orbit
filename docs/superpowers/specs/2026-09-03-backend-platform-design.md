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

### D-008: Support multiple workspaces in each installation

**Decision:** A single Orbit installation supports multiple isolated workspaces from the initial schema onward.

**Rationale:** Workspace ownership affects nearly every persistent resource, authorization decision, uniqueness rule, job, event, and stored file. Including that boundary from the beginning is substantially simpler and safer than retrofitting tenancy after application data and APIs exist.

**Alternatives considered:**

- **One workspace per installation:** Rejected because it would embed installation-level assumptions throughout the model and make later multi-workspace support a broad, risky migration.
- **One database per workspace:** Rejected initially because it complicates connection management, migrations, backups, cross-workspace identity, and operations without providing necessary isolation for the intended deployment model.

**Consequences:**

- Workspaces share one SQLite database within an installation.
- Every workspace-owned aggregate must carry an explicit `workspace_id`.
- Workspace-scoped uniqueness constraints must include `workspace_id`.
- Authorization, jobs, events, file paths, test fixtures, and queries must preserve workspace context.
- Automated tests must attempt cross-workspace access and prove that it is rejected.
- Custom workspace domains, per-workspace databases, and workspace billing are not implied by this decision.

### D-009: Use global user identities with workspace memberships

**Decision:** A user has one global account within an Orbit installation and may belong to multiple workspaces through explicit memberships.

**Rationale:** Global identity avoids duplicate credentials and sessions, and it gives users a direct way to switch among workspaces without signing into separate accounts.

**Alternatives considered:**

- **Separate account per workspace:** Rejected because it duplicates identity and complicates login, recovery, session management, and workspace switching.
- **Globally shared access without membership records:** Rejected because workspace authorization and roles require an explicit relationship.

**Consequences:**

- Authentication establishes the global user identity; authorization additionally requires a valid membership in the selected workspace.
- Roles and workspace-specific user settings belong to the membership rather than the global user where appropriate.
- Sessions may remember a last-selected workspace, but that selection never substitutes for a membership check.
- Removing a membership revokes access to that workspace without deleting the global account or its other memberships.
- Invitations target an identity attribute such as an email address and create or attach a membership when accepted.

### D-010: Make foundation and tasks the first backend milestone

**Decision:** The first usable backend milestone will deliver global account login and sessions; workspace creation and switching; invitations, memberships, and basic roles; projects and project-specific statuses; tasks, assignees, labels, comments, and task attachments; and the first real frontend API integration for those capabilities.

**Rationale:** Tasks provide a broad but manageable vertical slice through authentication, tenancy, authorization, relational data, validation, uploads, API conventions, and optimistic frontend mutations. This validates the platform on real product behavior without making realtime chat or mail synchronization prerequisites.

**Alternatives considered:**

- **Foundation without a product feature:** Rejected because it would validate infrastructure in isolation while delivering little user-visible value.
- **Foundation and chat:** Deferred because realtime delivery, ordering, presence, and reconnection would introduce too many new concerns into the first slice.
- **Implement every mocked feature together:** Rejected because the scope would delay feedback and make platform mistakes expensive to unwind.

**Consequences:**

- The milestone is complete only when the listed task and workspace flows persist across application restarts and are exercised through the frontend.
- Task attachments require an initial authenticated file-storage path within this milestone.
- The first API, authorization, error, testing, and frontend data-access conventions will be proven by this slice and reused later.
- Docs, chat, direct messages, notifications, mail, webhooks, custom emoji, and other administration features remain outside this milestone unless needed directly by the accepted scope.
- Realtime infrastructure is not a prerequisite for the first milestone.

### D-011: Keep unmigrated mock features accessible

**Decision:** During incremental backend migration, product areas without a real backend remain accessible using mock data while migrated foundation and task areas use persistent server data.

**Rationale:** Keeping the existing mock surfaces available preserves the complete product reference and allows continued UX evaluation while backend capabilities are implemented in bounded vertical slices.

**Alternatives considered:**

- **Hide every unmigrated feature:** Rejected because it would remove useful working mockups and make the application appear less complete during backend development.
- **Wait and switch the entire frontend at once:** Rejected because it creates a large integration step and delays validation of the first backend conventions.

**Consequences:**

- Server-owned and mock-owned domains must have an explicit boundary; the mock store must not overwrite or impersonate persistent foundation or task records.
- Navigation between real and mock areas remains available.
- Mock-only surfaces must be visibly identified in development so behavior is not mistaken for persistence.
- Cross-domain mock relationships may use display-only fixtures but must not create false guarantees about server data.
- Each later feature milestone removes its corresponding mock domain until the mock layer can be deleted.

### D-012: Start with email and password authentication

**Decision:** Initial authentication uses email addresses and passwords, backed by secure server-managed sessions. The initial capability also includes workspace invitations, password recovery, and a supported way to create the first administrator.

**Rationale:** Email and password authentication keeps self-hosted installations independent of an external identity provider and is sufficient to validate the identity, membership, session, and authorization foundations. It avoids adding passkey and OAuth complexity to the first milestone.

**Alternatives considered:**

- **Password and passkeys from the beginning:** Deferred because credential enrollment, recovery, browser behavior, and additional test paths would expand the first milestone.
- **External identity only:** Rejected initially because it would make a self-contained installation depend on provider configuration and availability.

**Consequences:**

- Passwords must use a modern password-hashing algorithm with per-password salts and an upgradeable cost policy.
- Login establishes a global identity; workspace access is still determined by memberships.
- Browser authentication uses cookies with secure defaults rather than exposing long-lived credentials to frontend JavaScript.
- Password reset tokens must be single-use, time-limited, and stored so a database disclosure does not reveal usable reset links.
- Passkeys and external identity providers remain compatible future additions, not first-milestone requirements.
- Account enumeration, login throttling, session invalidation, cookie attributes, and recovery delivery require explicit decisions in the security and authentication sections.

### D-013: Make account registration invite-only

**Decision:** Initial account registration is invite-only. There is no public self-registration endpoint or installation setting that enables it in the first milestone.

**Rationale:** Orbit is initially a self-hosted workspace application. Invite-only registration gives workspace administrators explicit control over membership and avoids making abuse prevention for public registration part of the first milestone.

**Alternatives considered:**

- **Open registration:** Rejected because anyone who could reach the installation could create an account, requiring additional abuse controls and workspace-creation policies.
- **Administrator-configurable registration:** Deferred because its flexibility does not justify another policy branch and test matrix in the first milestone.

**Consequences:**

- The installation needs a separate, secure bootstrap flow for its first administrator.
- After bootstrap, new accounts are created only through accepted invitations.
- Invitation tokens must be single-use, time-limited, and stored so a database disclosure does not expose usable invitations.
- Invitations must identify the target workspace and intended initial role.
- Accepting an invitation either creates a global account or adds a membership to an existing global account with the matching verified identity.
- Public registration can be considered later as a separate feature and threat-model decision.

### D-014: Bootstrap the first administrator in the browser

**Decision:** An unconfigured Orbit installation presents a one-time browser setup flow that creates the first global administrator, initial workspace, and owner membership.

**Rationale:** A browser flow gives self-hosted operators a direct onboarding experience without requiring familiarity with container shells or application CLI commands.

**Alternatives considered:**

- **CLI-only bootstrap:** Rejected as the primary flow because it adds operational friction to first-time setup.
- **Environment-provided credentials:** Rejected because passwords and other long-lived credentials can leak through deployment configuration, process environments, or automation logs.

**Consequences:**

- The setup route is available only while no global user exists.
- The database transaction that creates the first user, workspace, and owner membership must be atomic and must prevent two concurrent requests from both succeeding.
- After successful setup, the route permanently behaves as unavailable for that database; deleting the administrator must not silently reopen bootstrap.
- Recovery from a lost final administrator requires an explicit administrative recovery procedure rather than re-enabling public bootstrap automatically.
- The setup form uses the same password policy and secure session creation as normal authentication.
- The mechanism used to prove that the browser user controls the installation remains an open security decision.

### D-015: Protect browser bootstrap with a one-time setup token

**Decision:** On first boot, Orbit generates a cryptographically random one-time setup token and prints a setup URL containing that token to the operator-visible console. The browser setup flow requires the token before it can create the first administrator.

**Rationale:** A one-time token preserves browser-based onboarding while preventing an arbitrary person who discovers an unconfigured installation from claiming it.

**Alternatives considered:**

- **Restrict setup by source address:** Rejected because reverse proxies obscure source addresses and private networks do not establish operator identity.
- **Leave setup open until claimed:** Rejected because deployment and discovery can race, allowing an unauthorized user to become the first administrator.

**Consequences:**

- The token must be generated using a cryptographically secure random source.
- Only a non-reversible hash of the token may be persisted.
- The token must expire and be invalidated atomically when setup succeeds.
- Startup output must avoid printing the token again after the installation has been configured.
- The operator needs an explicit local administrative command to rotate an unused or expired setup token.
- Setup responses and application logs must not echo or retain the plaintext token.
- The browser should remove the token from its address bar after exchanging or validating it so it is not retained in history or sent as a referrer.

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
