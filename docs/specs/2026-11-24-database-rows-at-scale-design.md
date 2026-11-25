# Databases at scale: server paging, then SQL queries

Date: 2026-11-24. Status: design. Measured on 2026-11-24 (section 9). Rollout step 1 is done. Section 9.4
changes the rollout: the Phase 1 snapshot cache is dropped.

## 1. Problem

A Docs database holds at most 5,000 live rows (`MAX_ROWS`, `apps/server/src/repositories/databases.rs:109`). Users
hit this limit with log, CRM and asset databases. The cap exists because every layer works on **all** rows:

| Layer | Today | File |
|---|---|---|
| Read | `GET …/databases/{db}` returns every row with stored and computed values. No limit, no cursor. | `database_routes.rs:256`, `databases.rs:473` (`get_with`) |
| Storage | One `page_property_values` row per cell, `value_json` TEXT. No index on the value, so SQL cannot filter or sort. | `migrations/postgres/0086_page_databases.sql:43` |
| Computed | Rollups and formulas are calculated on every read. A rollup loads **all rows of the target database** (`rows_in_tx`). | `databases/computed.rs:141`, `rollups.rs:217` |
| Client | `applyView` filters, sorts and searches all rows. Calculations and board counts run over all rows. | `packages/database/src/view.ts:140`, `calculate.ts:64`, `board.ts:90` |
| API query | `agent_query` loads the full detail, then filters and pages in memory (offset cursor). | `databases/agent.rs:796`, `query.rs:39` |
| Order | `pages.position` is dense `0..n`. Create and move renumber every sibling, one `UPDATE` each. | `pages.rs:3003`, `databases/rows.rs:186`, `:422` |
| Bulk writes | Property create, type change, delete, option removal, unique-id numbering and CSV import update every row in **one** transaction. | `properties.rs`, `search.rs:83`, `unique_id.rs:134`, `import.rs:610` |
| Pickers | Relation pickers fetch the whole target database. | web `cells/RelationCell.tsx:102`, `RelationConfig.tsx:135`; mobile `Relation.tsx:72` |

Raising the constant alone gives slow phones, large responses, long write locks (lock timeout 30 s) and a risk of
OOM on the server.

## 2. Goals and non-goals

**Goals**

- Phase 1: 50,000 rows per database, with no full row list on the client and bounded server memory.
- Phase 2: no practical limit (tested at 1,000,000 rows). Request cost depends on the page size, not the row count.
- Each request has a known upper bound on memory and time.
- Both phases work on SQLite and Postgres.
- Web and mobile change together.

**Non-goals**

- Offline mode, live cursors, CRDT for cells.
- A change to the Notion import cap (`notion/import.rs:66`, `MAX_PAGES = 5_000`). It is a separate limit.
- Search ranking changes in the global search.

## 3. One idea for both phases: a query endpoint

Both phases use **one** API. Only the engine behind it changes:

```
POST /api/v1/workspaces/{ws}/databases/{db}/query
{
  "view_id": "…",              // saved view config (filter, sorts, group, range)
  "config": { … },             // optional: unsaved local view (viewers who cannot edit, section 7.1 of P1)
  "search": "acme",            // optional
  "group": { "key": "opt_1" }, // optional: one board column or lane
  "range": { "from": "…", "to": "…" }, // optional: calendar and timeline window
  "cursor": "…",               // optional
  "limit": 100,                // 1..200, default 100
  "tz": "Europe/Budapest",
  "link_id": "…"               // as GET today
}
→ {
  "rows": [DatabaseRow…],      // same shape as today
  "next_cursor": "…" | null,
  "total": 48211,              // rows that match filter + search
  "groups": [{ "key": "opt_1", "count": 120 }…],   // board only, first request only
  "calculations": { "<property id>": <value> }      // footer, first request only
}
```

`GET …/databases/{db}` keeps the head, properties, views, templates and `row_count`. It stops returning `rows`.
Orbit is not public yet, so there are no old mobile builds to support: web and mobile move in the same change.

The engine is chosen per request by a **planner** (`databases/planner.rs`):

1. Phase 2: every filter, sort, search and group term can run in SQL → **SQL engine**.
2. Else, the database has ≤ `MEMORY_ENGINE_ROWS` (50,000) live rows → **memory engine** (Phase 1).
3. Else → 422 `database_query_unsupported` with the term that blocks it (for example "sort by a formula that uses
   `now()`"). The UI shows the reason on the view.

After Phase 2, the memory engine stays as a fallback for the few terms that SQL cannot do. Thus the two phases do
not need a rewrite of each other.

### 3.1 Cursor

The cursor is keyset, not offset, in both engines: `base64(json{ v: <view hash>, k: [<sort key values>…], id: <row
id> })`. The view hash covers config, search, group, range and tz (as `agent.rs:672` today).

- Memory engine: binary search for the first row after `(k, id)` in the sorted list.
- SQL engine: `WHERE (k1, …, id) > ($1, …, $n)`, written out as `OR` terms for mixed directions.
- An edit between two pages does not skip or repeat rows. A changed view hash returns 400 `cursor_mismatch`, and
  the client starts again from the top.
- `query_database` (MCP, REST) switches to this cursor too. Its limit stays 1..100.

## 4. Phase 1: memory engine on the server (cap 50,000)

### 4.1 Server

**Snapshot.** `DatabaseSnapshot` holds the stored data of one database in a compact form: row ids, titles, positions,
timestamps, and the parsed values per property (`Vec<Option<CellValue>>` per column, not JSON text), plus relation
links. It does not hold computed values.

**Snapshot cache.** A process-wide LRU (`databases/snapshot_cache.rs`):

- Key: `(database_id, workspace realtime sequence)`. Every write records an audit event, and the
  `realtime_sequences` trigger bumps the sequence in the same transaction (`0046_baseline.sql:809`). One
  `SELECT sequence` per request (`realtime.rs:126`) tells whether the snapshot is fresh. This is coarse (any write in
  the workspace makes a new snapshot), but it is correct on several servers and needs no new write-path code.
- A write path that does not record an audit event would leave the snapshot stale. Step 1 of the rollout lists all
  database write paths and adds a test for each one: write, then query, then expect the new value.
- Budget: 256 MB by estimated bytes. An entry larger than the budget is not cached.
- Single flight: concurrent requests for the same key wait for one build.

**Request flow.**

1. Check access (as `get_with` today).
2. Get the snapshot. Build it on a cache miss: two queries, read in batches of 2,000 rows (no full `fetch_all` of
   JSON text).
3. Compute rollups and formulas **only for the columns the view needs**: filtered, sorted, grouped, calculated
   columns over all rows. The columns that are only shown are computed for the returned page only.
4. Filter, search, sort, group and calculate with the Rust engine (`query.rs`, plus a Rust port of `calculate.ts`
   and the board grouping).
5. Return one page. Access filtering of relations (`relations.rs:218`) stays per request, because it depends on the
   actor.

**Guards against OOM and slow requests.**

- A semaphore allows at most `max(2, cpus / 2)` snapshot builds and memory-engine queries at the same time. Others
  wait up to 5 s, then get 503 `database_busy` with `Retry-After: 1`.
- The CPU work runs in `spawn_blocking`.
- A query that runs longer than 10 s stops and returns 503 `database_query_timeout`.
- Rollups read the target database through the same cache. Rollup depth stays 2 (`computed.rs:41`).

### 4.2 Writes that touch every row

At 50,000 rows, one transaction that updates every row would hold the write lock for too long. Each such write
becomes a short transaction plus a batched, idempotent job (batches of 200, `jobs` queue, transient lock errors mean
"retry next round"):

| Write | Inside the transaction | Job |
|---|---|---|
| Property create, type change, delete (search text) | One `UPDATE pages SET property_text = NULL WHERE parent_id = $1`. | Reuse `fill_search_text`, which already fills `NULL` rows in batches (`search.rs:209`). |
| Option removal | Remove the option from the property config. | Clear the option from values, 200 rows per transaction. Reads ignore unknown option ids until then (as `repair` does for views). |
| Unique-id numbering | Save the config. | Number the rows that have no value, in batches, in row order. |
| Type change (value conversion) | Save the new type and a `converting` flag on the property. | Convert values in batches; clear the flag at the end. The UI shows "Converting…" on the column. |
| CSV import | Create the database and its properties. | Insert rows in batches of 200; the job keeps the last CSV line done, so a restart continues from there. |

### 4.3 Row order: sparse positions

Keep the integer `pages.position` column. New rows get gaps: position `k * 1024`.

- Insert between two rows: the midpoint. Insert first or last: min − 1024 or max + 1024.
- When two neighbours have no gap left, renumber that database with gaps again in a job (batches of 200). The insert
  that found no gap renumbers only a local window of 100 rows in its own transaction.
- Code that expects `0..n` (`insert_index`, `placement_index`, client drag code) changes to "order by position, id".
  The API never exposes the position as an index.
- A migration does not rewrite old rows. They get gaps on the first renumber.

### 4.4 Clients (web and mobile)

- Use `useInfiniteQuery` per view, with `maxPages: 10` (at most 1,000 rows in memory) and both `getNextPageParam`
  and `getPreviousPageParam`. Sources: [useInfiniteQuery reference](https://tanstack.com/query/v5/docs/framework/react/reference/useInfiniteQuery),
  [Infinite queries guide](https://tanstack.com/query/v5/docs/framework/react/guides/infinite-queries).
- Table (web): the virtualizer (`DatabaseTable.tsx:102`) sizes the scroll area from `total`, and loads the next or
  previous page near the edge of the loaded window. Rows not loaded show a placeholder.
- Board: the first request returns `groups` with counts. Each column loads its own pages with `group`. "Show N more"
  stays (`DatabaseBoard.tsx:21`).
- Calendar and timeline: one request per visible window with `range`.
- Gallery and list: as the table.
- Footer calculations and board counts come from the server.
- Search box: debounce 250 ms, then a new query.
- Optimistic edits patch the loaded pages in place. A row that no longer matches the filter stays until the next
  refetch, as in Notion.
- Realtime: `workspace.changed` refetches the loaded pages only (TanStack refetches each stored page in sequence).
- Relation pickers call the query endpoint with `search` and `limit: 50`.
- `@orbit/database` keeps `matchesFilter`, `sortRows` and `calculate` for the optimistic edits and for the shared
  test cases. `applyView` is no longer used on full data.
- Mobile: `FlatList` with `onEndReached`, the same hooks and the same endpoint (`DatabaseView.tsx`,
  `Gallery.tsx`, `Relation.tsx`, `data/database-card.ts`).

### 4.5 Other paths

- Export (`export.rs:173`) and duplicate (`copy.rs`) read rows in batches of 2,000 and write the output as a stream.
- Backup restore keeps the row-capacity check.
- `MAX_ROWS` becomes 50,000. Import (`import.rs:142`) and the 409 error keep the same rule.

### 4.6 Gate for Phase 1

Before the cap changes, a benchmark on a 50,000-row database with 20 properties, 5 formulas, 2 rollups (target
database also 50,000 rows), 3 sorts and 5 filter rules must give:

| Measure | Limit |
|---|---|
| Snapshot build (cold) | ≤ 1.5 s |
| Query, warm snapshot | ≤ 300 ms p95 |
| Snapshot size | ≤ 60 MB |
| Server RSS with 4 such databases open | ≤ 400 MB |
| Longest write transaction (property delete, import batch) | ≤ 500 ms |

If a limit fails, ship with a lower cap (for example 20,000) and record the measured numbers here.

## 5. Phase 2: SQL engine (no practical limit)

### 5.1 Typed sort and filter keys

Add NULL-able key columns to `page_property_values` (same migration on SQLite and Postgres):

| Column | Type | Filled for |
|---|---|---|
| `num_key` | `DOUBLE PRECISION` / `REAL` | number, checkbox (0/1), unique id, number formulas and rollups |
| `text_key` | `TEXT` | text, url, email, phone, single select and status (option position as a zero-padded key for sort; option id for filter), text formulas. `lower(…)`, first 200 characters. |
| `date_key` | `BIGINT` | date start (Unix ms, UTC), date formulas |
| `date_end_key` | `BIGINT` | date end |

Multi-value cells (multi-select, person, relation, files) go to a new table:

```
page_property_items (
  page_id, property_id, item TEXT, position BIGINT,
  PRIMARY KEY (page_id, property_id, item)
)
INDEX (property_id, item, page_id)
```

Indexes: `(property_id, num_key, page_id)`, `(property_id, text_key, page_id)`, `(property_id, date_key, page_id)`.
Title and timestamps already live on `pages`. Add `pages (parent_id, lower(title), id)` for title sort.

**Writes:** `write_values`, `upsert_value` and `update_value` (`rows.rs:525-602`) compute the keys from the
parsed `CellValue` in the same statement. One function, `CellValue::sort_keys()`, gives the keys, so the write
paths cannot differ.

**Backfill:** a job fills the keys in batches of 200 per database, then sets `databases.sql_ready = 1`. Until then
the planner uses the memory engine (and the 50,000 cap still applies to that database).

### 5.2 Stored formulas and rollups

- Formula and rollup results are written to `page_property_values` with `computed = 1` and the same keys.
- **On a row write:** recompute that row's formulas in the same transaction (one row, cheap).
- **Rollups:** a write to row R enqueues `recompute_rollups(property, rows)` for the rows that link to R
  (`page_relations` index on `(related_page_id, property_id)`). The job runs in batches of 200.
- **On a formula or rollup config change:** the property gets a `computing` flag, and a job recomputes every row.
  The UI shows "Calculating…" on the column, and the planner treats the column as not ready (memory engine or 422).
- **Not stored (stay at read time):**
  - Volatile formulas (`now()`, `today()`, and date functions that depend on `tz`). They are computed for the
    returned page. Filter or sort on them uses the memory engine (≤ 50,000 rows) or returns 422.
  - Rollups over a relation whose target pages have different access from the database (another teamspace or a
    private page). A stored value would show data that the viewer cannot read. These are computed per request, as
    today.

### 5.3 SQL compiler

`databases/sql_query.rs` builds the query with `orbit_platform::SqlBuilder` from the view config:

- One `LEFT JOIN page_property_values v{i} ON v{i}.page_id = p.id AND v{i}.property_id = $n` for each filtered or
  sorted property; `EXISTS (SELECT 1 FROM page_property_items …)` for multi-value rules.
- `AND` / `OR` groups up to `MAX_FILTER_LEVELS` map to SQL directly.
- Empty values sort last in both directions, as the client does today. Portable form: `ORDER BY (v1.num_key IS
  NULL), v1.num_key DESC, p.id`.
- Relative dates ("today", "next week") are resolved in Rust with `tz` before the query and bound as numbers.
- Search: `page_search` restricted to `parent_id = $db` (FTS5 on SQLite, tsvector on Postgres).
  **Behaviour change:** today's view search is a substring match; FTS matches word prefixes ("acm" finds "Acme",
  "cme" does not). The UI search uses prefix matching on both backends.
- Aggregates: `COUNT`, `SUM`, `AVG`, `MIN`, `MAX`, count empty and count distinct on the keys; board groups with
  `GROUP BY text_key` or `item`.
- Range: `date_key < $to AND COALESCE(date_end_key, date_key) >= $from`.

**One truth for filter and sort:** the existing shared cases (`packages/database/cases/filter.json`, `sort.json`)
run against three engines: TypeScript, Rust memory and SQL on SQLite and Postgres. A case that fails on one engine
blocks the merge.

### 5.4 Removing the cap

- `MAX_ROWS` becomes 1,000,000: a safety limit for import, export and backup, not a product limit.
- `ensure_row_capacity` keeps the `COUNT(*)`; it uses the `pages_workspace_parent_position` index.
- Export, duplicate and backup stream in batches (section 4.5).

### 5.5 Gate for Phase 2

Benchmark at 1,000,000 rows, 20 properties, on SQLite and on Postgres:

| Measure | Limit |
|---|---|
| First page, 2 sorts + 3 filters | ≤ 200 ms p95 |
| Next page (keyset) | ≤ 100 ms p95 |
| Board group counts | ≤ 500 ms |
| Footer calculation (sum) | ≤ 500 ms |
| Server RSS during the run | no growth with row count |
| Row write incl. keys and own formulas | ≤ 50 ms |

## 6. Rollout (each step ends in a state you can check)

| # | Step | Check |
|---|---|---|
| 1 | List every database write path; test write → query returns the new value. | Server tests, SQLite + Postgres |
| 2 | Bulk writes as jobs (4.2) and sparse positions (4.3), cap still 5,000. | Tests: lock time per batch, restart in the middle |
| 3 | Snapshot cache, query endpoint, keyset cursor, memory engine, guards (3, 4.1). `query_database` uses it. | Tests + shared cases in Rust |
| 4 | Web: paged table, board, calendar, timeline, gallery, list, pickers (4.4). | Dev env in a browser at 50,000 seeded rows |
| 5 | Mobile port of step 4. | `bunx tsc --noEmit -p .`, `bun test src`, iOS bundle on Metro |
| 6 | Remove `rows` from `GET`. Benchmark gate 4.6. Cap to 50,000. | Benchmark numbers in this file |
| 7 | Key columns, items table, backfill job, `sql_ready` (5.1). | Migration on a copy of a large dev database |
| 8 | Stored formulas and rollups (5.2). | Tests: change a related row, wait for the job, query |
| 9 | SQL compiler and planner (5.3); shared cases on 3 engines. | Tests on both backends |
| 10 | Benchmark gate 5.5. Cap to 1,000,000 (5.4). | Benchmark numbers in this file |

Steps 1–6 are Phase 1. Steps 7–10 are Phase 2.

## 7. Risks

| # | Risk | Mitigation |
|---|---|---|
| 1 | A write path with no audit event leaves a stale snapshot. | Step 1 test per write path. |
| 2 | Busy workspaces invalidate snapshots often (coarse sequence key). | Measure the hit rate in step 6. If low, add a per-database `data_version` column, bumped by the write paths. |
| 3 | Stored rollups show data a viewer cannot read. | Store only when access matches (5.2); else compute per request. |
| 4 | Three filter engines disagree. | One shared case file, run on all engines in CI. |
| 5 | Search changes from substring to word prefix. | Write it in the release notes. |
| 6 | A user waits on "Calculating…" for a long time after a formula change on a big database. | Show progress (done / total) from the job. |
| 7 | Scroll position with keyset pages and `maxPages`. | Test scroll to bottom and back on 50,000 rows in step 4. |

## 8. Open questions

| # | Question | Proposal |
|---|---|---|
| OQ-1 | Should Phase 1 ship alone (50,000) before Phase 2 starts? | Yes. It removes the user problem for most cases. |
| OQ-2 | Snapshot budget: a fixed 256 MB, or a setting? | Fixed. Add a setting only if a real install needs it. |
| OQ-3 | Should a row that stops matching the filter after an edit disappear at once? | No, keep it until refetch (as Notion). |
| OQ-4 | 422 for an unsupported query on a large database, or a slow memory query with a lower page cap? | 422 with the reason. A slow query hurts every user of the server. |
| OQ-5 | Should the Notion import cap (5,000 pages) rise with Phase 1? | Separate task. |

## 9. Measurements (2026-11-24)

Harnesses: `apps/server/tests/database_scale_bench.rs` (server, release build, `BENCH_ROWS`, both backends; raise
`MAX_ROWS` locally to measure the row create above the cap) and `BENCH_ROWS=50000 bun run src/bench.ts` in
`packages/database` (client engine). Data: 20 properties (5 text, 3 number, 2 select, multi-select, 2 date,
checkbox, URL, relation with 1 to 3 links, 2 rollups, 5 formulas) and a related database of the same size. The
machine had 12 cores and a load average of 7 to 11 from other sessions during the server runs, so the ranges are
wide. Each number is the median of 5 runs unless it says otherwise.

### 9.1 Server (current full-load engine)

| Measure | SQLite 5k | SQLite 50k | Postgres 5k | Postgres 50k |
|---|---|---|---|---|
| `GET` (all rows + computed) | 355 ms | 5,539 ms (3,715..5,629) | 494 ms | 4,450 ms (3,390..5,095) |
| `query_database`, 2 filters, 2 sorts, 50 rows | 489 ms | 5,164 ms | 371 ms | 2,566 ms |
| Response size | 8.5 MB | 85.1 MB | 8.5 MB | 85.1 MB |
| Peak RSS added by one `GET` | 1 to 22 MB | 84 to 283 MB | 10 MB | 131 to 138 MB |
| Create a row at the top (renumbers all) | 126 ms | 989 ms | 1,904 ms | 7,246 ms |
| Add a text property (search text of all rows) | 344 ms | 5,382 ms | 2,783 ms | 13,518 ms |
| Delete a text property | 340 ms | 3,542 ms | 2,510 ms | 11,604 ms |

The write rows are single runs.

**What limits the read at 50k.** Temporary timers in `get_with` and `computed::fill_at` (removed again) split one
`GET`:

| Part | SQLite | Postgres |
|---|---|---|
| `rows_in_tx` (pages + 1M cell rows, JSON parse, merge) | 1.7 to 3.2 s | 0.8 to 1.1 s |
| `relations::fill` (about 100k links, access filter) | 0.54 to 0.79 s | 0.47 to 0.90 s |
| `fill_rollups` (loads the 50k-row target database) | 0.62 to 0.82 s | 0.44 to 0.87 s |
| `formula::fill_rows` (5 formulas) | 0.13 to 0.21 s | 0.12 to 0.26 s |

Computing formulas is cheap. Reading every cell from the database, plus the relations, is the cost.

### 9.2 Client engine (`applyView`, desktop)

| Rows | No search | Search with hits | Search without hits |
|---|---|---|---|
| 5,000 | 8.7 ms | 12.9 ms | 26.8 ms |
| 50,000 | 82 ms | 119 ms | 275 ms |

Medians of 25 runs. Phones are slower (not measured).

### 9.3 Dev env, real HTTP path

The dev env (debug build, Postgres, load average 1.0) got a generated database with 50,000 rows and 5 properties
(225,000 values). The data was deleted after the run.

- `GET` over HTTP: median 2.6 s (2.57..2.73 s, 6 runs). Wire size 30.1 MB. **The API sends no compression**:
  `--compressed` gets the same 30.1 MB. Gzip of the same body is 1.78 MB (17×).
- In the browser: the fetch takes 2.68 to 2.80 s and `JSON.parse` 21 to 43 ms. The 1-row database takes 21 to
  50 ms. The table showed its first row 5.4 s after navigation, with a JS heap of 212 MB.

### 9.4 What the numbers change

1. **The Phase 1 gate fails.** A snapshot build is a full load (2.5 to 4.5 s at 50k), against the 1.5 s gate.
   A cache only makes warm reads fast, and its key has a known gap (9.5). **Drop the snapshot cache.** Server
   paging with the memory engine stays, without a cache and with the cap at 5,000. Rows above 5,000 come with the
   SQL engine (Phase 2), which never loads every cell.
2. **The writes are a problem now, not at 50k.** On Postgres at 5,000 rows, a row added at the top (1.9 s) and a
   property change (2.5 to 2.8 s) already hold the global write lock longer than the 2 s warning. At 50k they take
   7 to 13.5 s, close to the 30 s lock timeout. Section 4.2 (jobs) and 4.3 (sparse positions) come first.
3. **No gzip step.** Gzip would make today's full `GET` about 17× smaller, but after paging (step 4) one page
   is about 170 KB, so the gain is small. It is not part of this plan.

New rollout order, replacing section 6:

| # | Step | Check |
|---|---|---|
| 1 | Write-path test (`apps/server/tests/database_write_sequence.rs`, 37 writes). **Done**; passes on SQLite and Postgres. | Done |
| 2 | Bulk writes: search text in a job, and row positions with gaps (9.6). **Done.** | Done |
| 3 | Query endpoint with keyset cursor (3, 3.1) on the memory engine, **no cache**, cap 5,000. | Tests + shared cases in Rust |
| 4 | Web and mobile paging (4.4). | Dev env in a browser; mobile checks |
| 5 | Phase 2 steps 7 to 10 of section 6, then the cap rises. | Gate 5.5 |

### 9.5 Write paths the realtime sequence does not see

Found by step 1. Clients also depend on these events (`workspace.changed` refetches), so they matter without a
cache too. Both are fixed:

- Live editor (collab) edits of a row body: `collab/hub.rs:1033` audits at most every 10 minutes. A page that
  became empty or not empty again within that time changed `has_content` with no event. Now
  `project_collab_content` audits such a change at once (test:
  `collab_api::a_page_that_becomes_empty_or_not_empty_is_audited_within_the_interval`).
- Cleanup of links to a trashed page (`unlink_trashed`, `pages.rs`): it rewrote linking pages in their own
  transactions with no audit event. A page block has no text, so this never changes `has_content`, but clients
  that show the linking page stayed stale. Now each rewritten page records `page.updated` (test:
  `database_write_sequence::trashing_a_page_that_a_row_body_links_to_sends_an_event_for_the_row`).

### 9.6 Step 2 result (2026-11-24)

What changed:

- **Search text.** A schema change (property create, type or config change, delete, CSV import, numbering of
  many rows) writes no row any more. It queues the `docs.search_text` job with `{"database_id": …}`
  (`search.rs`, `refresh_search_text_later`). The job computes the text of every row of that database again, 200
  rows per transaction in id order, then fills rows without text (`run_search_text_job`). Until the job reaches a
  row, search matches the row's old text. On the dev env the job ran 0.5 s after the request.
- **Row positions.** A new or moved row takes the middle of the gap between its neighbours (`rows.rs`,
  `row_position`, gap 65,536). No other row is written. When a gap has no room (after about 16 rows put into one
  gap, or rows numbered `0..n` by older code or page moves), all rows of the database are spread once with
  `renumber_spaced`. Trashing rows no longer renumbers.
- **`renumber`** (all page lists) writes one statement per 200 rows, and only rows whose position changes.
- Clients no longer shift the other rows by +1 after a create (web `api.ts`, mobile `hooks/database.ts`).

Measured with `database_scale_bench`, Postgres, single runs:

| Write | 5k before | 5k after | 50k before | 50k after |
|---|---|---|---|---|
| Create a row at the top | 1,904 ms | 37 ms | 7,246 ms | 287 ms |
| Add a text property | 2,783 ms | 411 ms¹ | 13,518 ms | 6 ms |
| Delete a text property | 2,510 ms | 382 ms¹ | 11,604 ms | 25 ms |
| Slowest search text job transaction | — | 189 ms¹ | — | 411 ms |
| First move in a database with `0..n` positions (spreads all rows once) | — | 278 ms | — | 2,495 ms |

¹ The 5k runs used the first version, which set the text of all rows to `NULL` inside the request (4.3 s at 50k).
The job version replaced it.

Follow-up fixes (same day):

- **`renumber` keeps positions.** It reads the current positions, keeps those of the longest run that is already
  in order, and gives each other page an evenly spaced free position between its kept neighbours
  (`pages.rs`, `placed`). Only when a stretch has no room is the whole list spread with gaps. All page lists
  use it (page tree, rows moved into or out of a database, restore, duplicate), so these paths write only the
  pages that move and no longer number a database's rows `0..n` again. Page-tree positions now have gaps too;
  clients send list indexes for moves and sort by position, so they need no change.
- **Restore** puts a page before the first sibling at or after its stored position (before: it used the
  stored position as a list index, which breaks with gaps).
- **`docs.row_positions` job** (daily, first at startup): gives the rows of each database numbered `0..n`
  positions with gaps, a database per transaction (`spread_dense_rows`). A request then never spreads a large
  old database. On the dev env the job spread a 4-row `0..n` database within 24 s of being due.
- Creating a row at 50k took 287 ms. Profile on Postgres (2026-11-25, timers per step):

  | Step | Time | Cause | Fix |
  |---|---|---|---|
  | Return the row (`single_row`) | 194 ms | Each rollup read and computed every row of the target database (50k) | `rollups::fill` reads only the linked rows (`rows_among`, batches of 500); above 2,000 links it reads the whole database in one pass |
  | Row position (`Siblings::edge`) | 20 ms | Postgres cannot estimate `IS NOT DISTINCT FROM`, so it sorted every row in place of reading the index `pages_workspace_parent_position` | `=` and `IS NULL` per space: 0.5 ms |
  | Row cap (`ensure_row_capacity`) | 11 ms | `COUNT(*)` over the rows | Kept; the cap changes in Phase 2 |

  After the fixes, at 50k rows on Postgres: create a row at the top 24 ms, move the last row to the middle 25 ms
  (the benchmark now seeds gap positions, as the job leaves old rows; with `0..n` rows a move spreads all rows,
  2.4 s), full GET 2.6 s (no change). The same rollup fix speeds up every row write that returns the row
  (value updates too).
- A relation cell write read the ids of every row of the target database to check access
  (`readable_rows`). It now checks only the pages of the write and the cell's current links
  (`readable_among`, batches of 500): linking a row to 3 target rows at 50k, Postgres, 118 ms → 21 ms.
