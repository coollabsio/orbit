# Docs databases, phase 1 (P1) design

Status: draft, 2026-11-09. Source: [issue #77](https://github.com/coollabsio/orbit/issues/77).

## 1. Goal

Give Docs a Notion-style database: a page that holds rows, where each row is a page with typed
properties. A table view sorts and filters the rows by these properties. P1 is the smallest
useful slice: a full-page database with saved table views.

Users today use plain tables. They must copy the same values again and again, and they cannot sort
or filter. The Notion import has the same problem: it makes a database into a static table and
sub-pages (`apps/server/src/notion/scan.rs:28`).

## 2. Scope

### In P1

- A new page kind, **database**. You create it from the docs tree "+" menu and from the slash
  menu ("Database – full page"). It shows in the tree with a table icon.
- **Rows are pages.** Each row has a title, an icon, property values and a normal BlockNote body.
  This gives co-editing, comments, history, trash and search with no new work.
- **Property types:** `title`, `text`, `number`, `select`, `multi_select`, `status`, `date`,
  `person`, `checkbox`, `url`, `created_time`, `created_by`, `last_edited_time`,
  `last_edited_by`.
- **Table view:** inline cell edits, add, rename, delete and reorder properties, change the
  property type, resize, hide and reorder columns, multi-column sort, a flat AND filter, search in
  the view, manual row order (drag) when the view has no sorts, multi-row select and delete,
  keyboard navigation.
- **Saved views:** more than one table view per database, shown as tabs. Each view has its own
  columns, sorts and filter.
- **Row page:** opens in a side peek from the table, or as a full page. A property list shows
  under the title.
- **Mobile:** a database screen with a row list, a view picker and the row page with editable
  properties.

### Not in P1 (later phases)

| Phase | Item |
|---|---|
| P2 | Inline database block, linked views, board view, calculation footer, AND/OR filter groups, row templates, Notion import as real databases, property values in search, CSV import and export, moving pages into and out of a database |
| P3 | Relation, rollup, formula, files, email, phone, unique ID, button; gallery, calendar, timeline and list views; MCP and token API tools for databases; form view |

## 3. Concepts

| Term | Meaning in Orbit |
|---|---|
| Database | A page with `kind = 'database'`. It has no BlockNote body in P1. It has properties, views and rows. |
| Row | A page whose parent is a database page. The row title is the `title` property. |
| Property | One typed column of a database. It has a stable id, so a rename does not break views. |
| View | A saved way to show the rows: type (`table` in P1), columns, sorts, filter. All members who can edit the database share the views. |

A database lives in a space (teamspace or private) like any page. Its rows inherit the space and
the access of the database page, as sub-pages do today.

## 4. Data model

Migration `0078_page_databases.sql`, and the same change in
`apps/server/migrations/postgres/`. Also update the places that `.ai/lessons.md` names:
`crates/platform/src/db/migrate.rs` (both runners), `SUPPORTED_SCHEMA_VERSION` in
`crates/platform/src/backup.rs`, the version in `crates/platform/tests/{database,backup}.rs` and
`apps/server/tests/app_smoke.rs`, and the route counts in `apps/server/tests/openapi_contract.rs`.

```sql
ALTER TABLE pages ADD COLUMN kind TEXT NOT NULL DEFAULT 'page'
    CHECK (kind IN ('page', 'database'));

CREATE TABLE database_properties (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    type TEXT NOT NULL,              -- one of the P1 types; checked in Rust
    config_json TEXT NOT NULL DEFAULT '{}',
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX database_properties_database ON database_properties (database_id, position, id);
CREATE INDEX database_properties_workspace ON database_properties (workspace_id);

CREATE TABLE database_views (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    type TEXT NOT NULL CHECK (type IN ('table')),
    config_json TEXT NOT NULL DEFAULT '{}',
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX database_views_database ON database_views (database_id, position, id);
CREATE INDEX database_views_workspace ON database_views (workspace_id);

CREATE TABLE page_property_values (
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    property_id TEXT NOT NULL REFERENCES database_properties(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    value_json TEXT NOT NULL,
    updated_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (page_id, property_id)
);
CREATE INDEX page_property_values_property ON page_property_values (property_id);
CREATE INDEX page_property_values_workspace ON page_property_values (workspace_id);
```

Rules:

- Add scope triggers like `page_files_scope_insert` (0025): a property, view or value must have
  the same `workspace_id` as its page. A value's page must be a row of the property's database.
  On Postgres, write the same checks as the triggers in the Postgres schema.
- The `title`, `created_*` and `last_edited_*` properties store no value. They read `pages.title`,
  `pages.creator_id`, `pages.created_at`, `pages.updated_by` and `pages.updated_at`.
- A missing value row means "empty". To clear a value, delete the row. Do not store `null`.
- Do not add sort or index columns for values in P1. P1 sorts and filters on the client (see
  section 6). Add typed columns in the phase that adds server-side queries.
- A database has exactly one `title` property. You cannot delete it or change its type.

### 4.1 Property config and value shapes

| Type | `config_json` | `value_json` | Empty |
|---|---|---|---|
| `title` | `{}` | (in `pages.title`) | `""` |
| `text` | `{}` | `"plain text"` (max 10,000 chars, no rich text in P1) | no row |
| `number` | `{"format":"number"\|"number_with_commas"\|"percent"\|"currency","currency":"USD"}` | `12.5` (finite f64) | no row |
| `select` | `{"options":[{"id","name","color"}]}` | `"<option id>"` | no row |
| `multi_select` | same as `select` | `["<option id>", …]` (max 100, no duplicates) | no row |
| `status` | `{"options":[{"id","name","color","group":"todo"\|"in_progress"\|"done"}]}` | `"<option id>"` | default option |
| `date` | `{"date_format":"relative"\|"short"\|"iso","time_format":"12h"\|"24h"}` | `{"start":"2026-11-09","end":null}` or with time `{"start":"2026-11-09T14:00:00Z","end":…}` | no row |
| `person` | `{}` | `["<user id>", …]` (members of the workspace) | no row |
| `checkbox` | `{}` | `true` (store only `true`; `false` = no row) | no row |
| `url` | `{}` | `"https://…"` (max 2,000 chars, http(s) only) | no row |
| `created_time`, `created_by`, `last_edited_time`, `last_edited_by` | `{}` | (read from `pages`) | never |

- Colors: `default`, `gray`, `brown`, `orange`, `yellow`, `green`, `blue`, `purple`, `pink`, `red`.
- Max 200 options for each property. The option order in config is the sort order.
- A new `status` property gets 3 options: "Not started" (todo), "In progress" (in_progress) and
  "Done" (done). A row with no value shows the first `todo` option.
- When you delete an option, the server removes its id from all values in the same transaction.
- The server parses and validates every value against the property type at the API boundary. It
  rejects bad input with 422 and the field path.

### 4.2 Type change

`PATCH` of `type` converts the values in one transaction. Conversions that lose data need a
confirmation in the UI ("Some values cannot be converted and will be cleared").

| From → to | Rule |
|---|---|
| any → `text` | the display text (option names joined with ", ", date as ISO, number as written) |
| `text` → `number` | parse; clear when it does not parse |
| `text` → `select`/`multi_select`/`status` | make options from the distinct values (split on "," for multi); max 200, then clear |
| `select` ↔ `multi_select` ↔ `status` | keep the options; multi → single keeps the first one |
| `text` → `url` | keep when it is a valid http(s) URL, else clear |
| `text` → `checkbox` | `true` for "true", "yes", "1", "x" (any case); else clear |
| `text` → `date` | parse ISO 8601; else clear |
| to/from `person`, and to a computed type | clear all values |

### 4.3 View config

```json
{
  "columns": [{ "property_id": "…", "width": 200, "hidden": false }],
  "sorts": [{ "property_id": "…", "direction": "asc" }],
  "filter": { "rules": [{ "property_id": "…", "operator": "contains", "value": "abc" }] },
  "wrap_cells": false
}
```

- `columns` holds the order of all properties. When a property is added, the server adds it to
  the end of every view, visible. A property that is not in `columns` shows at the end (the client
  repairs it on the next save).
- Width: 32–1,000 px, default 200 (title 280).
- Max 10 sorts and 20 filter rules for each view. All rules must match (AND). AND/OR groups are P2.
- A rule or sort that names a deleted property is dropped when the view loads and when it saves.

Filter operators:

| Types | Operators |
|---|---|
| `title`, `text`, `url` | `equals`, `not_equals`, `contains`, `not_contains`, `starts_with`, `ends_with`, `is_empty`, `is_not_empty` |
| `number` | `=`, `!=`, `>`, `<`, `>=`, `<=`, `is_empty`, `is_not_empty` |
| `select`, `status` | `is`, `is_not` (value: list of option ids, any match), `is_empty`, `is_not_empty` |
| `multi_select`, `person` | `contains`, `not_contains` (value: list of ids, any match), `is_empty`, `is_not_empty`; `person` also allows the value `"me"` |
| `checkbox` | `is` (`true`/`false`) |
| `date`, `created_time`, `last_edited_time` | `is`, `before`, `after`, `on_or_before`, `on_or_after`, `is_empty`, `is_not_empty`; value is an absolute date or `today`, `tomorrow`, `yesterday`, `one_week_ago`, `one_week_from_now`, `one_month_ago`, `one_month_from_now` |
| `created_by`, `last_edited_by` | `is`, `is_not` (list of user ids, or `"me"`) |

Text matching ignores case (Unicode lowercase). A rule with no value (for example, `contains` "")
matches all rows, as in Notion, so a half-made filter does not hide everything.

## 5. API

All paths start with `/api/v1/workspaces/{workspace_id}`. `{database_id}` is the page id of the
database. Access is the same as for the database page: who can read the page can read the
database, who can edit the page can write. Errors use the existing problem format. Add every
route to the OpenAPI document and run `just api`, so `@orbit/api` gets the generated client.

### 5.1 Changes to existing page routes

| Route | Change |
|---|---|
| `POST /pages` | New field `kind: "page" \| "database"` (default `page`). A database starts with a `title` property "Name", a `multi_select` property "Tags", one table view "Table" and no rows. |
| `GET /pages` (tree) | Each summary has `kind`. Rows (pages whose parent is a database) are **not** in the list, so a big database does not fill the tree or slow it down. |
| `GET /pages/{id}` | Has `kind`. For a row, also `database_id`. |
| `POST /pages/{id}/move` | 409 `database_move_unsupported` when you move a row out of a database, or a page into a database (P2 allows it). You can still move a database page itself. |
| `POST /pages/{id}/duplicate` | A database copies its properties, views (property ids rewritten), rows and values with new ids. |
| Trash, restore, permanent delete | No change. Rows are sub-pages, so they go to the trash with the database and come back with it. A row trashed on its own comes back into its database. Values go with the page on permanent delete (cascade). |
| `GET/WS /pages/{id}/collab`, versions | A database page has no body: the collab endpoint returns 409 for `kind = 'database'`. Rows work as today. |
| `GET /pages/search` | No change: row titles and bodies are pages, so they are found. A hit on a row shows "Row · <database title>". |

### 5.2 New routes

| Method and path | Body | Result |
|---|---|---|
| `GET /databases/{database_id}` | — | The whole database (below) |
| `POST /databases/{database_id}/properties` | `{name, type, config?, position?}` | The property; it is added to all views |
| `PATCH /databases/{database_id}/properties/{property_id}` | `{name?, type?, config?, position?}`; `type` and `config` cannot be in the same request (422) | `{property, converted, cleared}` |
| `DELETE /databases/{database_id}/properties/{property_id}` | — | 204; its values go too; views drop it |
| `POST /databases/{database_id}/views` | `{name, type: "table", config?, duplicate_of?}` | The view |
| `PATCH /databases/{database_id}/views/{view_id}` | `{name?, config?, position?}` | The view |
| `DELETE /databases/{database_id}/views/{view_id}` | — | 204; 409 for the last view |
| `POST /databases/{database_id}/rows` | `{title?, icon?, values?, after_row_id?}`; `after_row_id` omitted = last, `null` = first; `values` cannot hold the title or a computed property | The new row |
| `PATCH /databases/{database_id}/rows/{row_id}/values` | `{"<property_id>": value \| null, …}` | The row |
| `POST /databases/{database_id}/rows/trash` | `{row_ids: [...]}` (max 500) | 204; one trash batch for each row |
| `POST /databases/{database_id}/rows/{row_id}/move` | `{after_row_id: id \| null}` | The row (manual order, sets `pages.position`) |

`GET /databases/{database_id}` returns:

```json
{
  "database": { "id": "…", "title": "Research", "icon": "📚", "locked": false, "row_count": 42 },
  "properties": [{ "id": "…", "name": "Name", "type": "title", "config": {}, "position": 0 }],
  "views": [{ "id": "…", "name": "Table", "type": "table", "config": { … }, "position": 0 }],
  "rows": [{
    "id": "…", "title": "Paper A", "icon": null, "position": 0, "version": 0,
    "created_at": "…", "created_by": "…", "updated_at": "…", "updated_by": "…",
    "has_content": true,
    "values": { "<property_id>": ["opt_1", "opt_2"] }
  }]
}
```

`has_content` tells the table to show the "page has text" icon next to the title.

Rules:

- **Row cap:** 5,000 live rows for each database. `POST …/rows` returns 409 `database_row_limit`
  at the cap. Also max 100 properties and 50 views for each database.
- **Writes** use `immediate_transaction()`. Validate and convert before the transaction where
  possible (see `.ai/lessons.md`, "Write transactions").
- **Validation errors:** 422 `validation_failed` with the field path in `detail` (for example
  `values.<property id>` or `config.filter.rules[0].value`).
- **Row positions** are `0..n` without gaps (the page `renumber`). A row added first rewrites the
  positions of the other rows in the same transaction.
- **Conflicts:** a cell write is last-write-wins. It needs no version, as a Notion cell does. A
  value write updates the row's `pages.updated_at` and `updated_by` (so `last_edited_*` work) but
  not `pages.version`. Property and view writes are also last-write-wins.
- **Lock:** the page lock (`/pages/{id}/lock`) on a database page locks its properties and views
  (423 `page_locked`, as for pages). Members can still add rows and edit values, as in Notion's "Lock database".
- **Realtime:** every write records an audit event and an outbox event like a page metadata
  `PATCH` (resource type `database`, with the database id). Clients refetch the database query
  on that event. Cell edits are frequent: during implementation, check how page title edits
  limit audit rows and do the same.
- **Read-only workspace** (trial stop, read-only mode): all new write routes refuse as page
  writes do.

## 6. Shared client logic: `packages/database` (`@orbit/database`)

A new pure TypeScript package, used by web and mobile. It has no React and no network code.

- `types.ts`: property, view, row and value types, derived from the generated API types where
  they exist.
- `values.ts`: parse a value from the API; the display text of a value; a value from typed text
  (paste).
- `filter.ts`: `matches(row, rule, property, ctx)`; `ctx` has the current user id, "now", the time
  zone and the member names (for `me`, relative dates and person names). Rules for unclear cases:
  a date range counts by its start for `before`/`after`/`on_or_*`, and `is` matches a day inside
  the range; a date with a time counts as its day in the viewer's time zone; an empty number
  matches only `!=`; empty text matches `not_equals` and `not_contains`; a status with no value is
  its first `todo` option, so it is never empty.
- `sort.ts`: `sortRows(rows, sorts, properties, ctx)`; it computes each row's sort keys once. Empty values go last in both directions. Select
  and status sort by option order, person by name, checkbox `false` before `true`. Ties fall back
  to manual order (`position`, then `id`).
- `view.ts`: `applyView(database, view, search, ctx) → rows` (filter, then search over the title
  and the text of the visible properties, then sort), and `repairConfig(view, properties)`.

Unit tests (`bun test`) cover each operator, the empty rules, sort ties and the repair. This is
the logic that can break silently.

Client sort and filter is enough for 5,000 rows. Measured (`bun run bench`, 5,000 rows × 20
properties, 3 sorts, 5 rules, desktop): 8 ms without search, 27 ms for a search with no hits. Phones
are slower; cache each row's search text if search feels slow on mobile.

Display rules: `percent` shows the value × 100 (0.125 → "12.5%"), as in Notion. The `relative` date
format shows Today, Tomorrow or Yesterday, else the short date. Paste matches existing option and
member names only (case ignored); it never creates options.

## 7. Web UI

Files: `apps/web/src/features/docs/database/` (`api.ts`, `DatabasePage.tsx`, `ViewTabs.tsx`,
`ViewToolbar.tsx`, `FilterBar.tsx`, `SortPopover.tsx`, `DatabaseTable.tsx`, `ColumnHeaderMenu.tsx`,
`cells/*`, `PropertyList.tsx`, `RowPeek.tsx`). `DocsPage` renders `DatabasePage` when
`kind = 'database'`, instead of the BlockNote editor. Follow `.ai/lessons.md` → Design fidelity:
shadcn-style code, inline Tailwind, `cva` for variants, reicon outline icons, flat surfaces with
1px borders (no cards around the table).

### 7.1 Database page layout

```
┌───────────────────────────────────────────────────────────────────────────────┐
│ [cover]                                                                       │
│ 📚 Research                                         (title + icon, as a page) │
│                                                                               │
│ [▦ Table] [▦ To read] [+]                      [Filter] [Sort] [🔍] [⋯] [New ▾]│
│ ─────────────────────────────────────────────────────────────────────────────  │
│ [Status: Reading ✕] [Tags: AI ✕] [+ Add filter]       (only when rules exist)  │
│ ┌──────────────────────┬──────────────┬─────────────┬───────────┬───┐          │
│ │ Aa Name              │ ◎ Status     │ ≡ Tags      │ 📅 Added  │ + │          │
│ ├──────────────────────┼──────────────┼─────────────┼───────────┼───┤          │
│ │ Paper A        [Open]│ (● Reading)  │ (AI)(Rust)  │ Nov 9     │   │          │
│ │ Paper B              │ (● Done)     │ (Docs)      │ Nov 2     │   │          │
│ │ + New                                                                        │
│ └──────────────────────────────────────────────────────────────────────────    │
└───────────────────────────────────────────────────────────────────────────────┘
```

- **Title area:** the same cover, icon and title controls as a normal page. The table uses the
  full page width when the page is full width (`full_width` option). Otherwise the table can
  scroll sideways past the text column, as in Notion.
- **View tabs:** each tab shows the view icon and name. Click selects. Right-click or "⋯" gives
  Rename, Duplicate, Delete. "+" adds a table view. The selected view is stored per user in
  `localStorage` (`orbit:database-view:<database_id>`), and the URL holds it (`?view=<id>`).
- **Toolbar:** Filter and Sort show a count when they are active (pink text). Search opens an
  inline field that filters the visible rows only for this user (it is not saved). "⋯" opens
  Properties (show, hide, reorder), Wrap cells, Lock database, Copy link to view. "New" adds a
  row at the top of the view and opens it in the side peek; the "▾" part is for templates (P2,
  hidden in P1).
- **Filter bar:** one chip for each rule, `<property>: <value summary>`. Click opens a popover
  with the operator select and the value editor for the type. "+ Add filter" picks a property.
  The bar shows only when the view has rules.
- **Sort popover:** a list of sorts, each with property, direction, drag handle and remove. "+ Add
  sort".
- **Shared views:** a view change is saved for all at once (debounce 500 ms), as in Notion.
  Members without edit access, and editors of a locked database, can still change the filter and
  sort; the change stays local, and a bar shows "Changes are only visible to you · Reset".

### 7.2 Table

- **Grid:** `role="grid"`, `aria-rowcount`, `aria-sort` on sorted headers. Rows use TanStack
  Virtual (put `data-virtual-scroller` on the scroller, for tests). Row height 33 px, header
  33 px, cell padding 8 px, text 14 px. Thin `border-border` lines between cells and rows; no
  zebra stripes; the first column has no left border. A sticky header.
- **Header cell:** type icon (16 px, muted) and name. Click opens the header menu: name field,
  Edit property (type and config), Sort ascending, Sort descending, Filter, Hide, Insert left,
  Insert right, Duplicate, Delete (not for the title). Drag the header to reorder; drag the right
  edge to resize.
- **"+" column:** adds a property. A popover asks for the name and the type (list with icons).
- **Row hover:** a drag handle and a checkbox show to the left of the row. An "Open" button
  shows at the right end of the title cell.
- **Row select:** the checkboxes select rows. A bar then shows "N selected · Delete · Clear".
  Delete moves the rows to the trash (with undo in the toast, through restore).
- **"+ New" row** under the last row adds a row with the values that the active filter needs
  (for example, the filter `Status is Reading` gives the new row the status "Reading"), as Notion
  does. A new row that does not match the view stays visible until the next refresh, so it does
  not disappear under the cursor.
- **Drag to reorder rows:** only when the view has no sorts. With sorts, the handle shows a
  tooltip "Remove sorts to reorder rows".
- **Empty states:** no rows → "No rows yet" and "+ New". No rows match → "No results" and
  "Clear filters".

### 7.3 Cells

| Type | Display | Edit |
|---|---|---|
| `title` | text, row icon, a page icon when `has_content` | inline text field; Enter saves |
| `text` | one line (all lines with wrap cells) | a popover with a multi-line field |
| `number` | right-aligned, formatted (`1,234.5`, `12%`, `$12.00`) | inline field |
| `select`, `status` | a colored pill (status pill has a dot) | a popover: search, list of options, "Create <name>", "⋯" per option (rename, color, delete) |
| `multi_select` | pills, cut off with "+N" when there is no space | the same popover, more than one choice |
| `date` | as the config says (`Nov 9, 2026`, `Today`); a range as `start → end` | a popover with a calendar, "End date" and "Include time" switches, "Clear" |
| `person` | avatars and names | a member picker (the shared `AssigneePicker` pattern) |
| `checkbox` | a checkbox, centered | click or Space toggles |
| `url` | a link (the host and path), opens in a new tab | inline field |
| `created_*`, `last_edited_*` | time or person, muted | not editable |

Pill colors: one `cva` variant for each of the 10 colors, with a light and a dark value. Use a soft
background and stronger text, as in Notion (for example, blue: `bg-blue-500/15 text-blue-700
dark:text-blue-300`).

### 7.4 Keyboard

| Key | Action |
|---|---|
| Arrow keys | move the active cell |
| Enter | edit the active cell; in a title cell in edit mode, save and go down |
| Esc | stop the edit without save; a second Esc clears the active cell |
| Tab / Shift+Tab | next / previous cell |
| Space | toggle a checkbox; on a row with the cell not in edit mode, open the side peek |
| Delete / Backspace | clear the cell (not computed types) |
| Cmd/Ctrl+C, Cmd/Ctrl+V | copy the cell display text; paste text into a cell (parsed by `values.ts`) |

Range select and paste of many cells is P2.

### 7.5 Row page and side peek

- "Open" or a click on the title opens the **side peek**: a right pane like the history pane
  (`PageHistoryPane`), about 50% of the width, full screen on phones. The URL gets `?row=<id>`, so
  Back closes it and you can share the link. The peek header has: open as full page, copy link,
  previous and next row (in the view order), "⋯" (Delete, Duplicate), close.
- A full row page (`/docs/<row id>`) shows the breadcrumb `<database> › <row>`.
- Both show, under the title, the **property list**: one line for each property, with the type
  icon and name (muted, 160 px) on the left and the cell editor on the right. Empty values show
  "Empty" (muted). The hidden properties of the current view show too. "+ Add a property" at the
  end adds a property to the database.
- Under the property list, a 1px line, then the comments and the BlockNote body, as on a page
  today.

### 7.6 Create and tree

- The docs tree "+" menu and the page "⋯" menu get "New database". The slash menu gets
  "Database – full page": it makes a database as a sub-page and puts a `page` link block in the
  current page (the inline block is P2).
- The tree shows a database with a table icon (or its own icon) and no expand arrow, because rows
  are not in the tree.

## 8. Mobile (`apps/mobile`)

Port the same data and logic (`@orbit/database`, the same API client). Mobile has no wide table,
so it shows a list.

- `src/app/(tabs)/docs/[id].tsx` shows a database screen when `kind = 'database'`: the title,
  a view picker (sheet), a search field, and a list of rows. Each row shows the title and the
  first 3 visible properties of the view as small chips and pills.
- A filter and sort sheet changes the view, with the same "only visible to you" rule as on the web.
- A tap opens the row page: the property list (each value opens a native sheet editor), then the
  BlockNote body as today (`PageEditor.dom.tsx`).
- "+" adds a row.
- **Not on mobile in P1:** add, rename, delete or change the type of properties; reorder or
  resize columns; drag to reorder rows; multi-row select. The screen shows these as "Edit on the
  web" where a user looks for them. Reason: these edits need a wide table to be clear. They can
  come later.

## 9. Other areas

| Area | P1 behavior |
|---|---|
| Page export (`pageExport.ts`, `repositories/page_export.rs`) | A database exports as a Markdown table of its first view (visible columns, view order). Rows export as sub-pages; each row file starts with a property list. |
| Workspace export (`workspace_export/`) | The same rules as the page export. Check that the archive code sees the new tables. |
| Backups | The new tables are in the database snapshot. Update `SUPPORTED_SCHEMA_VERSION`. |
| Version history | Rows keep the page history for the title and body. Property values are not in history in P1. |
| MCP and token API | No new tools in P1. The existing page tools see rows as pages. A database page reads as its title with no body. |
| Notion import | No change in P1. P2 maps Notion data sources to databases. |
| Comments, mentions, favorites, recent | Work on rows and on the database page as on pages. |

## 10. Build order

Each step ends in a state you can check.

1. **Schema and API** (migration, repository `repositories/databases.rs`, routes
   `database_routes.rs`, page route changes, OpenAPI, `just api`). Check: integration tests
   `apps/server/tests/databases.rs` on SQLite and Postgres.
2. **`@orbit/database`** (types, values, filter, sort, view). Check: unit tests and the 5,000-row
   time.
3. **Web table** (database page, tabs, toolbar, filter, sort, table, cells, keyboard). Check: the
   worktree dev env in a browser.
4. **Web row page and side peek** (property list, peek, breadcrumb, create menus, tree icon).
   Check: the dev env.
5. **Export** (page and workspace export). Check: the export goldens.
6. **Mobile** (database screen, row list, sheets, row properties). Check: `bunx tsc --noEmit -p .`
   and `bun test src` in `apps/mobile`, and the iOS bundle on the running Metro server.

## 11. Tests

| Area | What to prove |
|---|---|
| Server (`apps/server/tests/databases.rs`, both databases) | create a database with defaults; value validation for each type (good and bad); type conversion table; delete option removes it from values; delete property cleans views and values; row cap; lock blocks schema and view writes but not values; rows are not in `GET /pages`; move into or out of a database is refused; duplicate copies schema, views, rows and values with new ids; trash and restore of a database brings back its rows and values; access: a member who cannot see a private database gets 404 on every route |
| `@orbit/database` | each filter operator, including empty and `me` and relative dates; sort order of empty values and ties; search; repair of a view config |
| Web | a cell edit sends one `PATCH` and shows the saved value; a new row under a filter gets the filter values. No markup or snapshot tests. |

## 12. Limits

| Item | Limit |
|---|---|
| Rows in a database | 5,000 |
| Properties in a database | 100 |
| Views in a database | 50 |
| Options in a select, multi-select or status property | 200 |
| Values in a multi-select or person cell | 100 |
| Sorts / filter rules in a view | 10 / 20 |
| Text value | 10,000 characters |
| URL value | 2,000 characters |
| Rows in one bulk trash | 500 |

## 13. Decisions

| # | Decision | Date |
|---|---|---|
| D1 | View changes (columns, sorts, filter) are shared with all editors at once, with no "Save view" step, as in Notion. Orbit task views use "Save view"; databases behave differently on purpose. Members who cannot edit see their changes only locally (section 7.1). | 2026-11-09 |
| D2 | P1 caps a database at 5,000 live rows; sort and filter run on the client. Server-side queries come with the MCP tools (P3). | 2026-11-09 |
| D3 | A database page has no body (no description above the table) in P1. | 2026-11-09 |
| D4 | A `person` property holds Orbit workspace members only, no free text, as in Notion. | 2026-11-09 |
| D5 | The slash menu item is "Database – full page": it creates a database as a sub-page and inserts a `page` link block. The inline database block is P2. | 2026-11-09 |
