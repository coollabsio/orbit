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
- Account enumeration, login throttling, session invalidation, and remaining cookie details require explicit decisions in the security and authentication sections.

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
- The setup token in D-015 proves that the browser user controls the installation.

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

### D-016: Use sliding sessions with an absolute lifetime

**Decision:** Browser sessions expire after 30 days of inactivity and have an absolute maximum lifetime of 90 days from initial authentication.

**Rationale:** Sliding expiration avoids interrupting active users, while the absolute limit ensures that every browser must periodically prove possession of the account credentials again.

**Alternatives considered:**

- **Fixed 30-day lifetime:** Rejected because it can sign out a user who is actively using the application.
- **Short session with a remember-me option:** Rejected initially because it introduces another cookie and session-policy branch without a demonstrated need.

**Consequences:**

- Sessions use opaque cryptographically random tokens in `HttpOnly`, `Secure`, `SameSite=Lax` cookies; only a non-reversible token hash is stored.
- Activity may advance the idle expiry but never the original 90-day absolute expiry.
- Session activity writes should be throttled rather than updating SQLite on every request.
- Password changes, explicit sign-out, account suspension, and administrator revocation invalidate applicable sessions immediately.
- The user can view and revoke their active sessions; authorized administrators can revoke sessions as defined by policy.
- Sensitive future operations may require recent authentication even when the session remains valid.

### D-017: Support SMTP and administrator-issued password recovery

**Decision:** Send password-recovery links through SMTP when it is configured. When it is not, an installation administrator can generate a short-lived recovery URL for the affected user.

**Rationale:** Recovery must work in small self-hosted installations without making an email service mandatory or revealing a replacement password to an administrator.

**Alternatives considered:**

- **Require SMTP:** Rejected because it would add an external dependency to the first milestone.
- **Let administrators assign passwords:** Rejected because an administrator would know the user's new credential.

**Consequences:**

- Recovery URLs are single-use, time-limited, and persisted only as token hashes.
- Only installation administrators, not workspace administrators, may issue recovery links because credentials belong to global accounts.
- Generating a link is security-audited and must not expose it through ordinary application logs.
- SMTP delivery is asynchronous through the durable job system; the local recovery path remains available when SMTP is disabled or unhealthy.

### D-018: Use fixed workspace access roles initially

**Decision:** Authorization uses the fixed workspace roles Owner, Admin, and Member during the first milestone. The existing custom chat roles remain display-only until chat authorization is designed. Visitor invitations are deferred.

**Rationale:** Fixed roles establish a clear, testable policy model without adding a permission editor or conflating decorative chat roles with access control.

**Alternatives considered:**

- **Custom permission-bearing roles:** Deferred because it would require a full permission schema, editor, compatibility rules, and policy migration strategy.
- **Owner and Admin only:** Rejected because ordinary collaborating members are a core workflow.
- **Enable Visitor immediately:** Deferred until its resource visibility and project-scoping semantics are designed.

**Consequences:**

- Every workspace has exactly one Owner.
- Only the Owner may transfer ownership or delete the workspace.
- Admins can manage workspace settings, projects, invitations, members, and roles but cannot alter or remove the Owner.
- Members may create, edit, and delete task-area resources, including projects, statuses, tasks, comments, and attachments; destructive UI actions still use confirmations where appropriate.
- Owners and Admins retain the same content capabilities as Members.
- Backend policy checks, rather than UI visibility, enforce every permission.

### D-019: Use UUIDv7 identifiers

**Decision:** Persistent entities use UUIDv7 identifiers and expose them in canonical UUID string form at API boundaries.

**Rationale:** UUIDv7 is a standardized, time-ordered identifier that can be generated without database coordination and remains suitable for clients, imports, and a future PostgreSQL adapter.

**Alternatives considered:**

- **ULID:** Rejected in favor of the more universal UUID representation.
- **Integer identifiers:** Rejected because they require centralized allocation and are awkward for client-created records and future data movement.

**Consequences:**

- The database representation must preserve UUID identity and indexed ordering consistently.
- Authorization must never infer workspace ownership or trust ordering information from an identifier.

### D-020: Standardize UTC timestamp storage and transport

**Decision:** Persist timestamps as UTC Unix milliseconds in SQLite integer columns and expose them as RFC 3339 UTC strings in JSON.

**Rationale:** Integer storage is compact and sortable in SQLite, while RFC 3339 makes API values readable and unambiguous.

**Alternatives considered:**

- **RFC 3339 text in SQLite:** Rejected because it is larger and easier to format inconsistently.
- **Unix milliseconds in JSON:** Rejected because it is less readable and self-describing for API consumers.

**Consequences:**

- Public API schemas use RFC 3339 strings and generated clients map them consistently.
- Application code owns parsing, formatting, and millisecond precision; local time zones are presentation concerns only.

### D-021: Soft-delete major records

**Decision:** Workspaces, projects, and tasks are soft-deleted in the first milestone. Comments and attachment metadata may be hard-deleted, with durable cleanup jobs for stored files.

**Rationale:** Major records have broad relationships and high recovery value. Applying soft deletion selectively avoids turning every table into a recycle-bin system.

**Alternatives considered:**

- **Hard-delete all records:** Rejected because accidental deletion of major resources would be immediately irreversible.
- **Full recycle bin for every entity:** Deferred because user-facing restore workflows for every resource would expand the milestone substantially.

**Consequences:**

- Normal queries exclude deleted major records by default.
- Unique constraints and restore behavior must account for soft-deleted rows explicitly.
- Retention, permanent purge, and user-facing restoration need concrete policies before implementation.

### D-022: Detect update conflicts with record versions

**Decision:** Mutable records carry an integer version. Update and delete requests include the version observed by the client and receive a conflict response when it is stale.

**Rationale:** Optimistic concurrency prevents one browser from silently overwriting another user's edits without requiring complex field-level merge semantics.

**Alternatives considered:**

- **Last write wins:** Rejected because concurrent changes could be lost without warning.
- **Automatic field-level merging:** Deferred because it requires domain-specific conflict behavior.

**Consequences:**

- Successful mutations increment the version atomically.
- The API defines a consistent conflict error and returns enough current-record context for the frontend to refresh or prompt the user.
- TanStack Query optimistic updates must roll back or reconcile when a conflict occurs.

### D-023: Use SQLx repositories for persistence

**Decision:** Use SQLx with explicit SQL behind domain-oriented repository interfaces. Platform helpers standardize connection setup, transactions, identifiers, pagination, error mapping, and tests.

**Rationale:** Explicit SQL keeps SQLite behavior visible and controllable while typed repository methods provide consistent application-facing DX.

**Alternatives considered:**

- **SeaORM entities:** Rejected because the additional ORM and generated-entity abstraction is not currently justified.
- **A custom Eloquent-like ORM:** Rejected because implementing ORM semantics would distract from building proven application capabilities.

**Consequences:**

- HTTP handlers do not issue SQL directly.
- Repository interfaces follow domain use cases rather than exposing generic active-record operations.
- Migrations remain explicit, ordered, testable artifacts.

### D-024: Build HTTP APIs with Axum and Tower

**Decision:** Standardize the server on Axum for HTTP routing and extraction and Tower for middleware and service composition.

**Rationale:** Axum and Tower provide composable primitives within the Tokio ecosystem while leaving room for Orbit-specific conventions around errors, validation, authentication, and state.

**Alternatives considered:**

- **Actix Web:** Not selected because the team prefers the Tower ecosystem and compositional model.
- **Poem/OpenAPI:** Not selected because stronger framework coupling and a smaller ecosystem are unnecessary for the desired contract generation.

**Consequences:**

- The internal platform supplies narrow response, error, request-context, validation, and middleware conventions rather than wrapping all of Axum.
- Domain and repository layers remain independent of Axum types.

### D-025: Generate the TypeScript API client from OpenAPI

**Decision:** Rust HTTP endpoints define an OpenAPI contract from which CI generates the TypeScript client used by frontend domain services.

**Rationale:** A generated contract prevents request, response, and validation types from drifting while avoiding handwritten duplication across Rust and TypeScript.

**Alternatives considered:**

- **Handwritten TypeScript contracts:** Rejected because they can silently diverge from server behavior.
- **Generate types but handwrite requests:** Rejected because endpoint definitions and error handling would still be duplicated.

**Consequences:**

- Generated code is not edited manually.
- Contract generation must be deterministic and checked in CI.
- Feature components consume frontend services or hooks rather than importing transport details throughout the view layer.

### D-026: Put workspace context in API paths

**Decision:** Workspace-scoped APIs include the workspace identifier explicitly in the URL, for example `/api/v1/workspaces/{workspace_id}/tasks`.

**Rationale:** Explicit path scoping makes tenancy visible in routing, logs, authorization, generated clients, and concurrent browser tabs.

**Alternatives considered:**

- **Custom workspace header:** Rejected because it makes critical context less visible.
- **Session-selected workspace:** Rejected because it is ambiguous across tabs and unsafe as an authorization input.

**Consequences:**

- Every scoped handler verifies that the authenticated global user has an active membership in the path workspace.
- A remembered active workspace is only a navigation preference.
- Nested resource lookups must verify workspace ownership rather than trusting resource IDs alone.

### D-027: Store attachments locally behind an abstraction

**Decision:** Store milestone-one attachment bytes under a configured local data directory and metadata in SQLite, behind a storage interface that can later support S3-compatible object storage.

**Rationale:** Local files preserve compact self-hosting without putting large blobs and their write load into SQLite.

**Alternatives considered:**

- **Require S3-compatible storage:** Deferred because it would add an external service to every initial installation.
- **SQLite blobs:** Rejected because they increase database write contention, backup size, and large-file memory pressure.

**Consequences:**

- Storage paths are generated, opaque, and workspace-scoped; user filenames never become trusted filesystem paths.
- Authorization is checked before upload and download.
- Database metadata and file operations require cleanup and reconciliation behavior for partial failure.
- Backups include both the SQLite database and attachment directory as one consistent data set.

### D-028: Use TanStack Query for frontend server state

**Decision:** Use TanStack Query to manage persistent server state, loading, caching, invalidation, and optimistic mutations in migrated frontend domains.

**Rationale:** It provides established server-state lifecycle behavior while allowing the generated client and Orbit domain hooks to remain explicit boundaries.

**Alternatives considered:**

- **Extend the custom external store into a server cache:** Rejected because it would recreate request lifecycle and invalidation behavior.
- **Component-local fetching:** Rejected because it would scatter caching, retry, loading, and rollback logic.

**Consequences:**

- Query keys include workspace context and stable domain identifiers.
- Generated transport calls are wrapped by feature-level queries and mutations.
- Optimistic updates define snapshots, rollback, and post-success reconciliation.
- The existing mock store remains responsible only for unmigrated domains.

### D-029: Expire, revoke, and replace workspace invitations

**Decision:** Workspace invitations expire seven days after creation. Owners and Admins may revoke a pending invitation. Sending a replacement invitation invalidates every previous pending token for the same email address and workspace.

**Rationale:** A seven-day lifetime gives recipients enough time to act without leaving enrollment credentials valid indefinitely. Replacement rather than parallel active tokens keeps the invitation state easy to explain and audit.

**Alternatives considered:**

- **Long-lived invitations:** Rejected because forgotten links would remain usable for too long.
- **Multiple active invitations for one recipient and workspace:** Rejected because revocation and audit behavior become ambiguous.

**Consequences:**

- Invitation tokens are cryptographically random, single-use, and stored only as non-reversible hashes.
- Expiry, revocation, replacement, and successful acceptance make a token unusable.
- Replacement happens atomically with invalidation so two tokens do not remain active after a resend.
- Owners and Admins can list pending invitations and see their status without seeing token values.
- Acceptance behavior follows D-030.

### D-030: Support SMTP and manually shared workspace invitations

**Decision:** Orbit supports both SMTP-delivered invitations and invitation links copied by an Owner or Admin. A new recipient creates a global account with the invited email address. An existing recipient must sign in to the global account whose normalized email matches the invitation before accepting it.

**Rationale:** Manual links keep workspace enrollment usable in installations without SMTP. Matching existing accounts by authenticated email prevents an invitation intended for one identity from being attached to another account.

**Alternatives considered:**

- **Require SMTP for invitations:** Rejected because SMTP is optional for self-hosted installations.
- **Allow any signed-in user holding the token to accept:** Rejected because forwarded or leaked links could grant membership to the wrong global identity.

**Consequences:**

- SMTP delivery proves control of the invited address and marks it verified when the invitation is accepted.
- A manually shared link grants the workspace membership but does not mark the global email address verified, because the link may have been forwarded through an unrelated channel.
- Manual and SMTP invitations use the same seven-day, single-use token lifecycle.
- A signed-in account with a different normalized email cannot accept the invitation.
- Acceptance creates the membership and consumes the invitation in one transaction.
- Accepting an invitation for an existing membership returns an idempotent success without creating a duplicate membership.
- Email normalization rules must be deterministic and must not apply provider-specific transformations such as removing dots or plus tags.

### D-031: Use a length-based password policy with Argon2id

**Decision:** Passwords must contain between 12 and 128 characters. Orbit allows spaces, Unicode, and paste; imposes no character-class rules or scheduled rotation; rejects known-common passwords; and hashes accepted passwords with Argon2id.

**Rationale:** Length and common-password screening improve resistance to guessing without encouraging predictable substitutions. Paste support and a generous maximum work well with password managers.

**Alternatives considered:**

- **Uppercase, number, and symbol requirements:** Rejected because they encourage predictable transformations and reject otherwise strong passphrases.
- **Scheduled password rotation:** Rejected because forced changes without evidence of compromise encourage weaker password choices.
- **Shorter minimum:** Rejected because password authentication is the only initial login factor.

**Consequences:**

- Length is measured consistently as user-perceived characters rather than UTF-8 bytes.
- Passwords are never silently truncated or normalized before hashing.
- Common-password screening uses a versioned local data set and does not send candidate passwords to an external service.
- Hash records include algorithm and parameters so a successful login can transparently rehash when the configured Argon2id policy increases.
- Password values must not appear in logs, validation telemetry, panic output, or generated API examples.
- Reset and initial-setup forms enforce the same policy as password changes.


### D-032: Throttle failed logins without permanent lockout

**Decision:** Apply progressive failed-login throttling to both the normalized email address and the effective client IP address. Return the same authentication error whether or not an account exists. Do not permanently lock accounts because of failed logins.

**Rationale:** Two independent limits slow targeted and broad password guessing. Avoiding permanent lockout prevents an attacker from indefinitely denying access to a known user.

**Alternatives considered:**

- **Permanent or administrator-cleared lockout:** Rejected because it creates an account-denial mechanism.
- **IP-only throttling:** Rejected because distributed attacks bypass it and shared networks can penalize unrelated users.
- **Account-only throttling:** Rejected because it lets an attacker repeatedly target one address and ignores broad scans.

**Consequences:**

- The email-specific backoff starts after the fifth consecutive failure. It begins at 5 seconds, doubles on each later failure, and caps at 15 minutes.
- The IP-specific bucket permits 50 failed attempts in a rolling 15-minute window, then returns `429 Too Many Requests` until the window allows another attempt.
- Throttled responses include `Retry-After`; handlers do not hold connections open merely to sleep.
- Successful authentication clears the email-specific failure state but does not clear the IP bucket.
- Failure state expires automatically and may remain in process memory, so a server restart resets it. This is acceptable under the single-instance deployment model.
- Client IP comes from the direct peer unless the request arrived through an explicitly configured trusted proxy. Forwarded headers from untrusted peers are ignored.
- Security logs record throttling events without recording passwords, session tokens, or full recovery tokens.


### D-033: Use host-only secure session cookies and origin checks

**Decision:** Browser sessions use a host-only cookie with the `__Host-` prefix, `Secure`, `HttpOnly`, and `SameSite=Lax`. Production requires HTTPS. Plain HTTP cookies are allowed only in an explicit development mode bound to loopback. State-changing browser requests must pass an origin check.

**Rationale:** These defaults keep session credentials out of frontend JavaScript, prevent domain-wide cookie injection, and provide layered CSRF protection while supporting ordinary same-site navigation.

**Alternatives considered:**

- **Bearer tokens in browser storage:** Rejected because frontend JavaScript and an XSS flaw could read long-lived credentials.
- **Cross-site session cookies by default:** Rejected because Orbit does not require cross-site embedding and the CSRF exposure is larger.
- **Secure-cookie exceptions inferred from hostnames:** Rejected because security behavior should follow an explicit environment mode.

**Consequences:**

- Production startup fails when configuration would serve authentication over known plain HTTP, except when a trusted reverse proxy supplies the original HTTPS scheme.
- Orbit trusts forwarded scheme and client-address headers only from explicitly configured proxy addresses.
- The cookie path is `/` and no `Domain` attribute is set, as required by the `__Host-` prefix.
- Unsafe HTTP methods require an allowed `Origin`; missing or mismatched origins fail before domain handlers run. Non-browser API authentication can be designed separately later.
- Sign-out and revocation expire the cookie and invalidate the server-side session record.
- Development mode must be visibly logged and cannot be enabled accidentally by a production default.


### D-034: Separate global suspension from workspace removal

**Decision:** Installation administrators may suspend a global account. A workspace Owner or Admin may remove a membership within that workspace, subject to Owner protections. These are separate operations with different scope.

**Rationale:** A global identity can belong to several workspaces. Workspace administrators should control their own membership without gaining authority over the user's account or access elsewhere.

**Alternatives considered:**

- **Let workspace administrators suspend global accounts:** Rejected because it grants one workspace authority over unrelated workspaces.
- **Treat membership removal as account deletion:** Rejected because it breaks the global identity model.

**Consequences:**

- Global suspension immediately invalidates all sessions and blocks authentication and invitation acceptance across the installation.
- Reinstating a global account restores login eligibility but does not recreate memberships removed separately.
- Membership removal immediately revokes access to that workspace but leaves other memberships and sessions valid. Active sessions fail authorization on their next workspace-scoped request.
- Admins cannot remove or change the Owner. The Owner must transfer ownership before leaving a workspace.
- Neither suspension nor membership removal deletes tasks, comments, attachments, audit records, or other authored content.
- Security audit records identify the actor, target, scope, timestamp, and action without copying sensitive credentials.


### D-035: Retain deleted major records for 30 days

**Decision:** Soft-deleted workspaces, projects, and tasks remain recoverable for 30 days in a simple trash view. After the retention period, a durable background job purges them and any files that are no longer referenced. Early permanent deletion is deferred.

**Rationale:** Thirty days provides a useful recovery window without retaining deleted application data indefinitely. A focused trash view makes soft deletion usable rather than leaving recovery as a database-only operation.

**Alternatives considered:**

- **No user-facing restoration:** Rejected because soft deletion would not protect users from mistakes without an operational intervention.
- **Keep deleted records indefinitely:** Rejected because storage and privacy obligations would grow without a retention boundary.
- **Allow immediate permanent deletion:** Deferred because it adds a high-risk destructive path to the first milestone.

**Consequences:**

- Members may restore projects and tasks because Members may also delete them.
- Only the workspace Owner may restore a deleted workspace.
- Deleting a parent hides its descendants without rewriting every child as individually deleted.
- Restoring a parent reveals descendants that remain within retention and were not separately deleted before the parent deletion.
- Purge order preserves referential integrity and queues file removal only after the corresponding metadata can no longer be restored.
- Restore detects uniqueness conflicts. It returns a conflict response rather than silently renaming restored records.
- Trash queries remain workspace-scoped and require the same authorization as the corresponding deletion action.


### D-036: Run guarded forward migrations at startup

**Decision:** Embed ordered forward migrations in the Orbit binary and apply pending migrations automatically before the server accepts traffic. Provide `orbit migrate status` and `orbit migrate run` for inspection and manual execution. Never run down migrations automatically.

**Rationale:** Automatic forward migration keeps appliance-style upgrades simple. Startup locking, backups, and strict failure behavior prevent the convenience from hiding schema errors.

**Alternatives considered:**

- **Require a separate migration command for every upgrade:** Rejected because it adds an avoidable operational step to the single-instance deployment.
- **Automatically roll back failed upgrades with down migrations:** Rejected because down migrations can destroy data and cannot reliably reverse application behavior.

**Consequences:**

- Startup acquires an exclusive application migration lock before inspecting or changing the schema.
- Orbit creates and verifies a pre-migration backup before any migration that rebuilds a table, removes data, or otherwise declares itself destructive.
- The server does not bind its public listener until migrations and post-migration checks succeed.
- A database schema newer than the binary causes startup to fail with a clear version error.
- A failed migration leaves the service unavailable and preserves the backup location in operator-facing output.
- Migration records include version, checksum, applied timestamp, and application version. A checksum mismatch fails startup.
- Migrations are transactional where SQLite permits it and contain explicit recovery instructions when they cannot be fully transactional.


### D-037: Keep verified daily and weekly backups

**Decision:** Orbit creates an automatic backup every 24 hours and retains 7 daily and 4 weekly backups. Each backup contains the SQLite database, attachments, and a manifest with checksums and schema and application versions.

**Rationale:** Built-in backups match the self-hosted single-instance model and cover both parts of Orbit's durable state. Daily and weekly retention provides recent restore points without unbounded local growth.

**Alternatives considered:**

- **Database-only backups:** Rejected because restored attachment metadata could point to missing or mismatched files.
- **Operator-managed backups only:** Rejected because safe defaults should not depend on every operator building automation before using Orbit.
- **Built-in off-site upload:** Deferred because destinations, credentials, encryption, and provider policy need a separate design.

**Consequences:**

- Orbit briefly blocks attachment mutations while it captures a consistent database and file snapshot. Reads and unrelated writes may continue when SQLite's online backup mechanism permits them.
- The configured backup directory must sit outside the live data directory so snapshots do not recursively include themselves.
- Orbit provides `orbit backup create`, `orbit backup list`, `orbit backup verify`, and `orbit backup restore` commands.
- Restore refuses to modify a database used by a running server and verifies checksums and supported schema versions first.
- Orbit verifies a new backup before counting it toward retention or deleting an older verified backup.
- Pre-migration backups use the same format but have a distinct reason and retention class.
- Operators remain responsible for copying backups off the host and protecting access to them.


### D-038: Fail closed when SQLite integrity checks fail

**Decision:** Run SQLite `quick_check` at every startup before serving traffic. Run full `integrity_check` and foreign-key verification weekly through the durable scheduler and after migrations where applicable. A failed check stops normal web traffic and write operations. Orbit never attempts automatic database repair.

**Rationale:** Early detection limits further writes to a damaged database. Automatic repair could destroy recoverable evidence or make corruption worse, so recovery remains an explicit operator action against verified backups.

**Alternatives considered:**

- **Check only when an error occurs:** Rejected because latent corruption may remain unnoticed while backups continue rotating.
- **Continue serving after a failed check:** Rejected because new writes could compound damage and users could observe inconsistent state.
- **Automatic repair:** Rejected because SQLite repair choices require operator review and a known-good backup.

**Consequences:**

- Startup does not bind the public listener when `quick_check` or required post-migration verification fails.
- A failed scheduled check marks health as failed and drains or stops normal HTTP handling before further application writes.
- Recovery remains available through local CLI commands, including backup listing, verification, and restore. The web application does not offer degraded read-only access.
- Operator-facing errors identify the database path and newest verified backup without exposing application records.
- Integrity results and failure transitions are logged and included in health diagnostics.
- Backup retention must never treat a corrupt snapshot as verified.


### D-039: Use bounded retries and retained dead jobs

**Decision:** Jobs use at-least-once execution and receive 8 attempts by default. Retry delays are 10 seconds, 30 seconds, 2 minutes, 10 minutes, 1 hour, 6 hours, and 24 hours, with bounded random jitter. A job type may override its attempt count and schedule. Non-retryable validation or configuration failures move directly to dead state. Dead jobs remain for 30 days and are never retried automatically without a new explicit action.

**Rationale:** A bounded schedule handles short outages and day-long provider failures without creating infinite work. Dead-job retention gives operators enough time to diagnose and retry failures while keeping the queue finite.

**Alternatives considered:**

- **Retry forever:** Rejected because invalid work and retired integrations would consume resources indefinitely.
- **One global fixed interval:** Rejected because immediate transient failures and long outages need different spacing.
- **Discard jobs after their final attempt:** Rejected because operators would lose failure context and controlled recovery.

**Consequences:**

- Job handlers classify errors as retryable or permanent and must remain safe under duplicate execution.
- Jitter changes each delay by at most 20 percent and never schedules before the unjittered delay's lower bound used by tests.
- Owners and Admins may inspect and retry jobs scoped to their workspace. Installation administrators may inspect and retry global jobs.
- Job views and logs redact secrets and sensitive payload fields.
- Manual retry creates a new queued execution linked to the original dead job. The original attempts and error history remain unchanged.
- Expired dead jobs are purged by a maintenance job unless an explicit incident hold protects them.


### D-040: Persist recurring schedules and materialize normal jobs

**Decision:** Persist recurring schedules in SQLite. Support fixed intervals and five-field cron expressions evaluated in UTC. Each due occurrence creates a normal durable job. After downtime, enqueue only the latest missed occurrence unless that schedule explicitly opts into full catch-up.

**Rationale:** Persisted schedules survive restarts, while materializing ordinary jobs keeps retries, leases, observability, and dead-letter handling on one execution path. Coalescing missed runs prevents a long outage from flooding the queue.

**Alternatives considered:**

- **In-memory timers only:** Rejected because restart timing and missed work would be lost.
- **Replay every missed occurrence:** Rejected as the default because maintenance and notification schedules could create an unbounded backlog.
- **Local-time cron expressions:** Rejected because daylight-saving transitions make execution ambiguous.

**Consequences:**

- A unique schedule identifier and scheduled timestamp prevent duplicate materialization.
- Fixed intervals use stored schedule timestamps rather than process uptime.
- Full catch-up is an explicit per-schedule option for work where every occurrence matters.
- Repeated permanent schedule-configuration failures disable the schedule and notify the appropriate administrator.
- Ordinary handler failures affect the materialized job and its retries, not the schedule definition.
- Schedule edits and enable or disable actions are audited.


### D-041: Bound worker concurrency and drain on shutdown

**Decision:** Jobs use low, normal, high, or critical priority and FIFO order within the same priority and availability time. Worker concurrency defaults to 4 and is configurable per installation. A job type may set a lower concurrency limit. One execution slot is reserved for critical work. During shutdown, the worker stops claiming immediately, gives active jobs 30 seconds to finish, then cancels local tasks and leaves their leases to expire.

**Rationale:** A small default protects SQLite and self-hosted machines while still allowing independent network work to overlap. Reserved critical capacity prevents bulk maintenance work from blocking urgent jobs. Lease recovery is safer than recording interrupted work as an ordinary failure.

**Alternatives considered:**

- **Unlimited asynchronous jobs:** Rejected because it could exhaust connections, memory, file handles, or provider quotas.
- **Strict single-job execution:** Rejected because unrelated network-bound work would block unnecessarily.
- **Wait forever during shutdown:** Rejected because deployment and recovery could hang on a stuck handler.

**Consequences:**

- Priority affects claim order but does not interrupt a running lower-priority job.
- Each job type inherits the global concurrency unless it declares a smaller positive limit.
- The worker passes a cancellation signal to handlers. Handlers should stop at safe boundaries and must not convert shutdown cancellation into a final job failure.
- After the drain deadline, incomplete jobs remain leased until the recovery rule makes them claimable again.
- Configuration validation rejects zero concurrency and reserves critical capacity without exceeding the configured global limit.
- Metrics expose queued and active jobs by priority and type, plus shutdown drain outcomes.


### D-042: Protect job leases with heartbeats and claim tokens

**Decision:** A claimed job receives a five-minute lease and a random claim token. While the handler runs, the worker heartbeats every 30 seconds and extends expiry to five minutes from the heartbeat. A job type may request a longer lease but cannot disable leasing. Expired jobs re-enter the normal attempt and retry flow.

**Rationale:** Heartbeats support long-running work without making crash recovery wait for a worst-case static timeout. A claim token prevents a stalled worker from completing or extending a job after another claim has taken ownership.

**Alternatives considered:**

- **One fixed lease with no heartbeat:** Rejected because handlers would need either short timeouts that duplicate valid work or long timeouts that delay recovery.
- **Process ownership without durable leases:** Rejected because crashes would leave running jobs stranded.
- **Unlimited leases:** Rejected because abandoned jobs would never recover.

**Consequences:**

- All lease deadlines use the same database-derived UTC time convention.
- Heartbeat, completion, retry, and failure updates include the current claim token and affect the row only when it still matches.
- A stale claimant that loses ownership must discard its result and stop further side effects where possible.
- Lease expiry consumes the current attempt and records an interruption before retry policy is applied.
- Job types with longer leases retain the 30-second heartbeat unless they explicitly choose a shorter safe interval.
- Metrics distinguish handler failures, lease expirations, stale claimant updates, and shutdown cancellations.


### D-043: Return RFC 9457 Problem Details errors

**Decision:** API failures use RFC 9457 Problem Details with content type `application/problem+json`. Every response includes `type`, `title`, `status`, a stable machine-readable `code`, a safe `detail`, `instance`, and `request_id`. Validation failures may add an `errors` object keyed by field path. Conflict failures add the current version and safe refresh metadata.

**Rationale:** A standard envelope gives generated clients and frontend forms one error path while separating stable program logic from user-facing text and private diagnostic detail.

**Alternatives considered:**

- **Endpoint-specific error shapes:** Rejected because clients would need custom parsing for every feature.
- **Expose internal error strings:** Rejected because implementation and sensitive details could leak and messages would become accidental API contracts.

**Consequences:**

- Clients branch on HTTP status and stable `code`, never on `title` or `detail` text.
- The `errors` object appears only when field-level validation information is useful. Each value is an ordered array of safe messages.
- Authentication and recovery errors do not reveal whether an email address exists.
- Unhandled failures return a generic problem and request ID. Structured logs store the underlying error under that request ID.
- Problem `type` identifiers remain stable documentation URLs even if the public documentation is initially served by Orbit itself.
- OpenAPI documents each declared problem code and shared problem schema.


### D-044: Use strict request contracts and layered validation

**Decision:** Mutation DTOs reject unknown JSON fields and duplicate keys. Orbit enforces body-size and nesting-depth limits before full deserialization. HTTP-boundary validation checks shape and syntax; application and domain code enforce stateful business rules. Safe field failures are returned together using JSON field paths.

**Rationale:** Strict contracts catch client drift instead of silently discarding input. Separating structural validation from business rules keeps transport concerns out of the domain while still producing useful form feedback.

**Alternatives considered:**

- **Ignore unknown fields:** Rejected because misspelled or outdated fields could appear to succeed while losing user intent.
- **Put all validation in handlers:** Rejected because non-HTTP callers and background jobs would bypass business rules.
- **Return only the first field error:** Rejected because it creates repetitive form submission cycles.

**Consequences:**

- OpenAPI schemas and runtime DTO validation describe the same required, optional, nullable, and bounded fields.
- Omitted, `null`, and empty values remain distinct wherever the schema permits them.
- Orbit trims surrounding whitespace only for fields whose domain meaning requires it, such as names and email addresses.
- Orbit never normalizes passwords, message bodies, descriptions, or original filenames.
- Validation paths use JSON notation such as `assignees[2].user_id`.
- Domain failures still use Problem Details and stable codes but are not forced into field errors when no single input field caused them.


### D-045: Use opaque cursor pagination

**Decision:** Paginated lists use opaque cursors, a default page size of 50, and a maximum of 100. Stable sorting ends with the UUIDv7 identifier as a tie-breaker. Responses contain `items` and `next_cursor`; the final page returns a null cursor. Total counts are omitted unless a product view needs one.

**Rationale:** Cursor pagination remains stable while records are inserted or removed and avoids the growing scan cost of deep offsets. Omitting automatic counts keeps ordinary list requests focused on the rows they display.

**Alternatives considered:**

- **Offset and page-number pagination:** Rejected as the platform default because concurrent writes can shift records between pages and deep offsets become expensive.
- **Return a total with every list:** Rejected because exact counts add work and may not affect the UI.

**Consequences:**

- The cursor encodes the effective filters, sort, last sort values, and tie-breaker in a server-owned format.
- Reusing a cursor with different filters or sorting returns the stable `invalid_cursor` problem code.
- Clients treat cursors as opaque and never construct or modify them.
- Endpoints expose explicit count queries only when an accepted UI requirement displays the result.
- OpenAPI uses one shared pagination pattern while each endpoint retains its typed item schema.
- Cursor format may include a version so the server can reject obsolete encodings cleanly.


### D-046: Version the API while shipping frontend and backend together

**Decision:** HTTP endpoints live under `/api/v1`. Before Orbit's first stable release, backend, OpenAPI contract, generated client, and bundled frontend may make breaking v1 changes only in the same commit and release. After the first stable release, breaking changes require a new API version or a documented migration window. Orbit does not maintain multiple active versions before a real external client needs them.

**Rationale:** Lockstep releases keep early internal development fast without pretending the API is already stable. A path version and explicit breaking-change rules leave a clear boundary when external clients or independent upgrades appear.

**Alternatives considered:**

- **Promise v1 stability immediately:** Rejected because the first real vertical slice will expose design mistakes that should be corrected before a stable release.
- **Unversioned endpoints:** Rejected because later compatibility and diagnostics would lack an explicit contract boundary.
- **Maintain every historical version:** Rejected because no external client currently requires that cost.

**Consequences:**

- Additive response fields and new endpoints are compatible within v1. Field removal, renaming, meaning changes, and stricter accepted input are breaking.
- CI regenerates the client and fails when committed generated output or the frontend no longer matches the OpenAPI contract.
- The generated client sends its expected contract identifier; incompatible combinations fail with a clear version problem rather than unpredictable parsing errors.
- Release notes call out contract changes even while pre-stable lockstep breaking changes are allowed.
- API stability is reassessed before declaring the first stable Orbit release.


### D-047: Stream bounded uploads and restrict inline rendering

**Decision:** Attachments default to a maximum of 25 MiB per file and 100 MiB per request. Installation configuration may lower or raise both values. Orbit streams uploads to temporary files, allows arbitrary file types, detects content type from bytes, renders only an explicit safe image allowlist inline, and serves every other type as a download.

**Rationale:** Streaming bounds memory use. Treating supplied names and types as untrusted prevents active content and misleading metadata from becoming executable browser responses while preserving general-purpose attachments.

**Alternatives considered:**

- **Buffer complete uploads in memory:** Rejected because concurrent uploads could exhaust process memory.
- **Trust browser MIME declarations:** Rejected because clients control them.
- **Serve HTML and other active formats inline:** Rejected because same-origin active content can become an account-compromise path.

**Consequences:**

- Limit enforcement occurs while streaming. Rejected and interrupted uploads remove their partial temporary files.
- Content detection determines security behavior; the browser-provided MIME type remains optional metadata only.
- The initial inline allowlist contains JPEG, PNG, GIF, and WebP. SVG is downloaded rather than rendered inline because it can contain active content.
- Downloads use `Content-Disposition: attachment`, `X-Content-Type-Options: nosniff`, and a sanitized header filename. Metadata retains the original display name subject to length and control-character validation.
- Temporary and final storage stay on the same filesystem where possible so finalization can use an atomic rename.
- Configuration validation rejects values that exceed platform-safe integer and disk-handling bounds.


### D-048: Deduplicate attachment blobs within each workspace

**Decision:** Compute SHA-256 while streaming each upload and deduplicate matching bytes only within the same workspace. Each attachment keeps its own metadata row, while matching rows reference one workspace-scoped blob. Cleanup determines liveness by querying references rather than trusting a mutable reference counter.

**Rationale:** Workspace-local deduplication saves disk space without creating cross-tenant existence signals or coupling deletion correctness to a counter that can drift from actual metadata.

**Alternatives considered:**

- **No deduplication:** Rejected because repeated task and comment attachments would store identical bytes unnecessarily.
- **Installation-wide deduplication:** Rejected because timing and storage behavior could reveal that another workspace holds matching content.
- **Delete from a stored reference count:** Rejected because failed or reordered mutations can make counters incorrect.

**Consequences:**

- Blob identity includes `workspace_id`, SHA-256 digest, and byte size. A digest match with a different size is never treated as the same object.
- The digest is an integrity and deduplication key, not an authorization credential.
- Downloads authorize the attachment metadata and its owning resource before resolving the blob.
- A cleanup job removes a blob only after a current database query finds no live or retained attachment references.
- Newly finalized but unreferenced blobs remain quarantined for 24 hours before cleanup so interrupted transactions can be diagnosed or recovered.
- Backup manifests include blob checksums and verification checks referenced content against them.


### D-049: Finalize uploads through staged files and pending metadata

**Decision:** Stream an upload to a random temporary file while hashing and validating it, then create a pending upload record tied to the authenticated user and workspace. Atomically rename or deduplicate the bytes into the final blob path. In one database transaction, create the attachment reference and mark the upload complete. Return success only after both final bytes and metadata are available.

**Rationale:** SQLite and the filesystem cannot share one atomic transaction. Explicit pending state and reconciliation make each partial-failure case recoverable without claiming success too early.

**Alternatives considered:**

- **Write final bytes after committing attachment metadata:** Rejected because successful metadata could point to a file that never finalized.
- **Delete every unreferenced blob immediately after database failure:** Rejected because concurrent deduplication and crash recovery need a safe quarantine window.
- **Permanent public URLs:** Rejected because task attachments require current workspace and resource authorization.

**Consequences:**

- A database failure after file finalization leaves an unreferenced blob for the 24-hour quarantine and cleanup process.
- A file-finalization failure rolls back pending metadata and returns a retryable storage problem.
- Startup and scheduled reconciliation remove stale temporary files and expire incomplete pending uploads.
- Downloads require current access to the owning task or comment. Possession of an attachment or blob identifier grants no access.
- Download responses resolve attachment metadata first and never expose internal filesystem paths or blob keys.
- Milestone one has no permanent public or bearer-token attachment URLs.


### D-050: Use authenticated WebSockets for realtime delivery

**Decision:** Each browser tab opens one authenticated WebSocket connection. HTTP remains the mutation transport. The socket carries server events, subscription control, presence, and typing. A connection may subscribe only to workspaces in which its session has an active membership.

**Rationale:** WebSockets support future chat and bidirectional ephemeral signals without moving ordinary validated mutations away from the HTTP and OpenAPI contract.

**Alternatives considered:**

- **Server-sent events:** Deferred because future chat presence and subscription control benefit from a bidirectional connection.
- **Perform all mutations over WebSockets:** Rejected because it would duplicate request validation, error, observability, and generated-client behavior already defined for HTTP.
- **One installation-wide event stream:** Rejected because workspace subscriptions and authorization should remain explicit.

**Consequences:**

- The WebSocket handshake uses the existing secure session cookie and validates `Origin` before upgrade.
- Subscription and resubscription requests verify current workspace membership.
- Membership removal or global suspension closes affected subscriptions immediately rather than waiting for the next client request.
- Ping and pong heartbeats detect dead connections and release their in-memory presence state.
- Socket messages use versioned envelopes even while event payloads remain domain-specific.
- SSE can be added later for a client that cannot use WebSockets; it is not part of the initial platform.


### D-051: Replay ordered workspace events from a transactional outbox

**Decision:** Domain mutations write their durable realtime event to an outbox in the same SQLite transaction. Each workspace has a monotonically increasing event sequence; installation-global events use a separate sequence and authorization path. Retain events for seven days. Reconnecting clients send their last applied sequence and receive ordered replay or `resync_required` when replay is impossible.

**Rationale:** A transactional outbox prevents committed data from losing its notification. Per-workspace ordering makes reconnect behavior deterministic without creating one cross-tenant stream.

**Alternatives considered:**

- **Publish only from process memory after commit:** Rejected because a crash between commit and publish would lose the event.
- **One global sequence for every workspace:** Rejected because it couples tenant traffic and leaks ordering gaps.
- **Retain every event forever:** Rejected because realtime recovery is not an audit-log replacement.

**Consequences:**

- Delivery is at least once. Clients deduplicate by stable event ID and apply sequences monotonically.
- Events identify changed resources and may carry safe summary data; HTTP remains authoritative for complete records.
- A missing, invalid, or older-than-retention cursor receives `resync_required` rather than a partial replay.
- On resync, the frontend invalidates affected workspace query keys and refetches through HTTP before accepting later sequences.
- Outbox pruning never removes events inside the seven-day retention window.
- Authorization is rechecked before replay, so old cursor possession does not grant workspace access.


### D-052: Keep presence and typing state ephemeral

**Decision:** Presence and typing indicators live only in process memory and are never written to SQLite or replayed. Presence expires after 60 seconds without a heartbeat; clients refresh it every 20 seconds. Typing expires after 5 seconds and a client sends at most one typing refresh every 2 seconds per conversation.

**Rationale:** These signals describe current connection activity and have no recovery value. TTLs produce correct eventual behavior after abrupt disconnects without durable writes on every keystroke or heartbeat.

**Alternatives considered:**

- **Persist ephemeral state:** Rejected because it would add frequent SQLite writes and restore stale information after restart.
- **Replay typing and presence events:** Rejected because historical ephemeral signals are meaningless.

**Consequences:**

- A clean disconnect clears state owned by that connection immediately. A crash leaves state only until its TTL expires.
- Broadcasts go only to subscribers currently authorized for the relevant workspace and conversation.
- Presence remains workspace-scoped even when one global user belongs to several workspaces.
- The server rate-limits client typing messages and may drop them without affecting durable chat behavior.
- Clients treat missing or delayed ephemeral events as normal and never derive authorization or durable unread state from them.
- Application restart begins with no users present or typing until connected clients refresh their state.


### D-053: Send outbound email only through configured providers

**Decision:** Orbit never delivers outbound email directly to recipient mail servers. Every outbound message uses a configured third-party transport. SMTP is the first supported outbound transport and sits behind an internal mail transport interface.

**Rationale:** Direct internet mail delivery requires reputation management, bounce processing, retry policy, and deliverability operations that do not belong in a compact self-hosted application. A provider interface leaves room for later HTTP email providers without changing domain jobs.

**Alternatives considered:**

- **Direct SMTP delivery to recipient MX servers:** Rejected because each Orbit installation would need to operate as a reputable sending mail server.
- **SMTP-specific calls throughout features:** Rejected because invitations, recovery, and notifications should not depend on one transport's API.

**Consequences:**

- Transactional messages are versioned templates queued through the durable job system.
- Missing or invalid outbound configuration produces a permanent configuration failure and dead job rather than silently dropping mail.
- Approved administrator-copyable setup, invitation, and recovery links remain available where their decisions permit them.
- Transport credentials remain installation secrets and never enter workspace-visible job payloads or logs.
- Later providers implement the same narrow send contract and define their own retryable error mapping.

### D-054: Include an inbound SMTP receiver in Orbit

**Decision:** A later mail milestone will add an SMTP receiver to the Orbit binary so an installation can accept inbound messages locally. This receiver is separate from the outbound provider interface and is not part of the foundation-and-tasks milestone.

**Rationale:** Local receipt keeps inbound mailbox data under the operator's control and avoids requiring a mailbox provider for Orbit's future mail feature.

**Alternatives considered:**

- **External receiver forwarding to a webhook or pipe:** Rejected as the primary architecture because the chosen product direction is an integrated receiver.
- **Treat inbound and outbound mail as one transport:** Rejected because receiving and provider-based sending have different protocols, security boundaries, and operational failure modes.

**Consequences:**

- The receiver needs its own listener configuration, recipient routing, message-size limits, TLS policy, queueing, parsing, abuse controls, and observability design before implementation.
- SMTP acknowledgement occurs only after Orbit has durably accepted the raw message or can safely retry processing it.
- Message parsing and mailbox projection happen asynchronously after durable receipt.
- Outbound mail always continues through a third-party provider, even when the inbound receiver is enabled.
- Whether the receiver accepts public internet delivery or only trusted relays remains undecided.


### D-055: Support trusted-relay and public-MX inbound modes

**Decision:** The embedded SMTP receiver supports both trusted-relay mode and direct public-MX mode. Trusted-relay mode is the default. Public-MX mode requires explicit operator opt-in and successful readiness checks.

**Rationale:** Trusted relays provide a safer default for ordinary installations, while public-MX mode lets operators receive mail without another mail ingress service when they accept the operational burden.

**Alternatives considered:**

- **Trusted relays only:** Rejected because local direct receipt is an explicit product goal.
- **Public MX only:** Rejected because it would force every installation to expose and operate an internet-facing mail service.

**Consequences:**

- Trusted-relay mode accepts mail only from configured source networks and requires authenticated SMTP or another explicitly configured trust mechanism.
- Public-MX mode applies recipient validation, connection and message limits, TLS policy, anti-abuse controls, durable spooling, and clear rejection codes before accepting internet traffic.
- Public-MX readiness checks cover listener reachability, configured recipient domains, hostname, TLS material when required, storage writability, and queue health. DNS guidance is reported but cannot be proven completely from inside every deployment.
- Neither mode becomes an open relay. The inbound listener never relays arbitrary recipient mail or sends outbound messages directly.
- Listener ports and bind addresses are independent from the HTTP server. Privileged port and container mapping remain operator configuration.
- The future mail milestone must define spam filtering, sender authentication signals, recipient routing, raw-message retention, bounce handling, and mailbox projection before implementation.


### D-056: Route inbound domains to workspace mailboxes

**Decision:** Installation administrators assign each inbound domain to exactly one workspace. Workspace Owners and Admins create personal or shared mailboxes and aliases under assigned domains. Personal mailboxes belong to a workspace membership; shared mailboxes grant access to selected memberships. Unknown recipients are rejected during SMTP `RCPT TO`. Catch-all routing is deferred.

**Rationale:** Domain ownership provides an unambiguous tenant boundary before Orbit accepts message data. Membership-scoped mailboxes preserve workspace isolation even when one global user belongs to several workspaces.

**Alternatives considered:**

- **Attach personal mailboxes directly to global users:** Rejected because the same identity can have unrelated addresses and permissions in different workspaces.
- **Accept unknown recipients and bounce later:** Rejected because backscatter is abusive and wastes storage and processing.
- **Enable catch-all addresses immediately:** Deferred until spam and routing behavior has production evidence.

**Consequences:**

- Domain matching is case-insensitive. Orbit preserves the displayed local part but routes it case-insensitively within a domain.
- Database constraints prevent a domain from being active in two workspaces in the same installation.
- Removing a membership suspends its personal mailbox access without deleting retained messages or changing other recipients' shared-mailbox access.
- SMTP recipient validation performs a workspace-scoped mailbox or alias lookup before accepting message content.
- Aliases resolve to one mailbox initially; distribution lists and forwarding rules require later design.
- Detailed spam filtering, raw-message retention, mailbox synchronization, threading, and sending UI behavior belong to a separate mail feature specification.


### D-057: Start with three Rust crates and strict dependency direction

**Decision:** The Cargo workspace begins with `apps/server` for the binary, CLI, startup, adapters, and dependency wiring; `crates/platform` for reusable infrastructure modules; and `crates/orbit` for Orbit domain types, application services, and ports. Platform modules become separate crates only when a second internal application needs them or measured dependency isolation provides a concrete benefit.

**Rationale:** Three crates establish the important application and infrastructure boundaries without turning every capability into a package before reuse exists.

**Alternatives considered:**

- **One server crate:** Rejected because Orbit domain code and reusable platform code would be difficult to separate later.
- **One crate per platform capability immediately:** Rejected because it adds manifests, feature coordination, and public interfaces before those boundaries are proven.

**Consequences:**

- `platform` never depends on `orbit`.
- `orbit` may depend on small platform value types and interfaces but does not depend on Axum or SQLx.
- `server` implements and wires Axum handlers, SQLx repositories, storage adapters, mail transports, jobs, and runtime configuration.
- Feature modules do not query another feature's tables directly. They call its application service or a declared port.
- Platform capabilities begin as focused modules with private implementation details inside `crates/platform`.
- New generic traits require at least two real implementations or call sites that demonstrate the variation. A likely future implementation alone is not enough.
- Tests enforce dependency direction and keep domain tests runnable without HTTP or a production database file.


### D-058: Use TOML configuration with deployment overrides

**Decision:** TOML is the primary configuration source. Environment variables named `ORBIT__SECTION__KEY` override TOML, and CLI flags override environment variables. Secrets may come from environment variables or paired `_FILE` variables. Setting both forms for one secret is an error. Orbit loads `.env` files only in explicit development mode.

**Rationale:** TOML gives operators one readable configuration file. Environment and file-based overrides support containers and secret mounts without forcing credentials into that file.

**Alternatives considered:**

- **TOML only:** Rejected because container deployments need practical secret and one-off setting injection.
- **Environment variables only:** Rejected because a large application configuration becomes difficult to inspect and maintain.
- **Store infrastructure configuration in SQLite:** Rejected initially because startup, database, listener, and recovery settings must exist before the application database is usable.

**Consequences:**

- Orbit resolves the complete precedence chain once at startup and validates it before migrations or network listeners begin.
- Orbit never writes secrets back into TOML or emits them through logs, diagnostics, panic output, or CLI commands.
- `orbit config check` validates the effective configuration. `orbit config show` displays sources and effective non-secret values while always redacting secrets.
- `_FILE` inputs must be regular readable files with bounded size; Orbit trims one trailing line ending and otherwise preserves secret bytes.
- Unknown TOML keys and malformed environment overrides fail validation rather than being ignored.
- Runtime infrastructure settings require restart. Workspace product settings may still live in SQLite where appropriate.

## Current architectural direction, not yet accepted

The following ideas have been discussed but are not decisions:

- A single artifact that may embed the built frontend.

These choices require explicit evaluation before implementation.

## Remaining decision queue

The accepted decisions above settle the first milestone, tenancy, authentication and initial authorization, core data conventions, SQLite operations, durable jobs, API contracts, persistence style, attachment handling, realtime behavior, mail boundaries, and frontend server-state library. The remaining decisions will be resolved in this order:

1. Platform and Orbit module boundaries
2. Configuration, secrets, deployment, and observability
3. Remaining security controls and audit records
4. Testing and local developer experience
5. Detailed incremental frontend migration

## Next open decision: Code and module boundaries

The design must define the initial Cargo workspace, dependency direction, and the threshold for extracting platform modules into separate reusable crates.
