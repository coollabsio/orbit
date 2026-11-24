# Split orbit-server into crates (design)

Status: proposal, not implemented. Made with an architect arena of 4 candidate designs (layered, vertical by
feature, minimal cut, store first) and one cross-judge. The work files are in `/tmp/arena-orbit-split/`.

## Problem

`orbit-server` (`apps/server`) is one crate with 213,738 lines in 341 files. Measured on 2026-11-24 (rustc 1.97,
sccache on):

| Build | Time | Where the time goes |
|---|---|---|
| Edit of one file (dev) | ~16 s | 11.7 s rustc lib (macro expansion 3.2 s, metadata 3.1 s) + ~4 s bin and link |
| Full debug lib | 113 s | metadata 81 s, monomorphization 76 s, codegen 32 s, typeck 27 s (one rustc process, mostly serial) |
| Release in Docker (amd64) | 16.5 min of a 27 min job | one crate, `codegen-units` is the only parallelism |

Most-edited files since 2026-09-01: `openapi.rs` 78 commits, `repositories/tasks.rs` 54, `app.rs` 54,
`workspace_routes.rs` 50, `cli.rs` 48, `repositories/workspaces.rs` 43, `auth_routes.rs` 43, `task_routes.rs` 39.
About 52% of edits touch `repositories`.

What blocks a split:

1. A cycle of 36 modules (179k lines). `repositories` (78k lines, used by 58 modules) calls up into `audit`,
   `collab`, `billing`, `live`, `secret_box`, `export`, `workspace_export`, the importers and others.
2. The shared HTTP helpers live inside route modules: `task_routes` (`ApiError`, `ApiJson`, `ApiQuery`,
   `authenticate_session`, …) and `auth_routes` (`CookieMode`, `request_session`). There are 4 copies of `ApiError`.
3. `openapi.rs` names all 701 handlers as `crate::module::handler`.
4. `App::build` and `initialize_production_services` name every state, job handler and schedule.

## Usage after the split

```rust
// A route crate (for example orbit-routes-work) adds a handler.
use orbit_http::{ApiError, ApiJson, authenticate_session};
use orbit_store::tasks::TaskRepository;

#[utoipa::path(post, path = "/api/v1/tasks/{id}/pin", tag = "tasks")]
pub async fn pin_task(State(s): State<TaskState>, headers: HeaderMap, Path(id): Path<String>)
    -> Result<Json<TaskBody>, ApiError> { todo!() }

// orbit-openapi (sink crate, not on the server binary's graph) lists it:
//     paths(orbit_routes_work::tasks::pin_task, …)
// A test in orbit-openapi fails when a #[utoipa::path] in any route crate is not listed.

// A repository method that changes a document asks for revalidation without knowing the collab hub:
orbit_store::revalidate::workspace(&database, workspace_id); // live revalidate + cluster send; hub listens
```

Rule for agents: shared HTTP types come only from `orbit_http`; repositories only from `orbit_store`; there are no
re-exports from `orbit_server`.

## Shape: crates at the end of phase 2 (about 12)

| Crate | Content (from today's modules) | ~Lines | Depends on |
|---|---|---|---|
| `orbit-platform`, `orbit-mail`, `orbit-domain` | as today; `orbit-domain` also takes pure types (billing access types, `NewEvent`, `Recipients`, `Provider`, `Owner`) | 25k | — |
| `orbit-doc` | `collab::{blocknote, markdown_import, markdown_merge, sanitize}`, `export::{markdown, names}` | 7k | domain |
| `orbit-formula` | `repositories/databases/formula` | 3.3k | domain |
| `orbit-web-assets` | `static_assets.rs`, `include_dir!` of `apps/web/dist`, `CONTRACT_ID`; own `build.rs` | 0.2k | platform |
| `orbit-store` | `repositories` (minus mail), `audit`, `secret_box`, live hub, collab hub, cluster transport, job-kind consts, billing `enqueue_*_in` | ~66k | platform, domain, doc, formula |
| `orbit-store-mail` | `repositories/mail` | ~18k | store, mail |
| `orbit-http` | one `ApiError`/`ProblemBody`/`ApiJson`/`ApiQuery`, `CookieMode`, `request_session`, `authenticate_session`, `socket_ticket`, upload helpers, billing guard | ~5k | store |
| `orbit-routes-*` (work, docs, mail, account/admin, integrations) | route modules, their services, MCP and public-API parts, their `register_jobs` and `SCHEDULES` | 10–30k each | http, store(-mail) |
| `orbit-openapi` | `openapi.rs` (the `paths(...)` list and the `ProblemDetails` modifier) | 3.3k | route crates |
| `orbit-server` (`apps/server`) | `App`, `router.rs`, `cli.rs`, `main.rs`; stays in `apps/server`, so test and asset paths do not move | ~10k | everything |

Critical path (longest chain): about 130k lines of 213k. The other crates build in parallel.

## How the cycle is cut

| Back-edge from `repositories` | Fix |
|---|---|
| `collab` pure helpers (markdown merge, sanitize, `DocKind`, …) | move down to `orbit-doc` |
| `export::{markdown, names}` | move down to `orbit-doc` |
| `CollabHub::revalidate_database` (~25 calls) | `orbit_store::revalidate::workspace(db, ws)`: it always does the live revalidate and the cluster send (as `collab/hub.rs:1665-1679` does today, also with no hub); the hub subscribes only for its local rooms |
| `CollabWrite` gate, `CollabHub` | collab hub moves down into the store (the hub writes back through `project_collab_content`, so it cannot be inverted safely); the sockets move up to the route crates |
| `live` (`LiveHub`, `NewEvent`, `Recipients`) | hub into the store, types into `orbit-domain` |
| `audit` (+ `webhooks::capture` + delivery-row SQL) | move down into the store |
| `billing` access types; `enqueue_*_in` | types to `orbit-domain`; the two job-row inserts into the store (outbox write, same signature) |
| importer `scan_job_kind`/`import_job_kind`, `workspace_export::job_kind` | one `orbit_store::jobs::kinds` module |
| `secret_box` | into the store |
| `mail_intake`, `mail_send::RecipientOutcome`, `mail::valid_address` | type and pure function down; `suspend_owned_mailboxes` SQL into `store::membership`, so mail repositories can sit above the core store |
| `sign_in_providers`, `object_storage` types, `auth_routes::avatar_url` | pure types and functions down |

## Synthesis decision

- **Base: candidate 1 (layered).** Its build model is correct: a changed crate makes every dependent run rustc
  again, and each crate pays its own macro expansion and metadata cost. Its first step (make the module graph
  acyclic inside the one crate) has value alone, and every later cut needs it.
- **Phase 1 from candidate 3 (minimal cut)**, because it is cheap and gives a stop point: `orbit-web-assets`,
  `orbit-openapi`, `orbit-doc`.
- **Grafts:**
  - Candidate 3: `orbit-openapi` as one sink crate in place of per-crate fragments. There is one list, and it is
    not in the server binary's graph.
  - Candidate 2: one `ApiError` family in `orbit-http`, and `orbit-formula` as a pure leaf.
  - Candidate 2: lists derived from the owning crates for job kinds, schedules and MCP tools.
  - Candidate 2: the revalidation signal, corrected as above. Its original form dropped the cluster send.
  - Candidates 2 and 4: tests that use one crate move into that crate's `tests/`.
  - Candidates 1 and 4: pure types into `orbit-domain`.
- **Rejected:**
  - Candidate 4's `Effected<T>` return values and the collab gate as a parameter: they change lock order and error
    paths at many call sites, for no build gain.
  - Candidate 2's 19 crates and its hooks called inside write transactions: this is the biggest rewrite, and it puts
    statement order under the write-lock rules at risk.
  - Long-lived re-export shims from `orbit_server`: they give two import paths for one item.

## Tradeoffs and expected gain (estimates, not measured)

| Edit or build | Today | After phase 1 | After phase 2 |
|---|---|---|---|
| Edit `openapi.rs` only (modifier, schema list) | 16 s | ~4 s (dev server does not rebuild it) | ~4 s |
| Add or change a handler | 16 s | 16 s (lib) + openapi crate only for `just api` | 7–8 s |
| Edit a repository | 16 s | 16 s | 11–12 s (all dependents rerun rustc) |
| Edit `orbit-doc` / kernel code | 16 s | 16 s | 14–16 s (rebuilds everything) |
| Full debug | 113 s | 106–109 s | 60–70 s |
| Release in Docker | 16.5 min | ~15.5 min | 8–10 min (needs ≥ 8 cores on the runner) |

Costs:

- Total CPU goes up 10–20%, because generics are built again in each crate.
- Many `pub(crate)` items become `pub` across crate boundaries (internals become importable).
- Repository edits, the biggest share, gain the least. A further gain needs a vertical cut inside the store, and
  pages, databases and tasks depend on each other (`repositories/pages.rs:20` ↔ `databases/{schema,import,templates}.rs`).

## Alternatives

| Candidate | Why not the base |
|---|---|
| 2: vertical feature crates (19) | best agent safety, but the biggest rewrite; hooks inside transactions; a core edit costs 18–22 s |
| 3: minimal cut (3 crates) | cheap and safe, but does not help repository and route edits (>80% of edits); used as phase 1 |
| 4: store first with `Effected` | lock-order and error-path risk; store edits get slower; a pages ↔ databases crate cycle |

## Migration order (each step compiles, passes `cargo check -p orbit-server --tests`, and can be the last)

Phase 0, without a crate split (cheap, measure first):

1. Add `[profile.dev] debug = "line-tables-only"` and `[profile.dev.package."*"] debug = false` to the repo's
   `Cargo.toml`. The global `~/.cargo/config.toml` on this machine already has them, but Docker and other machines
   do not.
2. Add `[profile.dev.build-override] opt-level = 3` (measured: edit loop −11%) and `opt-level = 3` for
   `libsqlite3-sys`, `argon2` and `blake2` (measured: slow tests −52%).
3. Profile with `cargo build --timings` and `-Zself-profile` to have a baseline for each step.

Phase 1, minimal cut (~1–2 days):

4. Spike: check that a downstream crate can list 5 handlers in `#[openapi(paths(...))]`. The utoipa-gen 5.5.0
   source emits a `pub struct __path_<fn>` for each handler; schema types in `components(...)` must be `pub`.
5. `orbit-web-assets`: move `static_assets.rs` and `CONTRACT_ID` (the only part of `openapi.rs` that the lib uses:
   `router.rs:140`, `app.rs:553`).
6. `orbit-openapi`: move `openapi.rs`. The `openapi` CLI command moves to a small bin in that crate (or `just api`
   runs it). Update `justfile` lines 90 and 101 and the tests `openapi_contract.rs`, `guest_access.rs`,
   `app_smoke.rs`. Gate: the `openapi.json` output is byte-identical to the baseline.
7. `orbit-doc`: move the pure collab and export helpers. Five back-edges of `repositories` go away.
8. Measure again. Continue to phase 2 only if the numbers justify it.

Phase 2, layered split (~1–2 weeks of agent work, one step per change):

9. Inside the one crate: apply the cycle cuts from the table above as plain module moves, until the module graph
   (re-run the edges script) is acyclic. No crate boundary changes in this step.
10. One `ApiError`/`ApiJson`/`ApiQuery`/`ProblemBody` in an `http` module. Separate change, gated on the OpenAPI
    byte diff (schema names change; regenerate the web client).
11. Extract `orbit-formula`, then `orbit-store`, `orbit-store-mail`, `orbit-http`, then one route crate at a time.
    Each extraction rewrites the test imports with a script in the same change: no lasting re-exports.
12. Move job kinds, schedules and MCP tools to lists that the owning crates export; add a startup check for
    duplicate job kinds.
13. Move tests that use one crate into that crate's `tests/`.

## Open questions

- How many cores does the Docker builder have? The release gain needs parallel crates.
- Does `utoipa-axum` handle every route shape (for example the billing guard)? It is deferred until the OpenAPI
  byte diff is stable.
- Is phase 2 worth 1–2 weeks when repository edits only go from 16 s to 11–12 s?

## Next step

Phase 0 steps 1–3, then the phase 1 spike (step 4).
