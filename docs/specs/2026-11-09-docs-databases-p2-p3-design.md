# Docs databases, phases 2 and 3 (P2, P3) design

Status: draft, 2026-11-09. Follows the [P1 design](2026-11-09-docs-databases-p1-design.md) ("P1"
below). This spec does not repeat P1. A reference such as "P1 4.3" points to a section of P1.

## 1. How to read this spec

- Each item (sections 3 to 18) is one unit of work. You can build, test and merge it alone,
  unless its **Needs** line names another item.
- Each item has the same parts: goal, data model, API, shared logic, web, mobile, Notion import,
  other areas (export, search, MCP), limits, tests, open questions.
- Sizes: **S** is about 1 to 3 days, **M** is about 1 week, **L** is 2 weeks or more.
- The current code is the reference for what exists. Section 2 lists the facts that this spec
  builds on.

### 1.1 Build order and dependencies

| # | Item | Phase | Size | Needs | Section |
|---|---|---|---|---|---|
| 1 | Inline database block and linked views (done, 3.11) | P2 | L | none | 3 |
| 2 | Calculation footer | P2 | S | none | 4 |
| 3 | AND/OR filter groups | P2 | M | none | 5 |
| 4 | Property values in search | P2 | M | none | 6 |
| 5 | Row templates | P2 | M | none | 7 |
| 6 | Board: group by person, sub-groups | P2 | M | none | 8 |
| 7 | CSV import and export | P2 | M | none (does the P1 export step, see 9.1) | 9 |
| 8 | Move pages into and out of a database | P2 | S | none | 10 |
| 9 | Relation and rollup | P3 | L | none (adds the computed-value layer, 2.3) | 11 |
| 10 | Files, email, phone, unique ID | P3 | M | none | 12 |
| 11 | List view, calendar view | P3 | M | the view-type migration (3.2) | 13 |
| 12 | MCP and token API for databases | P3 | L | 3 (final filter shape) | 14 |
| 13 | Formula | P3 | L | the computed-value layer (from 9, or built here) | 15 |
| 14 | Gallery view, timeline view | P3 | L | the view-type migration (3.2); gallery files preview needs 10 | 16 |
| 15 | Button property | P3 | M | none | 17 |
| 16 | Form view (members only) | P3 | M | the view-type migration (3.2) | 18 |

Later items extend earlier ones when both exist. For example, the calculation footer (2) gets
rollup and formula functions when 9 and 13 land. Each item lists these extensions under
"Later".

### 1.2 Riskiest unknowns (all items)

| Rank | Unknown | Item | How to settle it |
|---|---|---|---|
| 1 | A database UI inside a BlockNote block: focus, keyboard, selection, drag and drop and undo must not leak between the editor and the table. | 1 | Prototype the table in a `content: 'none'` block first (section 3.9). |
| 2 | Formula cost at read time: 5,000 rows × 20 formulas on every `GET`. | 13 | Benchmark the Rust evaluator with the worst-case limits before the UI. |
| 3 | Two-way relations across spaces with different access: what a viewer sees, and what a write may change on the other side. | 9 | Server tests for each access case (11.10). |
| 4 | The search index rebuild on SQLite (FTS5 cannot add a column) on a big workspace. | 4 | Time the migration on a copy of a large dev database. |
| 5 | Two filter implementations (TypeScript for clients, Rust for the API) that must agree. | 12 | One JSON case file that both test suites run (14.4). |
| 6 | Public form submissions: abuse, spam and a new unauthenticated surface. | 16 | Out of this spec (section 18). |

## 2. What exists today

### 2.1 Facts from the code

| Area | Fact | Where |
|---|---|---|
| Schema | Tables `database_properties`, `database_views`, `page_property_values`; `pages.kind IN ('page','database')`. The property `type` is checked in Rust only, so a new property type needs no SQL change. | `migrations/0078_page_databases.sql` |
| View types | `database_views.type IN ('table','board')`. A new view type needs a table rebuild on SQLite (as `0079`). | `migrations/0079_database_board_views.sql` |
| View config | `ViewConfig` (`#[serde(deny_unknown_fields)]`): `columns`, `sorts`, `filter.rules` (flat AND, max 20), `wrap_cells`, board fields. `repair` drops unknown ids. | `repositories/databases.rs` |
| Limits | `MAX_ROWS` 5,000, `MAX_PROPERTIES` 100, `MAX_VIEWS` 50, `MAX_OPTIONS` 200, `MAX_CELL_ITEMS` 100, `MAX_SORTS` 10, `MAX_RULES` 20. | `repositories/databases.rs` |
| Row cap | `ensure_row_capacity(tx, database_id, adding)` runs on every path that makes a row live. | `repositories/databases.rs` |
| Move | `PageRepository::move_page` returns `DatabaseMoveUnsupported` (409) for a row, or for a move into a database. | `repositories/pages.rs`, `page_routes.rs` |
| Duplicate | `Remap` rewrites `pageId` props of `page` blocks and page-file URLs in copied content. | `repositories/pages.rs` |
| Search | SQLite: FTS5 `page_search(title, body)` keyed by `page_search_rows`, filled by triggers on `pages`. Postgres: a plain table with a generated `search_vector` (`orbit_search` config), weights A (title) and B (body). | `migrations/0027_page_search_fts.sql`, `migrations/postgres/0046_baseline.sql`, `PageRepository` search |
| Task filters | Tasks already have AND/OR groups: `FilterGroup {op, children}`, `FilterNode = Group \| Condition`, `MAX_NESTING` 3, `MAX_CONDITIONS` 50. The web has `AdvancedFilterDialog`. | `repositories/task_filter.rs`, `apps/web/src/features/views/components/` |
| Editor | Custom blocks `page`, `callout`, inline `mention`, `math`. Every client builds from `createEditorSchemas`. `schema.test.ts` checks against `apps/server/src/collab/blocknote-schema.json`. | `packages/editor/src/schema.ts` |
| Shared logic | `@orbit/database`: `values`, `filter` (`matches`), `sort`, `view` (`applyView`, repair), `board` (`groupRows`), `newRow`, `meta` (`operatorsOf`), `dates`. | `packages/database/src/` |
| Mobile | `components/database/` (`DatabaseView` shows a table as a list and a board as columns, `ViewSheets`, `ValueEditor`, `PropertyList`, `cells`). | `apps/mobile/src/components/database/` |
| Notion import | `flatten_filter` keeps AND parts and drops an OR group of more than one condition (`view_filter`). Relations, formulas and rollups become static values. | `notion/database.rs` |
| MCP / token API | Docs tools `list_docs`, `get_doc`, `create_doc`, `update_doc`; scopes `docs:read`, `docs:write`; personal tokens only for docs. | `mcp.rs`, `token_api.rs`, `docs/mcp.md`, `docs/public-api.md` |
| Export | P1 section 9 (database as a Markdown table) is **not built yet**: the export code has no database handling. Item 7 does it. | `repositories/page_export.rs`, `apps/web/src/features/docs/pageExport.ts` |

### 2.2 Rules for every item

- **Both databases.** Every migration has a SQLite file and the same change in
  `apps/server/migrations/postgres/`, with its entries in `crates/platform/src/db/migrate.rs`
  (both runners), `SUPPORTED_SCHEMA_VERSION`, the version in
  `crates/platform/tests/{database,backup}.rs` and `apps/server/tests/app_smoke.rs`. A new route
  updates the counts in `apps/server/tests/openapi_contract.rs`, then `just api`. Give each new
  `ToSchema` type a unique `#[schema(as = …)]` name (`.ai/lessons.md`).
- **Migration numbers.** The next free number when the item merges (0080 and up at the time of
  writing). This spec does not fix numbers.
- **SQL.** `$n` placeholders, `SqlBuilder`, `orbit_platform::Row`, `i64` flags, `ON CONFLICT`.
  Writes use `immediate_transaction()`. Do slow work (parsing, conversion, formula checks) before
  the transaction.
- **Errors.** 422 `validation_failed` with the field path, as in P1 5.2.
- **Port to mobile.** Each item says what mobile gets. Where mobile does not get an editor, the
  screen says "Edit on the web" at the place where the user looks for it (P1 8).
- **Realtime.** Every write records an audit event and an outbox event (resource type
  `database`), as in P1 5.2.

### 2.3 The computed-value layer (shared by items 9, 13, 15)

Rollups (9) and formulas (13) are computed values. Decision D-P3-1: the **server computes them
when it reads the database** and stores nothing.

- `GET /databases/{id}` computes every rollup and formula value for every row and puts it in
  `values`, in the same shape as stored values. Clients sort, filter and show them like any
  value. Clients never evaluate rollups or formulas.
- Why read time: there is nothing to invalidate. A rename of an option, an edit in a related
  database or a change to a formula needs no recompute job. 5,000 rows is small enough
  (benchmark in 15.9).
- `GET` takes `?tz=<IANA zone>` (default `UTC`). Date functions and `today()` use it. The web and
  mobile clients send the viewer's zone.
- A write returns the row with fresh computed values. Other rows whose computed values can
  change (rollups of related rows) arrive with the refetch after the realtime event.
- Code: `apps/server/src/repositories/databases/computed.rs` (new module). The first of items 9
  and 13 to land adds it.

## 3. Item 1: Inline database block and linked views (P2, L)

### 3.1 Goal

Show a database inside a page, as Notion does. Two kinds:

| Kind | What it shows | Its views |
|---|---|---|
| **Inline database** | A database that was made in this page (or a database chosen to show "as is") | The database's own views, the same tabs as its full page |
| **Linked view** | An existing database, anywhere in the workspace | Its own views, stored for this block only. A filter change here does not change the source database's views. |

Decision D-P2-1: the database of an inline block is a **normal sub-page of the host page, shown
in the tree** (not hidden like rows). Reasons:

- A `page` block today is a link, and deleting it does not trash the sub-page. The inline block
  behaves the same: the database keeps its own lifecycle (trash, move, permissions, favorites).
- If the tree hid it, a deleted block would leave the database with no place to find it.
- Rows stay hidden from the tree, as in P1.

### 3.2 Data model

One migration. It also widens the view type check for all planned layouts, so items 11, 14 and
16 need no table rebuild. Rust (`ViewType`) still accepts only the built types.

```sql
CREATE TABLE database_links (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    host_page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX database_links_database ON database_links (database_id);
CREATE INDEX database_links_host ON database_links (host_page_id);
CREATE INDEX database_links_workspace ON database_links (workspace_id);

-- SQLite: rebuild database_views as 0079 does, with:
--   link_id TEXT REFERENCES database_links(id) ON DELETE CASCADE,
--   type TEXT NOT NULL CHECK (type IN ('table','board','list','calendar','gallery','timeline','form'))
CREATE INDEX database_views_link ON database_views (link_id, position, id);
```

- A view with `link_id = NULL` is one of the database's own views. A view with a `link_id`
  belongs to one linked view block.
- Scope triggers (SQLite) and checks (Postgres): a link's `database_id` is a `kind = 'database'`
  page of the same workspace; its `host_page_id` is a page of the same workspace; a view's
  `link_id` names a link of the same database.
- `MAX_VIEWS` (50) counts the own views. A link has at most 20 views.
- The host page's permanent delete removes its links and their views (cascade). A trashed host
  keeps them.

### 3.3 Editor block

New block `database` in `packages/editor/src/schema.ts` (`pageEditorSchema` only; the task
schema leaves it out and the task sanitizer drops it, as it drops `page`). Update
`blocknote-schema.json` and the converter in `apps/server/src/collab/blocknote.rs`.

```ts
const databaseBlockConfig = {
  type: 'database',
  propSchema: {
    databaseId: { default: '' },
    /** Empty: the database's own views (inline database). Else a `database_links` id (linked view). */
    linkId: { default: '' },
  },
  content: 'none',
} as const
```

`EditorViews` gets `database: { render, toExternalHTML? }`. The selected view of a block is per
user, in `localStorage` (`orbit:database-view:<database_id>:<block id>`), not in the block. So
two people can look at two tabs of the same block.

### 3.4 API

| Route | Change |
|---|---|
| `GET /databases/{id}?link={link_id}` | `views` holds the link's views instead of the own views. 404 when the link is not of this database. |
| `POST /databases/{id}/links` | Body `{host_page_id, from_view_id?}`. Makes a link with one view, a copy of `from_view_id` (default: the first own view). Result: `{link, views}`. Needs read access to the database and edit access to the host page. |
| `DELETE /databases/{id}/links/{link_id}` | 204. Not called by the editor (see 3.6). For cleanup tools. |
| `POST/PATCH/DELETE /databases/{id}/views…` | New optional field `link_id` (body for `POST`, query for `PATCH`/`DELETE`). The last view of a link cannot be deleted (409, as P1). |
| `POST /pages` | Unchanged. The slash menu makes an inline database with `kind: "database"` and `parent_id` = host. |
| `POST /pages/{id}/duplicate` | `Remap` also rewrites `databaseId` (when the database is copied with the page) and `linkId` (a new link with copies of its views). |

Access:

| Action | Needs |
|---|---|
| Read rows, own views | read access to the database |
| Edit values, add rows | edit access to the database (P1 rules) |
| Change an own view | edit access to the database; database lock blocks it |
| Change a linked view | edit access to the **host page**; the host page lock blocks it. A member without it changes the view locally ("Changes are only visible to you", P1 7.1). |
| See a block of a database you cannot read | The block shows "No access to this database" and no title. |

### 3.5 Web

- **Slash menu:** "Database – inline" makes a database under the host page and inserts a
  `database` block. "Linked view of database" opens a database picker (search over databases
  the user can read), then `POST …/links`, then inserts the block with `linkId`. The P1 item
  "Database – full page" stays.
- **Block layout:** the database icon and title (a click opens the full page), the view tabs,
  the toolbar (P1 7.1), then the table or board. A linked view shows a small "↗" after the title.
  The block uses the editor width and scrolls sideways inside it.
- **Rows shown:** the first 25 rows, then "Load more" (25 more each time). A board shows 25 cards
  per column, then "Show 25 more". No virtualization inside the editor.
- **Row peek:** opens as on the full page (`?row=<id>`, P1 7.5).
- **Code:** `DatabasePage` splits into `DatabaseSurface` (tabs, toolbar, table or board), used by
  the full page and the block. The block passes `linkId` and `rowLimit`.
- **Editor isolation:** the block root is `contentEditable={false}`. Keys, pointer events,
  `drag*` events and paste inside it stop at the root, so BlockNote does not see them. The block
  is selectable as a whole from outside (click on its left margin, or arrow keys from the block
  above), and Backspace deletes it then.
- **Delete the block:** the block goes (undo works). The database stays as a sub-page. A toast
  says "The database is still in the page tree" with "Move to trash".
- **Turn into:** a `page` block that links to a database can turn into a `database` block, and
  back.
- **Missing or trashed database:** the block shows a muted line "Database in trash" or "Database
  not found", like `PageBlockCard`.

### 3.6 Orphan links

The editor never deletes a link: a deleted block can come back with undo or from page history.
Orphan links cost a few rows. They go with the host page's permanent delete. Open question
OQ-1.1 asks for a cleanup job.

### 3.7 Mobile

- `PageEditor.dom.tsx` renders a `database` block as a card: icon, title, row count, the name of
  the first view. A tap posts a message to native, which opens the database screen
  (`docs/[id].tsx`) with `link` and the view. No table inside the mobile editor.
- The database screen takes `?link=<id>` and then lists the link's views.
- "Database – inline" and "Linked view of database" are not in the mobile slash menu: "Edit on
  the web".

### 3.8 Other areas

| Area | Behavior |
|---|---|
| Notion import | Notion uses a `child_database` block for inline and full-page databases. An inline database (`is_inline: true` on the database) becomes a `database` block. A full-page database becomes a `page` link block, and so does a database the import could not read. Both kinds stay sub-pages of their page. Notion API views of linked databases: see OQ-1.2. |
| Markdown (`get_doc`, export) | The block exports as a link `[<database title>](/docs/<id>)`. The page export (item 7) also writes the view's rows as a Markdown table under the link. |
| Markdown merge (`update_doc`) | The block is kept as is, like a `page` block. Markdown cannot create it. |
| Search | No change. The block has no text. |
| Backups, restore of a page | A restored host page keeps its blocks. A block whose link is gone shows "View not found" with "Show the database's own views" (clears `linkId`). |

### 3.9 Tests

| Area | What to prove |
|---|---|
| Server | link create needs host edit and database read; a link view write does not change own views; last link view cannot be deleted; host permanent delete removes links; duplicate rewrites `databaseId` and `linkId`; 404 for a link of another database; both databases |
| Editor | `schema.test.ts` parity with the new block; the task schema has no `database` block |
| Web | typing in a cell does not change the editor document; Backspace in a cell does not delete the block; the block shows "No access to this database" for a 404 |
| Prototype first | Before the full build: a block with a text field, a popover and a drag source inside the editor, checked in the browser for focus, undo and drag. |

### 3.10 Open questions

| # | Question |
|---|---|
| OQ-1.1 | Add a job that deletes links whose block is gone from the host page and from its history for 30 days? |
| OQ-1.2 | Does the Notion API return the views of a linked database block? If yes, import them as a link. |
| OQ-1.3 | Should the database of an inline block move with its block when the block is cut and pasted into another page? (Proposal: no. The block keeps pointing to the same database.) |

### 3.11 Status

Done on 2026-11-09 (migration `0080_database_links`). Decisions: D-P2-1 as written; OQ-1.1 no cleanup job; OQ-1.3 the
block keeps pointing to the same database; OQ-1.2 see below. Differences from this section:

| # | Item | Where | Why |
|---|---|---|---|
| 1 | `POST …/links` with a host page the caller cannot see answers 422 `validation_failed` (`host_page_id`), not 404. | `repositories/databases/links.rs` | The database was found; the body names a bad page. |
| 2 | A link's 21st view answers 409 `database_link_view_limit`. `DELETE …/links/{id}` needs the host page like a link view write. | `database_routes.rs` | The spec names no code; cleanup should not bypass the host lock. |
| 3 | A 404 shows "No access to this database" unless the database is in the trash. "Database not found" is only for a block without a database id. | `editor/DatabaseBlock.tsx` | The server answers 404 for both a deleted database and one the viewer cannot read. |
| 4 | A row peek opened from a block puts `?row=<id>&row_block=<block id>` in the URL. | `database/DatabaseSurface.tsx` | Two blocks of one database must not both open the peek. |
| 5 | "Turn into" is its own item of the block handle menu ("Turn into inline database", "Turn into page link"). | `editor/BlockSideMenu.tsx` | The "Turn into" list holds text block types only. |
| 6 | The Markdown export writes the link only, not the rows table. | `export/markdown.rs` | Item 7 builds the database export. |
| 7 | Notion: the current API has view endpoints (`GET /v1/views`, "including linked views"), but nothing says a linked `child_database` block id is accepted. Import keeps a `database` block of the database's own views. | `notion/convert/mod.rs` | OQ-1.2 is open: linked views could become links later. |
| 8 | Mobile saves linked view changes and lets the server refuse them; it has no "only visible to you" drafts for a locked host. | `apps/mobile/src/components/database/` | Mobile does not know the host page lock on the database screen. |
| 9 | The toolbar's "Copy link to view" in a linked view block copies a view id that the full page does not have (it opens the first view). | `database/ViewToolbar.tsx` | Not decided yet. |

## 4. Item 2: Calculation footer (P2, S)

### 4.1 Goal

A row under the table with one calculation per column, as Notion's "Calculate"
([Notion: tables](https://www.notion.com/help/tables)). It counts the rows of the view after
filter and search.

### 4.2 Data model

No migration. View config gets:

```json
{ "calculations": { "<property id>": "count_values" } }
```

Repair drops unknown properties and functions that no longer fit the type (after a type change).

### 4.3 Functions

| Function | Types | Result |
|---|---|---|
| `count_all` | all | rows in view |
| `count_values` | all except checkbox | non-empty values; a multi-select or person cell counts each item |
| `count_unique` | all except checkbox | distinct values (items, for lists) |
| `count_empty`, `count_not_empty` | all except checkbox | rows |
| `percent_empty`, `percent_not_empty` | all except checkbox | % of rows, 1 decimal |
| `sum`, `average`, `median`, `min`, `max`, `range` | `number` (later: number rollups and formulas) | number in the property's format |
| `earliest_date`, `latest_date`, `date_range` | `date`, `created_time`, `last_edited_time` | date; range as "3 days", "2 months" |
| `checked`, `unchecked`, `percent_checked`, `percent_unchecked` | `checkbox` | count or % |

The checkbox functions follow the Notion API rollup list
([Notion API: property object](https://developers.notion.com/reference/property-object)); the
help page does not list per-type footer functions.

### 4.4 Shared logic

`@orbit/database/calculate.ts`: `calculate(rows, property, fn, ctx) → {value: string, raw}`,
and `calculationsFor(type)` for the menu. Status counts as its option. Empty rules follow P1 6.

### 4.5 Web

- The footer row sits under "+ New", 33 px, muted text, sticky at the bottom of the scroller.
- Each cell shows "Calculate" on hover when it has no function. A click opens a menu of the
  functions for the type, plus "None". The cell shows `<short label> <value>`, for example
  "Sum 1,240" or "Empty 12%".
- A change is a view change (shared at once, or local; P1 D1).

### 4.6 Mobile

The row list ends with a "Summary" section: one line per calculation (`<property> · <label>
<value>`), read only. To change a calculation: "Edit on the web".

### 4.7 Other areas

Notion import: Notion's API does not expose view calculations (unverified); nothing to map.
Export, search, MCP: no change.

### 4.8 Tests

`@orbit/database`: each function on empty, one and many rows; multi-select item counts; status
default; percent rounding; median of an even count. Server: repair drops a function after a
type change. Web: picking "Sum" sends one view `PATCH` and shows the value.

### 4.9 Later

Board columns get a count per column today; per-column calculations wait for user demand
(OQ-2.1). Items 9 and 13 add rollup and formula results by their result type.

## 5. Item 3: AND/OR filter groups (P2, M)

### 5.1 Goal

Filters with groups, as Notion's advanced filters
([Notion: views, filters and sorts](https://www.notion.com/help/views-filters-and-sorts)).
Notion's UI nests groups up to three layers. Notion's API nests up to two levels
([Notion API: filter](https://developers.notion.com/reference/post-database-query-filter)).
Decision D-P2-2: **three levels** (the root group counts as level 1), the same as Orbit task
filters (`MAX_NESTING` 3). Every Notion API filter fits.

### 5.2 Data model

The filter shape becomes the task filter shape, with database rules as leaves:

```json
{ "filter": { "op": "and", "children": [
  { "property_id": "p1", "operator": "is", "value": ["opt_1"] },
  { "op": "or", "children": [
    { "property_id": "p2", "operator": "contains", "value": "rust" },
    { "property_id": "p3", "operator": ">", "value": 10 }
  ] }
] } }
```

- Migration: rewrite each `database_views.config_json` from `{"rules": [...]}` to
  `{"op": "and", "children": [...]}`. SQLite uses `json_set`/`json_extract`; Postgres uses
  `jsonb_set` on `config_json::jsonb`, cast back to text. No compatibility code reads the old
  shape after the migration.
- Limits: 3 levels, 20 rules in total (P1), 10 children per group. An empty group matches every
  row.
- Repair: a rule that names a deleted property goes; a group left empty goes; a group with one
  child stays (the UI shows it as a group).

### 5.3 Shared logic

- `filter.ts`: `matchesFilter(row, group, properties, ctx)` walks the tree. `matches` (one rule)
  is unchanged.
- `newRow.ts`: a new row gets values only from rules that are direct children of an `and` root,
  and from `and` groups under it. `or` groups give nothing.
- `meta.ts`: `isSimpleFilter(group)` (an `and` root with only rules) decides the bar style.

### 5.4 Web

- A simple filter shows the P1 chips. Any `or` or nested group shows one chip "N rules" that
  opens the advanced filter popover.
- Advanced popover: one line per rule (`Where` / `And` / `Or`, property, operator, value, "⋯"
  with Remove, Duplicate, Turn into group). "+ Add filter rule", "+ Add filter group" (hidden at
  level 3). The `And`/`Or` select of the second line sets the group's `op`.
- Reuse the layout of the task `AdvancedFilterDialog`, not its code: the leaves differ.

### 5.5 Mobile

The filter sheet edits a simple filter as today. A view with groups shows "Advanced filter (N
rules) · Edit on the web" and still applies it (shared logic).

### 5.6 Notion import

`flatten_filter` becomes `map_filter`: it keeps `and` and `or` groups up to 3 levels. A condition
with no Orbit operator is dropped (`view_filter`). A group left empty is dropped. A filter deeper
than 3 levels is dropped whole (`view_filter`).

### 5.7 Tests

`@orbit/database`: `or` of two rules; nested `and` in `or`; empty group; a rule with no value in
an `or` group (matches all, so the group matches all, as in P1 4.3). Server: migration rewrites a
P1 config on both databases (test from `embedded_through`); depth and count limits give 422 with
the path (`config.filter.children[1].children[0]`). Notion: a two-level `or` maps intact.

### 5.8 Open questions

| # | Question |
|---|---|
| OQ-3.1 | Should a rule in an `or` group with no value match all rows (P1 rule) or no rows? Notion's choice for groups is unverified. Proposal: keep P1, so a half-made rule never hides rows. |

## 6. Item 4: Property values in search (P2, M)

### 6.1 Goal

Docs search finds a row by its property values ("Reading", "Rust", "ACME-12"), not only by its
title and body.

### 6.2 Data model

```sql
ALTER TABLE pages ADD COLUMN property_text TEXT;   -- NULL = not computed yet
```

- SQLite: drop and recreate `page_search` with three columns `(title, body, props)`, refill it
  from `pages`, and recreate the triggers with `property_text` (`coalesce(…, '')`,
  `AFTER UPDATE OF title, content_text, property_text`). `page_search_rows` stays, so rowids do
  not change.
- Postgres: `ALTER TABLE page_search ADD COLUMN props TEXT`, drop and re-add the generated
  `search_vector` with `props` at weight `C`, recreate the GIN index and the trigger functions.

### 6.3 What `property_text` holds

A row's searchable values, joined with newlines: `text`, `url`, `email`, `phone`, `number` (as
written, no format), option names of `select`, `status`, `multi_select`, `date` as ISO, unique ID
(`PREFIX-12`). Not included: `person` (names change without a value write), `checkbox`,
`created_*`, `last_edited_*`, computed types. A non-row page has `NULL`.

### 6.4 Writes

The repository computes `property_text` in Rust (one function, `search_text(row, properties)`)
and writes it in the same transaction as:

| Write | Rows to update |
|---|---|
| Value write, row create, row move | that row |
| Option rename or delete | rows that hold the option |
| Property delete or type change | all rows of the database |
| Row moves out (item 8) | set to `NULL` |

5,000 rows is the most one write touches.

**Backfill:** the migration leaves every row `NULL`. A job (the existing `jobs` queue, one job per
database, idempotent) fills `NULL` rows in batches of 500. It treats a transient lock error as
"retry on the next round" (`.ai/lessons.md`).

### 6.5 Search results

- Ranking: SQLite `bm25(page_search, 10.0, 1.0, 2.0)`; Postgres `ts_rank` weights for A, B, C.
- Snippet: SQLite `snippet()` with column `-1` (best column). When the best hit is in `props`, the
  result shows the matched value, for example "Row · Research · Status: Reading". Postgres: the
  same with `ts_headline` over `props` when the body has no hit.

### 6.6 Clients

Web and mobile show the new snippet. No other change: the result already opens the row.

### 6.7 Tests

Server, both databases: a row is found by an option name; rename of the option finds the new
name and not the old; a property delete removes its words; backfill fills `NULL` rows and a second
run changes nothing; a page moved out of a database is not found by its old values.

### 6.8 Open questions

| # | Question |
|---|---|
| OQ-4.1 | Include person names (needs a refresh when a member renames)? Proposal: no. |
| OQ-4.2 | Should the view search (P1, client side) and docs search agree on what is searchable? Today the view search covers visible properties' display text, which includes people. Proposal: leave them different. |

## 7. Item 5: Row templates (P2, M)

### 7.1 Goal

A template fills in a new row's title, icon, values and body
([Notion: database templates](https://www.notion.com/help/database-templates)).

### 7.2 Data model

A template is a hidden child page of the database. Its values are normal
`page_property_values`, so the P1 scope trigger works.

```sql
CREATE TABLE database_templates (
    page_id TEXT PRIMARY KEY REFERENCES pages(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX database_templates_database ON database_templates (database_id, position);
```

- Every query that reads "the rows of a database" excludes template pages
  (`NOT EXISTS (SELECT 1 FROM database_templates t WHERE t.page_id = pages.id)`): the row list,
  `row_count`, `ensure_row_capacity`, `GET /pages/{id}` `database_id`, search, export, duplicate
  (copies templates as templates), restore, the tree. Riskiest part: find every such query;
  write one SQL fragment `LIVE_ROW` and use it everywhere.
- A template is not in docs search and not in recent pages.
- View config gets `default_template_id` (`null` = empty row). Repair drops a deleted template.
- Max 20 templates per database.

### 7.3 API

| Route | Body | Result |
|---|---|---|
| `GET /databases/{id}` | | adds `templates: [{id, name, title, icon, position}]` |
| `POST /databases/{id}/templates` | `{name, from_row_id?}` | the template; `from_row_id` copies a row's title, icon, values and body |
| `PATCH /databases/{id}/templates/{template_id}` | `{name?, position?}` | the template |
| `DELETE /databases/{id}/templates/{template_id}` | | 204; the page goes away (no trash), views drop it as default |
| `POST /databases/{id}/rows` | new optional `template_id` | the row with the template's title, icon, values and body; `values` in the request override the template's |

- Template body and values are edited through the normal page routes (collab editor, `PATCH
  …/rows/{template id}/values`), so the template editor is the row page.
- The body copy uses the duplicate code (flush, copy `content_json`, `Remap`).
- Dynamic values: a `person` value `"me"` and a `date` value `"today"` resolve at row create.
  They are stored in the template's values as these literals (a new value shape for templates
  only; validation allows them only on template pages).

### 7.4 Web

- "New ▾" (P1 7.1) shows the templates, "Empty", and "+ New template". Each template has "⋯":
  Edit, Set as default for this view, Duplicate, Delete.
- Edit opens the template in the side peek, with a yellow bar "You are editing a template".
- A board column "+" and the table "+ New" use the view's default template, then the filter
  values (`newRowValues`) on top.

### 7.5 Mobile

"+" uses the view's default template. A long press on "+" lists the templates. Create or edit a
template: "Edit on the web".

### 7.6 Other areas

| Area | Behavior |
|---|---|
| Notion import | The public Notion API does not expose database templates (unverified, OQ-5.2). No mapping. |
| Export | Templates are left out. |
| MCP | Item 12's `create_database_row` takes `template_id`. |

### 7.7 Tests

Server: a template is not a row (list, count, cap, search, tree, export); a row from a template
copies title, icon, values and body; request values override; `"me"` and `"today"` resolve; the
template limit; delete clears `default_template_id`; duplicate of the database copies templates.

### 7.8 Open questions

| # | Question |
|---|---|
| OQ-5.1 | Repeating templates (Notion: daily, weekly, …) need a scheduler. Proposal: not now. |
| OQ-5.2 | Confirm whether the Notion API exposes templates. |

## 8. Item 6: Board, group by person and sub-groups (P2, M)

### 8.1 Goal

Group a board by `person` (and `multi_select`), and add a second level of groups (swim lanes)
([Notion: boards](https://www.notion.com/help/boards)).

### 8.2 Data model

No migration. Board config:

| Field | Meaning |
|---|---|
| `group_by` | now also a `person` or `multi_select` property |
| `sub_group_by` | optional; a `select`, `status`, `person`, `multi_select` or `checkbox` property, not the same as `group_by` |
| `sub_group_order`, `hidden_sub_groups` | as `group_order` and `hidden_groups`, for lanes |
| `hide_empty_groups` | `false` by default; hides columns and lanes with no cards |

For `person`, a group id is a user id. Repair drops users who are no longer members. For
`checkbox`, the ids are `"true"` and `"false"`.

### 8.3 Rules

- **A card in many columns.** A row with people A and B shows in column A and column B, as
  Notion does for multi-value groups. The count of a column counts its cards.
- **Columns:** a person board shows the members in `group_order`, then the other people that
  appear in the rows, by name, then "No <property>". A multi-select board shows all options, as
  select.
- **Drag between columns:** remove the source group's id from the list and add the target's
  (others stay). Drop on "No <property>" clears the value. Drop from "No <property>" sets one id.
  One `POST …/rows/{id}/move` with `values` (P1 D7).
- **Lanes:** each lane is a horizontal band with the same columns. The lane header has the pill,
  the count and a collapse button. Collapse state is per user (`localStorage`). A card drop into
  another lane and column sets both values in one request.

### 8.4 Shared logic

`board.ts`: `groupRows` takes a `GroupProperty` of the four types and returns a card per (group,
row) pair. New `laneRows(rows, laneProperty, config)` returns lanes, each with `groupRows`
inside. `moveValues(row, from, to, property)` returns the value patch.

### 8.5 Web

- "⋯ → Group by" lists select, status, person and multi-select. "⋯ → Sub-group" lists the
  sub-group types and "None".
- With lanes, the column headers are sticky at the top, the lane headers sticky at the left.
- 25 cards per cell, then "Show 25 more".

### 8.6 Mobile

The board shows person and multi-select columns (shared logic). With lanes, a lane picker at the
top shows one lane at a time. Change the group or sub-group: "Edit on the web".

### 8.7 Notion import

A board grouped by `people` or `multi_select` keeps its grouping. Sub-groups: Notion's API view
object exposes them is unverified (OQ-6.1); map them when present.

### 8.8 Tests

`@orbit/database`: a row with two people in two columns; drag patch keeps other people; lane
grouping; `hide_empty_groups`. Server: `sub_group_by` equal to `group_by` is 422; repair drops a
non-member. Web: a drop into another lane sends one request with two values.

### 8.9 Open questions

| # | Question |
|---|---|
| OQ-6.1 | What does Notion's view API expose for sub-groups? |
| OQ-6.2 | Group by date (by day, week, month)? Not confirmed in Notion's help. Proposal: later. |

## 9. Item 7: CSV import and export (P2, M)

### 9.1 Goal

Export a view or a database as CSV, import a CSV as a new database, and merge a CSV into an
existing database ([Notion: import data](https://www.notion.com/help/import-data-into-notion)).
This item also builds P1 section 9 (Markdown export of a database), which is not built yet.

### 9.2 Export

| Where | What |
|---|---|
| Web "⋯ → Export view as CSV" | The rows of the view (filter, search, sort) and its visible columns, in order. Built on the client from `displayText`. |
| Page export (`page_export.rs`, `pageExport.ts`) | A database exports as `<title>.md` (P1 9: a Markdown table of the first view, rows as sub-pages) and `<title>.csv` (all rows, all properties, property order). |
| Workspace export | The same as the page export. |
| Mobile | "⋯ → Export view as CSV" opens the share sheet. |

CSV rules (`@orbit/database/csv.ts` and the same rules in Rust `export/csv.rs`):

- RFC 4180, UTF-8 with BOM (so spreadsheet apps read accents), CRLF line ends, header row of
  property names.
- Values are the display text (P1 6), with a fixed format for machines: numbers without
  grouping, dates as ISO 8601, multi-values joined with ", ".
- Formula injection: a text cell that starts with `=`, `+`, `-`, `@`, tab or CR gets a leading
  `'`, so a spreadsheet app does not run it. Number cells are not changed.

### 9.3 Import

Decision D-P2-3: the **client parses** the CSV (it needs the rows for the preview and the column
mapping) and the **server converts** the text with the P1 4.2 rules (`text → <type>`), which
exist and are tested. There is one conversion implementation.

| Route | Body | Result |
|---|---|---|
| `POST /databases/{id}/import` | `{columns: [{name, target: {property_id} \| {new: type} \| "skip"}], rows: [[cell text, …]]}` | `{created_rows, converted, cleared, created_options}`; all or nothing, one transaction |
| `POST /pages` | new optional `csv_import: {title_column, columns, rows}` with `kind: "database"` | the database, created with the imported properties instead of the defaults, in one transaction |

- One column must map to the title. Text cells over 10,000 characters are cut
  (`values_truncated`).
- A merge adds rows. It does not update existing rows (as Notion).
- New options are created from distinct values, up to 200 per property (P1 4.2).
- Person cells match member names or emails (case ignored); others are cleared.
- Limits: 5 MB file, 5,000 rows (and the row cap), 100 columns. The import route's body limit is
  8 MB.

Shared logic: `parseCsv(text) → string[][]` (quotes, embedded newlines, BOM, `,` or `;` or tab
detected from the first line) and `guessType(column) → PropertyType` (number, date, checkbox,
url, email, select when there are ≤ 50 distinct short values, else text). The client shows the
guessed type per column; the user can change it.

### 9.4 Web

- Docs tree "+" menu and Settings → Import: "Import CSV" (new database).
- Database "⋯": "Merge with CSV".
- A dialog: file drop, a preview of 5 rows, one line per column (CSV name → property or "New
  <type>" or "Skip"), then "Import N rows". Errors show the row and column.

### 9.5 Mobile

Export: yes (share sheet). Import and merge: "Edit on the web" (the mapping needs width).

### 9.6 Tests

`@orbit/database`: CSV round trip (quotes, newlines, BOM); `guessType`; injection guard. Server:
import creates options; a bad row rolls back all; row cap 409; title column required; both
databases. Export golden files for a database with each property type.

### 9.7 Open questions

| # | Question |
|---|---|
| OQ-7.1 | Update existing rows on merge (by title or by an ID column)? Proposal: later. |
| OQ-7.2 | Export the current view or all rows in the page export? Proposal: all rows in the CSV, first view in the Markdown table (P1 9). |

## 10. Item 8: Move pages into and out of a database (P2, S)

### 10.1 Goal

Move a page into a database (it becomes a row) and a row out (it becomes a page). Remove the 409
`database_move_unsupported`.

### 10.2 Rules

| Move | Result |
|---|---|
| Page → database | The page is a row, last in manual order. Its sub-pages stay its sub-pages. Row cap checked (`ensure_row_capacity`, 409 `database_row_limit`). |
| Database page → database | Refused: 409 `database_in_database` ("A database cannot be a row."). A database inside a row's subtree is fine. |
| Row → outside | The row is a page. Its values are deleted in the same transaction. Its relation links go (item 9). `property_text` is `NULL`. |
| Row → other database | Values move when the target has a property with the same name (case ignored) and the same type. Options and people match by name or id; no options are created. Other values are deleted. |
| Template page | Cannot move (409 `database_template_move`). |

The move route gets `dry_run: true`, which returns `{dropped_values: N}` and moves nothing.

### 10.3 API

`POST /pages/{id}/move` as today, with the rules above, and `dry_run`. `POST …/rows/{id}/move`
stays for order inside a database.

### 10.4 Web

- The tree: a page dragged onto a database item becomes a row (drop "into" only).
- Page "⋯ → Move to" lists databases as targets.
- Row peek and row page "⋯ → Move to": any page or database. When values would be lost, a
  confirmation says "N property values will be removed".

### 10.5 Mobile

The move sheet lists databases as targets, with the same confirmation.

### 10.6 Tests

Server, both databases: page into a database is a row with no values; cap; a database cannot be
a row; row out drops values (and search text); row to another database keeps same-name,
same-type values; dry run moves nothing; undo (move back) works.

## 11. Item 9: Relation and rollup (P3, L)

### 11.1 Goal

Link rows to rows of another database (or the same), one-way or two-way, and show values of the
linked rows ([Notion: relations and rollups](https://www.notion.com/help/relations-and-rollups)).

### 11.2 Data model

```sql
CREATE TABLE page_relations (
    property_id TEXT NOT NULL REFERENCES database_properties(id) ON DELETE CASCADE,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    related_page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (property_id, page_id, related_page_id)
);
CREATE INDEX page_relations_related ON page_relations (related_page_id, property_id);
CREATE INDEX page_relations_workspace ON page_relations (workspace_id);
```

- Links live here, not in `page_property_values`, so a permanently deleted page loses its links
  by cascade, and the reverse side is one index lookup.
- **One row per link, for both sides.** A two-way pair has properties A (in X, to Y) and B (in Y,
  to X). Links are stored under the **primary** property (A). B reads `page_relations` with
  `property_id = A` and `related_page_id = <row>`. Nothing needs syncing.
- Scope checks: `page_id` is a row of the property's database; `related_page_id` is a row of the
  target database (`config.database_id`); same workspace.

| Type | `config_json` |
|---|---|
| `relation` | `{"database_id": "…", "limit": "none" \| "one", "synced_property_id": "…" \| null, "primary": true \| false}` |
| `rollup` | `{"relation_property_id": "…", "target_property_id": "…", "function": "…"}` |

Value shapes in the API: relation `["<page id>", …]` (order: `position` on the primary side,
`created_at` on the reverse side); rollup as its result (11.5).

### 11.3 Relation rules

- **Two-way:** "Show on <database>" creates B in the target database (name: the source database
  title, made unique). Turning it off deletes B; links stay under A.
- **Delete A when B exists:** links move to B (swap `page_id` and `related_page_id`, set
  `property_id = B`), B becomes primary, in one transaction.
- **Self relation:** target = the same database. Two-way gives two properties (for example
  "Blocked by" and "Blocking"), as in Notion.
- **Trash:** links stay. Trashed pages are left out of values, rollups and filters, and come back
  with a restore.
- **Duplicate a database:** relation properties keep their target and links; a two-way relation
  becomes one-way in the copy (as Notion).
- **Move a row out (item 8):** its links on both sides go.
- **Limits:** 100 links per cell for writes on either side. The reverse side can show more (it
  has no write through the cell when it is over 100: "Edit from <other database>").
- **Type change** to or from relation clears values (P1 4.2 last line).

### 11.4 Access

| Case | Behavior |
|---|---|
| Create a relation to Y | Needs read access to Y. Two-way needs edit access to Y. |
| Read a relation cell | Lists only related pages the viewer can read. The response has `hidden_count` per cell ("+2 without access"). |
| Write a relation cell | The new ids must be rows of Y that the writer can read. Hidden links in the cell are kept, not removed. |
| Write the reverse side (B) | The same, against X. |
| Rollup over pages the viewer cannot read | They are left out of the result. The cell shows "Some pages are hidden" in its tooltip. |

### 11.5 Rollup functions

From Notion's help and API
([Notion API: property object](https://developers.notion.com/reference/property-object)):

| Function | Target types | Result |
|---|---|---|
| `show_original` | any | list of values in the target's type |
| `show_unique` | any | list, duplicates removed |
| `count_all`, `count_values`, `count_unique`, `count_empty`, `count_not_empty` | any | number |
| `percent_empty`, `percent_not_empty` | any | number (percent format) |
| `sum`, `average`, `median`, `min`, `max`, `range` | number | number |
| `earliest_date`, `latest_date`, `date_range` | date types | date (or range) |
| `checked`, `unchecked`, `percent_checked`, `percent_unchecked` | checkbox | number |

- A rollup of a rollup or formula is allowed, one level deep (no chains; 422 otherwise).
- Computed on read (section 2.3). The server loads the related rows' values in one query per
  relation property.
- Filter and sort use the result type: number and date operators, and `contains` over the
  display text for lists.

### 11.6 Shared logic

`values.ts`: `relation` (`{kind: 'pages', ids}`) and `rollup` (`{kind: 'computed', value}`)
cells. `filter.ts`: relation `contains`, `not_contains`, `is_empty`, `is_not_empty`. `sort.ts`:
relations are not sortable (the sort menu leaves them out); rollups sort by result. `calculate.ts`:
number functions accept number rollups.

### 11.7 API

| Route | Change |
|---|---|
| `GET /databases/{id}` | rows have relation and rollup values; new `related_pages: {"<id>": {title, icon, database_id}}` for every visible related page; `hidden_count` per relation cell |
| `POST …/properties` | `type: "relation"` with `{database_id, limit, two_way}`; the result includes the created synced property |
| `PATCH …/rows/{id}/values` | a relation value replaces the cell's visible links |
| Realtime | A value write in Y also sends a `database` event for every database with a relation or rollup that targets Y. |

### 11.8 Web and mobile

- Web cell: pills with page icon and title; a click opens the related row's peek. Edit: a picker
  that searches the target database (`GET /databases/{target}`, titles only), with "Linked" and
  "Add" sections.
- Property editor: target database picker, "Limit: 1 page / No limit", "Show on <database>"
  switch. Rollup: relation, property and function selects.
- Mobile: relation cells show titles and open the related row; edit through a native search
  sheet. Create or change relation or rollup properties: "Edit on the web".

### 11.9 Notion import

Relations stay relations when both databases are in the import. The import maps Notion
`dual_property` to two-way (`synced_property_id` gives the pair) and `single_property` to
one-way. Links to pages outside the import become `relation_titles` text in a separate `text`
property ("<name> (titles)"). Rollups import as rollups when their relation imported; otherwise
static (`rollup_values`). The import creates all databases in pass 1, then the links in a pass
after it, so order does not matter.

### 11.10 Tests

Server, both databases: two-way link read from both sides; delete A moves links to B; self
relation; trash hides and restore shows; permanent delete removes links; access cases of 11.4,
each one; reverse cap; rollup functions on numbers, dates, checkboxes and empties; realtime event
for the target database; duplicate makes one-way. Notion: dual and single property mapping,
outside links.

### 11.11 Open questions

| # | Question |
|---|---|
| OQ-9.1 | Notion's help does not say what happens to links when a related page is deleted. Proposal: as 11.3 (hidden in trash, removed forever with the page). |
| OQ-9.2 | Relations to databases in another space: allowed (proposal), with the access rules of 11.4. |

## 12. Item 10: Files, email, phone, unique ID (P3, M)

### 12.1 Data model

No migration for email and phone (types are checked in Rust). Files and unique ID:

| Type | `config_json` | `value_json` | Notes |
|---|---|---|---|
| `email` | `{}` | `"name@example.com"` (max 320, one `@`, no spaces) | shows as a `mailto:` link |
| `phone` | `{}` | `"+36 1 234 5678"` (max 50; digits, spaces, `+ - ( ) .`) | shows as a `tel:` link |
| `files` | `{}` | `[{"file_id": "…", "name": "…"}]` (max 20) | `file_id` is a `page_files` row of the row page |
| `unique_id` | `{"prefix": "TASK" \| null, "next": 13}` | `12` (integer, read only) | computed type for writes |

- **Files:** upload through the existing page file route of the row
  (`page_file_routes.rs`), then write the value. A removed file stays in `page_files` until the
  existing reconciliation finds no reference; the reference scan must include `files` values.
- **Unique ID:** adding the property numbers the existing rows by `created_at`, then `id`, from 1.
  A new row gets `next`, and `next` grows in the same transaction (the row create already holds
  the write lock). Numbers are never reused, also after delete
  ([Notion: unique ID](https://www.notion.com/help/unique-id)). Max one per database. The
  prefix is 1 to 10 letters, digits or `_`.

### 12.2 Shared logic, web, mobile

| | Email, phone | Files | Unique ID |
|---|---|---|---|
| Filter | text operators | `is_empty`, `is_not_empty` | number operators |
| Sort | text | count | number |
| Web cell | link; inline edit | file chips; a popover with upload, drag in, reorder, remove | `PREFIX-12`, muted |
| Mobile | link; text sheet with the matching keyboard | chips; upload from the photo library or files app | `PREFIX-12` |

### 12.3 Notion import

`email` and `phone_number` become `email` and `phone` (not `text`). `unique_id` becomes
`unique_id` with its prefix and numbers, `next` = max + 1. `files` become `files`: the import
downloads uploaded files as it does for file blocks, and keeps external links as text
(`files_as_names` only for those).

### 12.4 Other areas

Search (item 4) includes email, phone and unique ID. Export: files as names, unique ID as
`PREFIX-12`. CSV import guesses email.

### 12.5 Tests

Server: email and phone validation; unique ID numbering of existing rows, new rows, no reuse
after delete, concurrent row creates get distinct numbers (two `App`s on one Postgres,
`.ai/lessons.md`); a file value must name a file of the same row; the file reference scan keeps
referenced files.

## 13. Item 11: List view and calendar view (P3, M)

Needs: the view type check from 3.2. If item 1 has not landed, this item does that rebuild.

### 13.1 List view

- Goal: a compact list ([Notion: views](https://www.notion.com/help/views-filters-and-sorts)).
- Config: `columns` (as table: order and hidden; width ignored), `sorts`, `filter`.
- Web: one line per row, 36 px: icon, title, then the visible properties right-aligned as small
  pills and text. Drag to reorder without sorts. A click opens the peek.
- Mobile: the P1 row list is this view. A list view shows its visible properties (not the first
  3).

### 13.2 Calendar view

- Goal: rows on a month or week grid by a date property
  ([Notion: calendars](https://www.notion.com/help/calendars)).
- Config:

```json
{ "date_property_id": "…", "calendar_mode": "month" | "week", "card_properties": ["…"] }
```

- `date_property_id`: a `date`, `created_time` or `last_edited_time` property; repair picks the
  first date property. Week start comes from the viewer's locale, with a per-user override in
  `localStorage`.
- Rules: a range covers each of its days (a bar across the days in a week row). A time shows on
  the card. A row with no date is not on the grid; a "No date (N)" button lists them.
- Drag a card to another day: move start and end by the same number of days, keep the time.
  Drag the right edge of a range: change the end. Created and edited times cannot move.
- "+" on a day: a new row with that date (plus filter values and the default template).
- Web: a month grid (6 weeks) or a week (7 columns), "Today", "<" ">", the month title. Max 3
  cards per day cell, then "+N more".
- Mobile: a month grid with dots, and a day list under it (tap a day). Drag: not on mobile;
  change the date in the row's property list.

### 13.3 Shared logic

`calendar.ts`: `calendarWeeks(month, weekStart, timeZone)` and `placeRows(rows, property, range,
ctx)` returning per-day cards and multi-day bars with lanes. `moveDate(value, days)`.

### 13.4 Notion import

`list` and `calendar` views map to Orbit list and calendar views (no longer `view_type:<type>`).
A calendar keeps its date property when it imported as a date.

### 13.5 Tests

`@orbit/database`: month weeks across a year end and DST; a range across two weeks; a moment's day
in the viewer's zone; `moveDate` keeps time and length. Server: a calendar without a date property
repairs to none and the client shows "Choose a date property".

## 14. Item 12: MCP and token API for databases (P3, L)

Needs: item 3 (the filter shape the API takes).

### 14.1 Goal

Let agents and scripts read a database, query rows with filters and sorts, and add or change
rows, with the same scopes as doc tools.

### 14.2 Decision D2 revisited (D-P3-2)

Keep the 5,000-row cap. Do **not** add typed value columns. The server filters and sorts in Rust,
in memory, over the rows of one database:

- Typed columns cannot filter computed values (rollups, formulas, section 2.3) anyway.
- 5,000 rows in memory is small.
- The cost is a second filter and sort implementation. Section 14.4 keeps the two equal.

### 14.3 Tools and endpoints

| MCP tool | REST | Scope | Arguments | Result |
|---|---|---|---|---|
| `get_database` | `GET /docs/{database_id}/database` | `docs:read` | `database_id` | properties (id, name, type, options by name), views (id, name, type), `row_count` |
| `query_database` | `POST /docs/{database_id}/database/query` | `docs:read` | `filter` (the view filter shape, property names or ids), `sorts`, `view_id` (use its filter and sorts), `limit` (1 to 100, default 50), `cursor` | rows `{id, title, values, url}` and `next_cursor` |
| `create_database_row` | `POST /docs/{database_id}/rows` | `docs:write` | `title`, `values`, `markdown` (body), `template_id` | the row |
| `update_database_row` | `PATCH /docs/{database_id}/rows/{row_id}` | `docs:write` | `values`, `title` | the row |
| `trash_database_rows` | `POST /docs/{database_id}/rows/trash` | `docs:write` | `row_ids` (max 100) | count |

- `list_docs` lists databases with `kind`. `get_doc` of a row returns its body as today plus
  `database_id` and `values`.
- **Values for agents:** keys are property names or ids (a name that matches two properties is
  an error). Select, status and multi-select take option names or ids, never create options.
  Person takes user ids or emails. Dates take ISO 8601. Results use names for options and
  `{id, name}` for people.
- `query_database` takes `tz` for relative dates and formulas.
- **Writes** act as the token's creator, check database access and lock as the web routes do, and
  are last-write-wins (P1 5.2). No `expected_version` for values.
- Service-account tokens get no database tools, as for docs.
- Schema changes (properties, views) are not in the API (OQ-12.1).

### 14.4 One filter, two languages

- New `apps/server/src/repositories/databases/query.rs`: `matches_filter`, `sort_rows`, the same
  rules as `filter.ts` and `sort.ts` (P1 6).
- New `packages/database/cases/filter.json` and `sort.json`: `{properties, rows, filter or sorts,
  ctx, expected_ids}`. `bun test` and `cargo test` both run every case. A change to one side
  without the other fails a test.

### 14.5 Docs and limits

Update `docs/mcp.md` (Tools, Permissions table) and `docs/public-api.md` (Docs section, Scopes
table). Limits: 100 rows per query page, 100 rows per trash, 50 values per write. The cursor is
opaque (offset plus a hash of the filter and sorts; a changed filter gets a 422).

### 14.6 Tests

Server: each tool with and without scope; a private database of another member is not found;
names and ids as keys; ambiguous name error; option by name; cursor paging; a value write on a
locked database succeeds (the lock blocks only schema and views, P1 5.2). The shared case files
pass in both suites. Both databases.

### 14.7 Open questions

| # | Question |
|---|---|
| OQ-12.1 | Add tools for properties and views (create a database with a schema)? Proposal: after usage data. |

## 15. Item 13: Formula (P3, L)

Needs: the computed-value layer (2.3). Rollup references need item 9.

### 15.1 Goal

A property whose value is an expression over the row's other properties
([Notion: formula syntax](https://www.notion.com/help/formula-syntax),
[Notion: formulas](https://www.notion.com/help/formulas)).

### 15.2 Where it runs (D-P3-3)

**On the server only, in Rust, at read time.** Clients show the result. The formula editor asks
the server for a preview. Reasons:

- One implementation. "The same result on server, web and mobile" holds by construction.
- Filters, sorts, exports and the API (item 12) see the same values.
- The other options are worse: a TypeScript copy needs a golden-test fence and still drifts; a
  WASM build of the Rust evaluator does not run in the mobile app's Hermes runtime without
  extra work (unverified, so not chosen).

Cost: a cell shows the new formula result after the write's response (about one round trip).

### 15.3 Stored config

```json
{ "expression": "if(prop(\"p_123\") > 10, \"big\", \"small\")", "result_type": "text" }
```

- The stored expression names properties by id. The editor shows names. `@orbit/database/formula
  /tokens.ts` has a small tokenizer that converts names ↔ ids and highlights tokens. It does not
  evaluate.
- `result_type` is inferred by the server at save time: `number`, `text`, `boolean`, `date`,
  `people`, `pages`, or `list<…>` of these.

### 15.4 Grammar

```
expr     = ternary
ternary  = or ( "?" expr ":" expr )?
or       = and ( ( "or" | "||" ) and )*
and      = not ( ( "and" | "&&" ) not )*
not      = ( "not" | "!" ) not | compare
compare  = add ( ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) add )?
add      = mul ( ( "+" | "-" ) mul )*
mul      = unary ( ( "*" | "/" | "%" ) unary )*
unary    = "-" unary | power
power    = postfix ( "^" unary )?
postfix  = primary ( "." name "(" args? ")" )*      -- a.f(b) is f(a, b)
primary  = number | string | "true" | "false" | list | call | name | "(" expr ")"
list     = "[" ( expr ( "," expr )* )? "]"
call     = name "(" args? ")"
args     = expr ( "," expr )*
string   = '"' ( char | '\"' | '\\' | '\n' )* '"'
number   = digits ( "." digits )? ( ( "e" | "E" ) "-"? digits )?
```

`name` alone is a `let` variable, `current` or `index` (inside `map`, `filter` and friends).

### 15.5 Types and values

| Property type | Formula type |
|---|---|
| `title`, `text`, `url`, `email`, `phone`, `select`, `status` | text (option name) |
| `number`, `unique_id` | number |
| `checkbox` | boolean |
| `date`, `created_time`, `last_edited_time` | date |
| `multi_select` | list of text |
| `person`, `created_by`, `last_edited_by` | list of people |
| `relation` | list of pages (title only in v1) |
| `rollup` | its result type |
| `formula` | its `result_type` |

Empty values: `empty()` is true for `""`, `0`, `[]` (as Notion) and for no value. Text `+`
text joins; number `+` text is a type error (no silent conversion; use `format()`).

### 15.6 Functions (v1)

| Group | Functions |
|---|---|
| Logic | `if`, `ifs`, `empty`, `equal`, `unequal`, `let`, `lets` |
| Text | `length`, `substring`, `contains`, `lower`, `upper`, `trim`, `replace`, `replaceAll` (plain text, no regex), `repeat`, `format`, `split`, `join`, `link` |
| Math | `add`, `subtract`, `multiply`, `divide`, `mod`, `pow`, `abs`, `round(n, places?)`, `ceil`, `floor`, `sqrt`, `min`, `max`, `sum`, `mean`, `median`, `sign`, `toNumber`, `pi`, `e` |
| Date | `now`, `today`, `dateAdd(d, n, unit)`, `dateSubtract`, `dateBetween(a, b, unit)`, `dateStart`, `dateEnd`, `dateRange`, `formatDate(d, pattern)`, `parseDate`, `year`, `month`, `date`, `day`, `hour`, `minute`, `week`, `timestamp`, `fromTimestamp` |
| People | `name`, `email` |
| List | `at`, `first`, `last`, `slice`, `concat`, `sort`, `reverse`, `unique`, `includes`, `find`, `findIndex`, `filter`, `some`, `every`, `map`, `flat`, `length` |
| Other | `id` (the row's page id) |

Names and argument orders follow Notion, so users can paste Notion formulas. Not in v1: `test`,
`match` (regex), `style`, `unstyle`, reading properties of related pages
(`prop("Tasks").map(current.prop("Done"))`, OQ-13.1). `units`: `years`, `quarters`, `months`,
`weeks`, `days`, `hours`, `minutes`. `formatDate` patterns: `YYYY`, `YY`, `MMMM`, `MMM`, `MM`,
`M`, `DD`, `D`, `dddd`, `ddd`, `HH`, `H`, `hh`, `h`, `mm`, `A`, and literal text in `[…]`.

### 15.7 Errors

| When | What the user sees |
|---|---|
| Save: parse error | The editor underlines the place and says, for example, "Expected `)` after the arguments of `if`". 422 `formula_invalid` with `{message, start, end}`. |
| Save: type error | "`+` cannot add number and text. Use format() to make text." Same 422. |
| Save: unknown property, cycle | "No property named "Statis"." / "This formula uses itself through "Total"." |
| Read: runtime error (division by zero, `at` out of range, step limit) | The cell shows a muted "Error" with a warning icon. Hover (web) or tap (mobile) shows the message. Sort puts errors after empty values; filters treat them as empty; calculations skip them. |
| A referenced property is deleted | The formula is saved again with the reference marked; all cells show "Error: a property was deleted". The editor shows the token in red. |

### 15.8 Limits

| Item | Limit |
|---|---|
| Expression length | 2,000 characters |
| Syntax tree | 500 nodes, depth 32 |
| Steps per cell | 2,000 (a deterministic counter, so the same input always gives the same result) |
| List size inside a formula | 1,000 items |
| Text result | 10,000 characters |
| Formula properties per database | 20 |
| References through other formulas | 15 levels (Notion's layer limit) |

### 15.9 Cost check

Before the UI: a Rust bench with 5,000 rows × 20 formulas, each at the node limit, with `map` over
100-item lists. Target: a `GET` under 300 ms on the dev machine. If it misses, cache results per
(row `updated_at`, formula id) in memory per server.

### 15.10 API, web, mobile

| | |
|---|---|
| API | `POST …/properties` and `PATCH` accept `{expression}` (names or ids; the server stores ids). New `POST /databases/{id}/formula/preview` `{expression}` → `{result_type, error?, samples: [{row_id, value}]}` for the first 10 rows of the view. |
| Web | Formula editor: a code field with highlight and name completion (`prop` names, functions with signatures), the error line, and a preview list. A cell shows the result in its type's style. |
| Mobile | Shows results and errors. Edit the formula: "Edit on the web". |
| Notion import | Notion's API gives the formula expression (`formula.expression`). The import parses it with the Orbit parser: when it parses and type checks, keep a live formula; else a static value of the result type, as today (`formula_values`). |
| Search | Not included (computed). |

### 15.11 Tests

Rust unit tests: every function (happy path and errors), operator precedence, `let` scope, step
limit, cycle check, type inference, time zone of `today()`. A fixture file of Notion formulas with
expected results (`formula_cases.json`). Server: preview; deleted reference; both databases. Bench
from 15.9.

### 15.12 Open questions

| # | Question |
|---|---|
| OQ-13.1 | Read properties of related pages inside formulas (Notion 2.0 does)? It makes rollups mostly unneeded but needs related rows at read time. Proposal: v2, after rollups. |
| OQ-13.2 | Rich text output (`style`, `link` as a link)? Proposal: `link` only. |

## 16. Item 14: Gallery view and timeline view (P3, L)

Needs: the view type check from 3.2. Files preview needs item 10.

### 16.1 Gallery

- Goal: cards in a grid with a preview ([Notion: galleries](https://www.notion.com/help/galleries)).
- Config: `{"card_preview": "none" | "cover" | "content" | {"files": "<property id>"},
  "card_size": "small" | "medium" | "large", "fit_image": false, "card_properties": [...]}`.
- `content` preview: the first image of the body, else the first 300 characters of
  `content_text`. The `GET` adds `preview: {image_url?, text?}` per row only when a gallery
  view asks for it (`?previews=1`).
- Web: grid of cards, 180 / 260 / 340 px wide by size, preview 3:2. "+ New" as the last card.
  Drag to reorder without sorts.
- Mobile: a 2-column grid (1 column for `large`).

### 16.2 Timeline

- Goal: bars on a time axis ([Notion: timelines](https://www.notion.com/help/timelines)).
- Config: `{"start_property_id": "…", "end_property_id": "…" | null, "zoom": "day" | "week" |
  "month" | "quarter" | "year", "show_table": true, "table_columns": [...]}`. With no end
  property, a date range's end is the end; a single date is a one-day bar.
- Web: a left table pane (title and `table_columns`, can hide), a right scrolling axis with the
  zoom select and "Today". Drag a bar to move both dates; drag an edge to change one. Rows without
  a date list under "No date". Rows render in view order, virtualized like the table.
- Mobile: no timeline. The screen shows the view as a list grouped by month, with "Timeline · Edit
  on the web".

### 16.3 Shared logic

`timeline.ts`: `axis(zoom, range, timeZone)`, `barOf(row, config, ctx)`, `moveBar(value, delta,
edge)`. Gallery needs only `applyView`.

### 16.4 Notion import

`gallery` and `timeline` views map to Orbit views; card preview and size when the API gives
them.

### 16.5 Tests

`@orbit/database`: bar placement at each zoom; a range across DST; `moveBar` on start, end and
both. Server: `previews=1` returns the first image of a body; a timeline repair with a deleted
end property.

## 17. Item 15: Button property (P3, M)

### 17.1 Goal

A cell with a button that runs actions on the row
([Notion: database buttons](https://www.notion.com/help/database-buttons)).

### 17.2 Config

```json
{ "label": "Mark done", "actions": [
  { "type": "set_values", "values": { "<property id>": "<value>" } },
  { "type": "add_row", "database_id": "…", "values": { … } },
  { "type": "open_url", "url": "https://…" }
], "confirm": "Close this task?" | null }
```

- Actions (v1): `set_values` on this row (values may be `"me"`, `"today"`, `"now"`), `add_row` to
  this or another database (values may be `"this_row"` for a relation to the pressed row),
  `open_url` (client only, after the server actions). Max 5 actions.
- Left out of v1: notifications, mail, webhooks, Slack (they need delivery and abuse limits).

### 17.3 API

`POST /databases/{id}/rows/{row_id}/press/{property_id}` runs the server actions in one
transaction and returns `{row, created_rows, open_url?}`. It needs edit access to the row's
database and to each `add_row` target. A failed action rolls back all. Rate limit: 10 presses per
user per second.

### 17.4 Web and mobile

Web: the cell shows the button; a press shows the confirmation when set, then a spinner, then
the result. Mobile: the same button in the row's property list and in list rows. Button config:
"Edit on the web".

### 17.5 Other areas

Notion import: buttons are skipped (`property:button`), as today. Notion's help says the API
cannot create, configure or press buttons
([Notion: database buttons](https://www.notion.com/help/database-buttons)). Export: left out.
MCP: not exposed.

### 17.6 Tests

Server: each action; rollback when one action fails; access to an `add_row` target in another
space; `"me"` and `"today"`; rate limit.

## 18. Item 16: Form view (P3, members-only part M; public part out of scope)

### 18.1 Recommendation (D-P3-4)

Split the form view in two:

| Part | In this spec | Why |
|---|---|---|
| **Members-only form**: signed-in workspace members with read access to the database fill a form; each answer is a new row. | Yes | It reuses auth, access and the row create route. |
| **Public form** ("anyone on the web with the link", anonymous answers) | No. A separate design. | Orbit has no public page sharing today. It needs public tokens, rate limits, CAPTCHA, spam handling, file upload limits for strangers, a public CSP, data retention, and an abuse report flow. |

Notion offers three access levels and anonymous answers
([Notion: forms](https://www.notion.com/help/forms)).

### 18.2 Members-only form

- Config: `{"questions": [{"property_id", "label", "description", "required"}], "title",
  "description", "submit_label", "confirmation", "access": "members" | "closed"}`.
- Question types: title, text, number, select, multi-select, status, date, person, checkbox,
  url, email, phone, files. Not: computed types, relation (later), button.
- `POST /databases/{id}/forms/{view_id}/submit` `{answers}`: creates a row as the submitter
  (`created_by` is the person), with the row cap. A member with read but no edit access to the
  database can submit (the only write they get). This is a new `Permission` or `Actor` method in
  `crates/orbit/src/policy.rs`, with a `can_submit` flag on the view (`.ai/lessons.md`,
  Permissions). 30 submissions per user per hour.
- Web: the form view shows the builder to editors (question list, drag to reorder, "+ Add
  question" picks or creates a property). "Preview" and "Copy form link"
  (`/docs/<database>/form/<view>`), a full-page form for members.
- Mobile: the form link opens a native form screen (fill only). The builder: "Edit on the web".
- Notion import: form views become table views (`view_type:form`), as today.

### 18.3 Tests

Server: submit with read access only; required questions; closed form is 409; rate limit; row
cap. Web: required question blocks submit.

## 19. Limits (new)

| Item | Limit | Item # |
|---|---|---|
| Views in one linked view block | 20 | 1 |
| Filter depth / children per group / rules | 3 / 10 / 20 | 3 |
| Templates per database | 20 | 5 |
| CSV file / rows / columns / request body | 5 MB / 5,000 / 100 / 8 MB | 7 |
| Links per relation cell (writes) | 100 | 9 |
| Files per cell | 20 | 10 |
| Email / phone length | 320 / 50 | 10 |
| API query page / trash / values per write | 100 / 100 / 50 | 12 |
| Formula: length / nodes / depth / steps per cell / properties | 2,000 / 500 / 32 / 2,000 / 20 | 13 |
| Button actions / presses | 5 / 10 per second per user | 15 |
| Form submissions | 30 per user per hour | 16 |

## 20. Decisions to confirm

| # | Decision | Date |
|---|---|---|
| D-P2-1 | The database of an inline block is a normal sub-page of the host page and shows in the tree. Deleting the block does not trash it. Linked views store their own views (`database_links`). | 2026-11-09 |
| D-P2-2 | Filter groups nest 3 levels (root included), like task filters and Notion's UI. The migration rewrites old configs; no compatibility code. | 2026-11-09 |
| D-P2-3 | CSV: the client parses and maps; the server converts text with the P1 4.2 rules in one transaction. | 2026-11-09 |
| D-P2-4 | Board grouping adds `multi_select` together with `person` (the same rules: a card in many columns). | 2026-11-09 |
| D-P2-5 | Row templates are hidden child pages listed in `database_templates`, so their body uses the normal editor. | 2026-11-09 |
| D-P2-6 | Search indexes a `pages.property_text` column, computed in Rust on every write that changes it, backfilled by a job. Person names are not indexed. | 2026-11-09 |
| D-P3-1 | Rollups and formulas are computed on the server at read time and never stored. | 2026-11-09 |
| D-P3-2 | D2 stays: 5,000 rows, no typed value columns. The API filters and sorts in Rust in memory; shared JSON cases keep it equal to `@orbit/database`. | 2026-11-09 |
| D-P3-3 | Formulas run only in Rust on the server. Clients show results and ask for previews. | 2026-11-09 |
| D-P3-4 | The form view ships members-only. Public forms get their own design. | 2026-11-09 |
| D-P3-5 | Two-way relations store each link once, under the primary property; the synced property reads the reverse. | 2026-11-09 |
| D-P3-6 | Buttons v1 run only data actions (set values, add row, open URL); no notifications, mail or webhooks. | 2026-11-09 |

## 21. Sources

Notion help center and API reference, read on 2026-11-09:

- [Relations and rollups](https://www.notion.com/help/relations-and-rollups)
- [Formula syntax](https://www.notion.com/help/formula-syntax), [Formulas](https://www.notion.com/help/formulas), [Common formula errors](https://www.notion.com/help/common-formula-errors)
- [Views, filters and sorts](https://www.notion.com/help/views-filters-and-sorts)
- [Tables](https://www.notion.com/help/tables), [Boards](https://www.notion.com/help/boards), [Calendars](https://www.notion.com/help/calendars), [Timelines](https://www.notion.com/help/timelines), [Galleries](https://www.notion.com/help/galleries), [Forms](https://www.notion.com/help/forms)
- [Database templates](https://www.notion.com/help/database-templates)
- [Database properties](https://www.notion.com/help/database-properties), [Unique ID](https://www.notion.com/help/unique-id), [Database buttons](https://www.notion.com/help/database-buttons)
- [Import data into Notion (CSV)](https://www.notion.com/help/import-data-into-notion)
- [API: property object](https://developers.notion.com/reference/property-object), [API: filter](https://developers.notion.com/reference/post-database-query-filter), [API: page property values](https://developers.notion.com/reference/page-property-values)
