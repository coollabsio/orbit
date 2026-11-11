# Feature behavior

## Persistence status

Milestone one persists setup, login and recovery, sessions, workspaces, memberships, invitations, projects, statuses, labels, tasks, comments, attachments, trash, and audit records. Those flows use the generated `/api/v1` client and never substitute mock records after an error.

Docs pages persist too (tree, content, trash, search; see Docs below).

Chat persists too (channels, direct messages, threads, files, search; see Chat below).

Mail persists too (receiving and a read-only web client: domains, mailboxes, threads, search; see Mail below). Sending mail is a follow-up milestone.

## Global shell

- First sidebar groups Workspace, Personal, and Manage navigation.
- It collapses to a 56px icon rail; state persists in `orbit:sidebar_collapsed`.
- Search opens the command palette. `Cmd/Ctrl+K` toggles it.
- User menu provides Account settings and a link to Account settings → Appearance (`/profile/appearance`).
- Mobile uses a bottom dock and a full navigation drawer.
- Topbar breadcrumbs are route-derived in `Topbar.tsx`.
- Appearance (port of Coolify's Profile → Appearance, `lib/theme.tsx`): the sidebar is contextual (settings areas, the docs and chat lists in the sidebar, Back), and the custom theme colours the whole app, with the sidebar, its menus (`MenuSurfaceContext`) and ⌘K. Color theme Light/System/Dark (default dark, `localStorage.theme`), the custom theme (`lib/customTheme.ts`, `CustomThemeDialog`; replaces Light/System/Dark while set: `ThemeProvider` puts `customThemeVariables` on `<html>` and picks `.dark` from the message colour; picking a base theme clears it; the app sidebar uses `appSidebarTheme`, which puts `.dark` or `.light` on the sidebar so its `dark:` styles follow the sidebar colour), Navigation style Default/Floating (`orbit:nav-style`, `[data-floating]` on `<html>`, `floating:` variant, desktop only) and Style 3D/Flat (`orbit:style`, `[data-flat]`, `flat:` variant on buttons and sidebar items), Font (`orbit:font`, `[data-font]` sets `--font-ui`, which `--font-sans` reads; Noto Sans default, Geist, Inter, IBM Plex Sans, System, Literata, Mono = Geist Mono; self-hosted `@fontsource-variable/*`, the browser downloads a face only when text uses it) and Text size Small/Compact/Default/Large/Extra large (`orbit:text-size`, `[data-text-size]` sets the root font size to 87.5/93.75/100/112.5/125%, so every rem size, text and spacing, follows; px sizes do not). `index.html` applies all of them before first paint. Coolify's Page width and Chart times are not ported.

## Home

`features/home/HomePage.tsx`

- Greets the current user and summarizes assigned tasks, unread mail/chat, and notifications.
- Shows My tasks, Inbox, Recent mail, and Active channels cards.
- Content has extra top breathing room and a restrained staggered entrance.
- The Home topbar contains search, theme, and settings actions only; the global New menu is intentionally omitted.

## Tasks

Core files: `TasksPage.tsx`, `components/TaskList.tsx`, `components/TaskBoard.tsx`, `components/TaskDetail.tsx`, `ProjectSettingsPage.tsx`.

- Project rail: color square, project name, hover settings gear.
- Sidebar project order is per member (`project_positions`, migration 0099, `GET`/`PUT /project-order`): drag a project row or press Alt+Arrow on it (web), long-press → Move up / Move down (mobile drawer). Projects not placed yet follow by name; the API project list stays by name.
- The task-list title is a project picker: All tasks or any project can be selected without opening the project rail.
- List and Kanban layouts share filtering and sorting.
- Sorting: manual, priority, created, updated, title.
- Board cards support cross-status and within-column drag placement with a visible placeholder.
- Inline status and priority changes avoid opening task detail.
- Multi-select displays a bottom bulk toolbar for status, priority, assignee, and labels.
- New task creates an empty record and opens task detail with title autofocus; no creation modal.
- Task detail supports title, description, status, priority, assignees, labels, project, due date, activity, comments, replies, and attachments.
- Rich text (migration 0052): the description and comments use the Docs BlockNote editor in a `task` profile
  (`PageEditor` `profile="task"`: no page blocks, no comments; slash menu with Callout and Mermaid; @mentions).
  `tasks.description_json` / `task_comments.body_json` hold the blocks; `description` / `body` keep a markdown copy
  (search, MCP, GitHub, API clients that send markdown, which the server converts: `collab/markdown_import.rs`).
  NULL JSON is a row written before rich text; it is converted (and stored) when first opened or edited. A task created
  with markdown (API, MCP) is converted at once, so its mentions notify. Task lists
  leave `description_json` out (null); `GET /tasks/{id}` and write responses carry it.
- The description is co-edited live like a docs page (`GET /tasks/{id}/collab`, `task_collab_docs` /
  `task_collab_updates`, the same hub keyed by `DocKey { Page | Task }`): cursors, presence avatars, a read-only
  preview until the first sync. Edits project into the task without a version bump; REST and GitHub writes replace the
  live document. A GitHub-linked task is locked. Newly mentioned members get `description_mentioned` (again after 10
  minutes, not while a mention is removed and added back).
- Files pasted, dropped or picked into the description or a comment become image/file blocks; they are task
  attachments with `inline = 1`, left out of the attachment list (which shows only older attachments). Images and files
  pasted into a task's description or comments from another task or a docs page are copied into the task (download,
  inline upload, the block points at the copy), so they stay when the source goes to the trash. The new task dialog has
  no task to upload to: images of other tasks stay as links, docs page images are dropped. The server accepts any task
  attachment URL of the workspace (API writes). Comments mount their editors only near the screen (the markdown copy
  shows until then), and reply fields when first used. The new task
  dialog keeps file chips and appends the files to the description after create.
- Comments: Cmd/Ctrl+Enter sends; edits use the same editor (Cmd/Ctrl+Enter saves, Esc cancels; Save on an emptied
  comment asks to delete it; an unchanged Save sends nothing); the server reads mentions from the mention nodes and
  notifies only newly mentioned members on edit.
- Projects, workflows, labels, tasks, comments, and task attachments persist in SQLite.
- Labels belong to one project (Project settings → Labels); a task takes only its project's labels. Moving a task keeps the labels whose name the target project also has. Views across projects merge same-named labels: label filters store the label key (the name) and the server matches it case-insensitively in every project (an id saved before migration 0051 stands for its name and becomes a key when the filter is edited); group-by-label, filter pickers and bulk label menus show one entry per name. Views across projects show no labels of trashed projects.
- Writes include the observed record version. Stale writes restore the server record and expose a conflict prompt.
- Bulk and reorder requests are one atomic request of at most 100 items. Empty unchanged choices are local no-ops.
- Task and project trash have server-backed restore flows and 30-day retention.
- Asana import (`pages/AsanaImportPage.tsx`, `api/asanaImports.ts`; server `asana/`, routes `/imports/asana`, migration
  0069): "Import from Asana" under "New project" in the Tasks sidebar (and in the header project menu) opens
  `/tasks-import`. Same states and token rules as the Notion import (scan, choose, import; token encrypted, wiped in every
  final state; private to its creator). Choose lists projects by workspace and team (archived ones unselected). Each
  project becomes a new Orbit project. Sections become statuses: a status with the same name is reused, otherwise
  done-like names are `completed`, the first section `unstarted`, the rest `started`. Completed Asana tasks outside a
  completed status go to the project's Done. Tasks keep rich-text notes, dates (dates at 12:00 UTC), priority (custom field
  "Priority"), tags as labels, assignee matched by email (else a description line), other custom fields, linked files (as
  links), subtasks (five levels), comments (by the importer, prefixed with the Asana author and time), Asana-hosted files
  and dependencies (blocked by). Tasks are created with `TaskRepository::import_task`: no notifications, no sub-issue
  auto-close, Asana's `completed_at`. A restart resumes from `asana_import_items` without duplicates. A failed or cancelled
  import keeps what it made.
- Linear import (`pages/LinearImportPage.tsx`, `api/linearImports.ts`; server `linear/`, routes `/imports/linear`,
  migration 0070): "Import from Linear" in the header project menu and in Settings → Import opens `/tasks-import/linear`.
  Same states and key rules as the Asana import (personal API key encrypted, wiped in every final state; private to its
  creator). Choose lists teams with issue counts (archived ones unselected). Each team becomes a new Orbit project keyed
  by the team key. Workflow states become statuses: the same name is reused (Canceled matches Cancelled), otherwise
  triage/backlog/unstarted → `unstarted`, started → `started`, completed → `completed`, canceled → `cancelled`; Linear's
  "Duplicate" state lands on Cancelled until its duplicate relation moves it. Issues keep the markdown description, title,
  priority (0-4 → none/urgent/high/medium/low), labels (`Group/Name` for grouped labels), assignee matched by email (else a
  description line), due date (12:00 UTC), completed or canceled time, URL as source, sub-issues (five levels; deeper ones
  and ones whose parent is in another team go to the top level with a "Parent" line). Linear project, cycle, milestone,
  estimate and SLA become description lines; linked attachments a Links section. Uploads (`uploads.linear.app`) in the
  description and comments are downloaded with the key, attached, and their URLs rewritten. Comments are written by the
  importer with author and time, replies flattened in time order. Relations: blocks, related and duplicate (similar is
  skipped); a relation Orbit cannot hold (one per pair of tasks, one duplicate per task, no chains) is checked before the
  write, skipped and counted. Trashed issues are left out. Shared importer code: server `task_import.rs`, web
  `api/importStatus.ts` and `components/ImportLayout.tsx`.
- Trello import (`pages/TrelloImportPage.tsx`, `api/trelloImports.ts`; server `trello/`, routes `/imports/trello`,
  migration 0071): "Import from Trello" in the header project menu and in Settings → Import opens `/tasks-import/trello`.
  Same states and rules as the Asana and Linear imports. Connect takes a Power-Up API key and a read-only token (the page
  links Trello's authorization page for the typed key); both are encrypted and wiped in every final state, and a key or
  token Trello rejects shows Trello's reason under its field. Choose lists boards by Trello Workspace with open card counts
  (archived boards unselected). Each board becomes a new Orbit project. Open lists become statuses in board order (same
  name reused; a done-like name is completed, the first list unstarted, the others started); archived lists and archived
  cards are skipped and counted. Open cards keep title, markdown description, card URL as source, labels (unnamed ones
  named after their color, Trello colors kept), start and due dates, and list and card order; a card on a completed list
  gets its last activity as completed time. A card member is assigned only when exactly one active Orbit member has the
  same full name (Trello hides other members' emails); otherwise a "Members" line. Checklists become task lists
  (checklist blocks) in the description, not sub-issues. Custom field values and "Due date: marked complete" on an open
  list become description lines; link attachments a Links section. Uploaded attachments and uploads linked from the
  description and comments are downloaded, attached, and their URLs rewritten; a download Trello refuses is counted.
  Comments are written by the importer, oldest first, with author and time.
- Orbit import (server `orbit_import/` with `client.rs`, `archive.rs`, `tasks.rs`, `pages.rs`, `chat.rs`; routes
  `orbit_import_routes.rs` under `/imports/orbit` (`POST {url, token}`, `POST /upload`, `GET`, `GET /{id}?include=tree`,
  `POST /{id}/start {selection}`, `POST /{id}/cancel`); migration 0085: `orbit_imports`, `orbit_import_items`,
  `orbit_import_links`): copies another Orbit workspace from its export archive, pulled with an export API token
  (`orbit.fetch` job: `/me`, `POST /exports` once, poll up to 24 h, download to storage, read the manifest and tree)
  or uploaded as a ZIP. `mode` comes from the token's scope or the manifest: `full` (owner/admin of the target;
  teamspaces matched by name, projects with statuses and labels, tasks parents first via `import_task` with creator
  and assignees by email, attachments, comments, relations, teamspace pages, the exporter's private pages into the
  importer's private space, public channels matched by name, private channels, DMs) or `personal` (any non-guest;
  only private pages and private channels/DMs, `project_ids`/`teamspace_ids` refused; any import needs
  `WorkspaceExportPersonal`, a full one also `WorkspaceExport`). Every created record is in `orbit_import_links`
  keyed by the source workspace id and a `scope` (`''` for full imports, the importer's user id for personal ones):
  a re-run in the same scope creates nothing twice; pre-existing pages keep their content and only gain new
  comments. The archive is under the importer's control, so a personal import writes only its own scope, skips
  (counts as existing) a channel or page a full import created, and writes every message and comment as the importer
  with a name/time header, keeping only the importer's reactions. Consequence: a private channel or DM imported by a
  member before the admin's full import, or by two members, exists once per scope. Chat history is inserted directly
  (`chat.rs`): no notifications, mention rows, push, bots, webhooks or live events; read markers end at the last
  message; unmatched authors get a name/time header. Report: `created` counts, `skipped_existing`, `unmatched_users`,
  `failures`, `members_must_import_private`. Problem codes: `orbit_url_invalid`, `orbit_token_invalid`,
  `orbit_scope_missing`, `orbit_unreachable`, `orbit_export_too_large`, `orbit_format_unsupported`,
  `orbit_import_state_conflict`, `forbidden`. Source side: public `POST /api/public/v1/exports` reuses only a queued or
  running export. Dev: `ORBIT_DEV_ORBIT_IMPORT_LOCAL=1` allows `http://127.0.0.1:<port>` as the source.

### Sub-issues

Core files: `apps/server/src/repositories/sub_issues.rs` (parent checks, tree walks, auto-close), `features/views/taskTree.ts`,
`components/TreeGutter.tsx`, `components/TaskSubIssues.tsx`, `useParentActions.ts`, `views/nestDrop.ts` + `useNestDrop.ts`.

- `tasks.parent_task_id` (migration 0034): unlimited depth, parent may be in another project of the same workspace. Self-parent
  or a cycle → 422 `parent_cycle`; missing/trashed parent → 422 `parent_invalid`.
- Task JSON: `parent`, `sub_issue_count`, `sub_issue_closed_count` (closed = completed, cancelled, duplicate); `ancestors` on
  GET one; `trashed_descendant_count` in trash.
- Display `sub_issues`: nested (default, list only) / flat / hidden. Nested subtrees stay in their root's group; a match
  whose parent is filtered out shows as a root with a "Parent ›" prefix. Filters: Parent (task or "No parent"), Sub-issues.
- Drag onto the middle of a row or card nests it (toast + Undo); in nested mode an edge drop makes a sibling (beside a root
  row = detach). BulkBar has Set parent… / Remove parent.
- Per-project automations, default on: close the parent when every sub-issue is closed and one is completed; close open
  sub-issues when the parent closes (same category). They run in the write transaction for PATCH, bulk, delete and GitHub
  sync; PATCH/bulk responses carry `auto_closed` for the toasts.
- Deleting a task trashes its subtree; restoring it restores what was trashed with it. A task restored alone under a trashed
  parent comes back top-level.

### Project workflows

- Statuses are per-project entities, not a fixed enum.
- Each status has category, name, description, color, and order.
- Categories are Unstarted, Started, Completed, Cancelled.
- Project settings can add, edit, delete, and reorder statuses inside a category.
- General project settings use a local draft and bottom unsaved-changes bar.
- Access (#37, migration 0093): a project is Workspace (every non-guest member) or Private (members listed in `project_members`, plus workspace owners and admins, who are not listed). Project settings → Access switches it and adds or removes people (anyone who sees the project may; creating or making it private lists the actor). The rule is `Policy::can_see_project`; `repositories/project_access.rs` is its only SQL form (`ProjectScope`, `user_sees_project_sql`). An unseen project and everything in it (tasks, comments, labels, statuses, files, relations, parent/duplicate data, inbox, push, collab, API tokens, OAuth/MCP grants) answers like a missing one. Assigning someone who cannot see the project is 422; web and mobile assignee and mention pickers offer only people who see it. Private projects show a lock (sidebar/drawer, pickers). Known gaps: the `blocked` flag and activity metadata can still reflect a private related task; existing assignees keep their assignment after losing access.

## Docs

Core files: `features/docs/pages/DocsPage.tsx`, `pages/PageTrashPane.tsx`, `components/DocTree.tsx`, `components/DocEditor.tsx`,
`editor/PageEditor.tsx` (BlockNote), `api/pages.ts` + `api/teamspaces.ts` + `api/favorites.ts` (query hooks), `autosave.ts`,
`pageTree.ts` (pure tree and space helpers).

Pages persist in SQLite through `/api/v1/workspaces/{id}/pages`, teamspaces through `/api/v1/workspaces/{id}/teamspaces`,
favorites through `/pages/favorites` (list), `/pages/{page_id}/favorite` (PUT/DELETE) and `/pages/favorites/{page_id}/move`.

- Spaces: every page lives in a teamspace or in the caller's Private space (a page's space is its root's; sub-pages follow).
  Every member sees and edits all teamspaces; private pages are visible only to their owner (tree, search, trash, links).
  Each workspace starts with a default teamspace "General"; new root pages go there unless created in another space.
  The default is derived: the first teamspace by position (with several teamspaces its header shows a small "Default"
  label with a tooltip), so moving another teamspace to the top makes it the default.
- Sidebar: 48px "Documents" header (its "+" creates a page in the default teamspace), a "Teamspaces" label with "+" (create
  dialog), one collapsible section per teamspace (hover "+" adds a page there; "…" menu: Rename inline, Change icon (emoji
  picker anchored to the header; "Remove" returns to the default people glyph), Move up / Move down, Delete teamspace for
  owners/admins only — refused with a toast while it has pages or is the last one), then the Private section (lock, hover "+").
  Empty sections show "No pages inside". Collapsed sections persist per workspace in localStorage.
- Teamspace order: drag a teamspace header before/after another one (primary line indicator, the dragged header stays
  mounted and faded; the drag carries `application/x-orbit-teamspace`, so page rows and the Private/Favorites headers ignore
  it, and page drags onto a header still move the page into that space). `POST /teamspaces/{id}/move`
  `{ expected_version, position }` → the renumbered list (any member, audited `teamspace.moved`, only the moved teamspace's
  version changes). Optimistic with rollback and an error toast.
- Auto-reveal: whenever the active page changes (navigation, search, links, favorites, imports) the sidebar opens the
  collapsed section of its space and its collapsed ancestor rows (it never closes anything), then scrolls its row into view
  (`block: 'nearest'`). Collapsing while staying on the page sticks; Favorites rows keep their own expansion.
- Favorites (Notion-style): per user and workspace, nobody else sees them, not audited. A "Favorites" section (star label) sits
  above Teamspaces and is hidden while empty; it lists favorite pages in the user's order as normal rows (icon, title, expandable
  sub-pages with their own expand state, active highlight); the pages stay in their own space too. Toggle with the star button in
  the page header (filled when favorited, `aria-pressed`), "Add to / Remove from favorites" in the page "…" menu and every row "…"
  menu, or by dropping a page from another section onto the Favorites header. Dragging a favorite reorders the favorites only
  (the page never moves); drops between Favorites and other sections do nothing. Only live pages the user can see are listed:
  a trashed favorite drops out and comes back on restore; a page moved into someone else's private space disappears for others.
  Add/remove/reorder are optimistic with rollback and an error toast.

- Routes: `/docs` opens the first root page of the default teamspace, else the first private page, else the first page of
  another teamspace, on desktop (phones show the page list first); no pages → "Create your first page" (default teamspace).
  `/docs/:pageId` opens a page; a trashed or unknown id shows "Page not found" with a link back. `/docs/trash` is the page trash.
- Page tree: create (root or child), "Move to trash" with confirmation (the whole subtree goes), HTML5 drag and drop
  before/inside/after mapped to `move` with `parent_id` + sibling index (root siblings are per space). Drops can cross spaces:
  onto/under a page the space follows the parent; next to a root page or onto a section header/empty row the page goes to that
  space (`teamspace_id` or `private: true`), taking its subtree along, with a "Moved to …" toast. The tree updates
  optimistically (space included for the whole subtree) and rolls back on error.
- Duplicate (row "…" menu and page header menu: "Duplicate", plus "Duplicate with sub-pages" when the page has children):
  `POST /pages/{page_id}/duplicate` `{ include_children }` → 201 `Page` (the new root). "<title> (copy)" ("Untitled (copy)"),
  same space and parent, placed right after the original; copies icon, cover, content and, with sub-pages, the live subtree
  in order. Files are shared: new `page_files` rows for the same blobs (no byte copy), and page-file URLs (image/file blocks,
  cover), `page` block `pageId`s and `/docs/<id>` links that point inside the copied pages are rewritten to the copies;
  links elsewhere stay. Any member who can see the page; each copy is audited `page.duplicated`. The open editor is flushed
  first; on success the copy opens with a toast. (`features/docs/pageActions.ts` context from DocsPage.)
- Export (page header "…" → Export: "Markdown", "Markdown with sub-pages" only when the page has children, "PDF"):
  `GET /pages/{page_id}/export?format=markdown&children=false|true` → `application/zip` (`Content-Disposition` with an
  ASCII `filename` and an RFC 5987 `filename*`, `no-store`). Access = the page's (another member's private page, trashed or
  unknown → 404 `page_not_found`); with children only the live sub-pages the caller can see (a trashed sub-page drops its
  subtree). Open documents are flushed first. Limits: 500 pages (413 `export_too_many_pages`) and 200 MiB uncompressed
  (413 `export_too_large`); the archive is written to an anonymous temp file on a blocking thread and streamed.
  Layout (Notion-like): `<Title>.md`; sub-pages in the folder `<Title>/` next to it; a page's files (only those its
  content or cover references) in `<Title>/assets/`. Names: separators/reserved characters → `-`, control characters,
  leading/trailing dots and Windows device names handled, 80-character stems, "Untitled" when empty, unique per folder
  ignoring case (" (2)"), `assets` reserved in sub-page folders. Each `.md` has front matter (`title`, `icon`,
  `exported_at`), `# Title`, the cover as an image, then the body. Links: `page` blocks and `/docs/<id>` links to exported
  pages → relative `.md` paths, page files of exported pages → relative asset paths (all percent-encoded); other visible
  pages and files → absolute URLs on the configured public origin; pages the caller cannot see → "*Missing page*" (no
  title leak); external links unchanged. Converter (`apps/server/src/export/markdown.rs`, BlockNote JSON → GFM,
  deterministic): headings 1-6 (toggle headings too), paragraphs, bulleted/numbered (with `start`)/check lists and toggle
  lists with nesting, quotes, code fences with language (longer fences when the code has backticks), dividers, GFM tables
  (first row as header, `\|` escaped, colspans padded, newlines `<br>`), images `![name](path)` + italic caption, files
  as links, callouts as `> 💡 text` blockquotes with their children, bold/italic/strike/code/`<u>` underline, links, hard
  breaks (`\`), Markdown punctuation escaped; colors and alignment dropped. Goldens: `apps/server/tests/fixtures/export/`
  (`UPDATE_EXPORT_GOLDENS=1`). Web: `features/docs/pageExport.ts` (SDK blob download with progress/success/error toasts;
  413 shows the server's message). PDF = `printPage`: adds `orbit-print-doc` to `<html>` and the page title as document
  title, calls `window.print()`, restores on `afterprint`; `@media print` rules in `src/index.css` keep only
  `[data-print-root]` (DocEditor's content; `[data-print-hide]` drops the add icon/cover row), full width, light theme
  (also over BlockNote's own `.dark` containers), images fitted, page breaks avoided inside images/code/tables/headings.
  Static CSS (production CSP). Smoke: `target/orbit-smoke/export.mjs`.
- Editor: BlockNote 0.55 (rich text, markdown shortcuts, nested blocks, drag handles, undo, slash menu), lazy-loaded in its own
  chunk. The schema lives in `packages/editor` (`@orbit/editor`): `createEditorSchemas(views)` builds the page and task
  schemas from fixed block configs (`page`, `callout`, `mention`, code block + `sugar-high` highlighting); a host only
  passes how blocks render (web: `editor/schema.ts` with the shadcn views; mobile: `PageEditor.dom.tsx`). The package
  test checks that any views give `blocknote-schema.json`, so every editor bound to a page's Y.Doc has the server's
  schema. The package also holds `codeLanguages`, `commentMarks()` (the comment mark for editors without threads),
  the framework-free `CollabSession` (status, close codes) and `ticketWebSocket` (a fresh socket ticket per connect). Custom `page` block links to a page; it greys out as "Missing page" when the target is trashed or gone.
  Slash items "Sub-page" (creates a child page, inserts the link, opens it) and "Link a page" (picker over the tree).
  Custom `database` block `{databaseId, linkId}` (`editor/DatabaseBlock.tsx`, databases P2 spec section 3): the database's
  title, view tabs, toolbar and table or board inside the page (`database/DatabaseSurface.tsx`, shared with the full
  database page), 25 rows then "Load more" (25 cards per board column), rows open in the peek (`?row=&row_block=`). Its
  root is isolated (`@orbit/editor/isolation`): keys, pastes and drags inside never reach the editor. Slash "Database –
  inline" makes a database sub-page (shown in the tree) and the block; "Linked view of database" picks a database and
  `POST /databases/{id}/links` gives the block views of its own (`database_views.link_id`; changes follow the host page's
  lock, the database's own views stay as they are; `GET /databases/{id}?link=`). Deleting an inline block keeps the
  database ("Move to trash" in the toast); "Turn into" swaps a page link to a database and a database block. States: "No
  access to this database" (404), "Database in trash", "View not found" (offers the database's own views). Mobile shows a
  card (icon, title, rows, first view) that opens the database screen with `link` and `view`; Markdown writes the block as
  a link to its database; task text drops it; duplicates copy links and their views.
- Database list and calendar views (databases P2/P3 spec section 13). List (`database/DatabaseList.tsx`): one 36 px line
  per row (icon, title, then the visible columns that have a value, right-aligned); drag to reorder without sorts; click
  opens the peek. Calendar (`database/DatabaseCalendar.tsx`, shared logic `@orbit/database/calendar`): config
  `date_property_id` (a date, created time or last edited time; repair takes the first date property) and
  `calendar_mode` (`month` 6-week grid, or `week`); a range is a bar across its days, a month cell shows 3 cards then
  "+N more", "No date (N)" lists undated rows. Cards of a date property drag to another day (start and end move by the
  same days, times kept in the viewer's zone) and a range's right edge drags to change the end; "+" on a day adds a row
  with that date (plus filter values and the default template). Week start: the locale, or "⋯ → Start week on" (this
  browser, `orbit:calendar-week-start`). Mobile: list views show all visible properties; a calendar is a month grid with
  dots and the tapped day's rows under it (`components/database/Calendar.tsx`), no drag.
- Database form views, members only (databases P2/P3 spec section 18, `database/DatabaseForm.tsx`, shared logic
  `@orbit/database/form`). Config `form` `{questions: [{property_id, label?, description?, required}], title?,
  description?, submit_label?, confirmation?, access: members|closed}`; a form view without one asks for the title.
  Editors (view edit rules) build it: texts, question cards (drag or arrows to reorder, label, help text, Required),
  "Add question" (a property not asked yet, or a new property), "Accepting answers"; "Preview" tries it without
  sending. "Copy form link" copies `/docs/<database>/form/<view>`, a full-page form (`DatabaseFormPage.tsx`) for members
  who can open the database. `POST /databases/{id}/forms/{view}/submit` `{answers}` adds the row as the submitter in
  one transaction (row cap, `database.row_created` and `database.form_submitted` audit events); closed is 409, more
  than 30 submissions per member per hour in a workspace is 429 (counted from the audit events). No computed,
  unique ID, files, relation or button questions. Mobile fills and submits form views (`components/database/
  FormFill.tsx`; screen `/docs/form/<database>/<view>`, which a web form link in a page opens); the builder is on the web.
- Database email, phone, files and unique ID properties (databases P2/P3 spec section 12). Email and phone are checked
  text shown as `mailto:`/`tel:` links. Files hold `[{file_id, name}]` (max 20) of the row page's own files: the web
  popover uploads (button or drop), reorders and removes; mobile uploads from the photo library or files app. Unique ID
  (`{prefix, next}`, one per database) numbers existing rows by creation when added and each new row in its create
  transaction (also CSV import, template, duplicate, move-in, restore); numbers are never reused and only the Notion
  import may set them. Notion email, phone, unique ID and uploaded files import as these types.
- Database button property (databases P2/P3 spec section 17, server `repositories/databases/buttons.rs`, shared
  `@orbit/database/buttons`). Config `{label, actions (max 5), confirm}`; actions `set_values` (this row), `add_row` (this
  or another database the presser can open, appended) and `open_url` (one, client side). Values take `"me"` (person),
  `"today"`/`"now"` (date), and in `add_row` a title string and `"this_row"`. `POST /databases/{id}/rows/{row}/press/
  {property}` `{today?}` runs the server actions in one transaction (any failure rolls back all; 422 names
  `actions[i]`), records `database.button_pressed` (plus `database.row_created` per added row) and returns `{row,
  created_rows, open_url?}`; 10 presses per user per second per server (429 `database_button_rate_limit`). A button
  stores no value, cannot be filtered or sorted, and is skipped by search, CSV import and forms. Web: the cell button
  (confirm dialog, spinner, error toast; Enter presses it in the table), "Edit property → Edit button…" opens
  `ButtonConfigDialog`. Mobile presses in the row's property list and in list, gallery, timeline and calendar rows
  (`components/database/ButtonCell.tsx`); its config is edited on the web. Notion buttons stay skipped.
- Database gallery and timeline views (databases P2/P3 spec section 16). Gallery (`database/DatabaseGallery.tsx`, shared
  `@orbit/database/gallery`): a grid of 180/260/340 px cards (`card_size`) with a 3:2 preview (`card_preview`: `none`,
  `cover`, `content` = the body's first image else its first 300 characters, or `{files: id}` = a files property's
  first image; `fit_image`), the title and `card_properties`, "New" as the last card, drag to reorder without sorts.
  `GET /databases/{id}` adds `preview` to rows only when a returned view is a gallery that needs it. Timeline
  (`database/DatabaseTimeline.tsx`, shared `@orbit/database/timeline`): `start_property_id` and `end_property_id`
  (dates; no end: a range's end, else one day), `zoom` (day to year), `show_table` and `table_columns` for the left
  pane; drag a bar to move both dates or an edge to change one (a moved time keeps its local hour); undated rows list
  under "No date" and a click on their track sets a date; zoom and "Today" above the axis; its own scroller, rows
  virtualized. Mobile: a gallery is a 2-column grid (1 for large cards, `components/database/Gallery.tsx`); a timeline is
  a list grouped by month with "Timeline · Edit on the web" (`TimelineList.tsx`). Notion gallery and timeline views
  import with their preview, size, dates, zoom and table pane.
- Database relation and rollup properties (databases P2/P3 spec section 11, D-P3-1/D-P3-5). A relation links rows to
  rows of a target database (any space the user can open, or the same one); each link is one `page_relations` row under
  the pair's primary property, and the synced property of a two-way pair ("Show on <database>") reads the same rows
  from the other side. `GET /databases/{id}?tz=` fills relation values (visible page ids), `hidden_links` counts and
  `related_pages` titles, and computes rollups (`repositories/databases/computed.rs`, the shared computed-value layer
  that formulas also use); a write replaces the cell's visible links (max 100) and keeps hidden ones. Rollup functions
  as Notion's (show, counts, percents, number, date and checkbox functions). Trashed pages are left out, permanent
  deletes and moves out of a database drop links, a duplicated database keeps links one-way. Web: pills, a target-row
  picker and relation/rollup setup in "New property"; mobile shows and edits links (search sheet) and edits their
  configs on the web. Notion relations stay relations when both databases are imported (dual = two-way), outside
  links stay as "<name> (titles)" text, rollups over them stay rollups.
- Database formula property (databases P2/P3 spec section 15, D-P3-1/D-P3-3). Only the server runs formulas
  (`repositories/databases/formula/`: `syntax` lexer and parser, `check` type checker to IR, `eval` with a step counter,
  `dates` on jiff in the reader's `?tz=` zone), on every read as the formula pass of the computed-value layer. Config
  `{expression, result_type}`; clients send `prop("Name")` or ids, the server stores ids and infers the type. A failed
  cell is `{"error": message}` (`row.formulaErrors` on clients). Limits: 2,000 characters, 500 nodes, depth 32, 2,000
  steps per cell, lists 1,000, text 10,000, 20 formulas, 15 formula layers, 2 s per read. `POST
  …/formula/preview` checks an expression and computes the first 10 rows; save errors are 422 `formula_invalid` with
  `formula: {message, start, end}` (UTF-16). Filters, sorts and calculations follow the result type
  (`@orbit/database/formula/values`). Web: "Edit property → Edit formula…" (`FormulaDialog`: highlight, completion,
  error, preview); cells show results or a muted "Error" with the message on hover (`cells/FormulaCell.tsx`). Mobile
  shows results, taps show errors, editing says "Edit this formula on the web". Notion formulas that parse and type
  check import live (`notion/formula.rs`), others keep static values.
- Databases for API tokens and MCP (databases P2/P3 spec section 14): tools `get_database`, `query_database`,
  `create_database_row`, `update_database_row`, `trash_database_rows` and REST `GET /docs/{id}/database`, `POST
  /docs/{id}/database/query`, `POST /docs/{id}/rows`, `PATCH /docs/{id}/rows/{row}`, `POST /docs/{id}/rows/trash` under the
  docs scopes (personal tokens only). Keys are property names or ids, options go by name, people by id, email or "me";
  results name options and show people as `{id, name}`. Queries filter and sort in Rust in memory
  (`repositories/databases/query.rs`, the rules of `@orbit/database` filter/sort; shared cases in
  `packages/database/cases/`), 100 rows per page with an opaque cursor; name translation is
  `repositories/databases/agent.rs`. `get_doc` of a row adds `values`.
  Custom `callout` block (`editor/CalloutBlock.tsx`; slash "Callout" in Basic blocks after Quote, aliases callout/note/tip/
  warning/info): inline rich text, props `emoji` (default 💡; click it to pick another in the app's emoji picker),
  `backgroundColor` (BlockNote color names, default `gray`; `default` = border only) and `textColor`, both editable in the
  block "Colors" menu. Notion-like rounded box that also holds nested blocks; tint mixed per theme (light full, dark faint).
  Copy/paste inside Orbit keeps it; other apps get "emoji + text".
  Image and File blocks (slash menu, file panel, paste and drop) upload to the page; video/audio blocks stay off. Image/file
  block URLs are limited to page-file paths, http(s) or empty (anything else is blanked on load and before saving).
- Page files: `POST /pages/{page_id}/files` (multipart, one `file` field) → `PageFile` with a same-origin `url`
  (`/api/v1/workspaces/{ws}/pages/{page_id}/files/{file_id}`, satisfies the CSP `img-src 'self'`); `GET` that url downloads.
  Access = the page's: teamspace pages for every member, private pages only for their owner, trashed pages for nobody
  (404). Bytes use the task-attachment blob layer (`UploadService`: size limits, sniffed type, per-workspace dedup);
  png/jpeg/gif/webp/avif are served inline, everything else (SVG, HTML, …) as an attachment, always `nosniff`. Upload
  audits `page.file_added`. Rows (`page_files`, migration 0025) go with the page (trash purge, teamspace/workspace
  delete) and quarantine the blob; reconciliation reclaims blobs no task attachment or page file references.
  `PageFileRepository::create_from_bytes` attaches server-held bytes (Notion import) through the same checks.
- Header: breadcrumbs prefixed with the space (teamspace name, or a lock + "Private"; the mobile top bar too), presence
  avatars and the sync status (see "Real-time co-editing, web" below; a failed title/icon/cover save shows Save failed +
  Retry or Conflict instead), page menu with "Move to" (teamspaces + Private, current disabled; moves to
  the end of that space's root) and "Move to trash".
- Title input (empty title shows "Untitled" everywhere), emoji icon picker, cover (upload an image to the page, or an http(s)
  URL; `cover_url` also accepts a file url of the same page only) with remove/change and focal-point repositioning.
- Content is live-collaborative (next bullet but one); the client never PATCHes `content` (`savePage` takes
  `PageMetadataUpdate` = no `content`; REST content writes stay for API clients).
- Autosave (`autosave.ts`) covers title/icon/cover only: title debounces 800 ms; icon/cover save immediately; blur, page
  switch, unmount and tab hide flush. One PATCH in flight at a time; each save sends the latest `expected_version`. A 409
  on changes that touch the same fields shows a banner: "Reload page" (take the server's title/icon/cover) or "Overwrite".
- Remote metadata updates (realtime refresh, other tabs) apply title/icon/cover when there are no unsaved local edits;
  metadata-only changes (e.g. a move) just advance the version. Content never comes from the page query after mount.
- Real-time co-editing, web (`features/docs/collab/`: `session.ts` state machine, `connection.ts` socket factory +
  `CollabConnectContext` for tests, `useCollabSession.ts`, `presence.ts`, `PresenceAvatars.tsx`, `cursor.ts`,
  `blockPresence.ts`; test fake
  `src/test/fakeCollab.ts`): DocEditor opens one Y.Doc + y-websocket provider per page/epoch (yjs + y-websocket load on
  demand, `disableBc`, backoff ≤ 5 s) and mounts `PageEditor` with `collab` (`withCollaboration`, no `initialContent`,
  Yjs undo, caret labels `showCursorLabels: 'activity'`) only after the first sync; both are destroyed on page switch.
  Header status: "Live" (green dot) / "Connecting…" (before the first sync and for the first 2 s of a drop) / "Offline —
  changes will sync" / "Access lost" / "Not syncing". Close handling: 4404/4403 → the editor turns read-only, a toast
  ("“X” was moved to the trash." / "You no longer have access to “X”."), the page leaves the tree and the app goes to
  /docs (not for the user's own "Move to trash", which announces itself); 4409 → refetch the page (new epoch) and
  reconnect with a fresh document; 4402 (Orbit Cloud: the workspace lost its subscription; every server closes its
  editing sockets of the workspace) → read-only toast, billing refetched, then the same as 4409, and the new socket is
  read-only; 4401 → the app-wide unauthorized flow (login); 4413 → "too large" banner + read-only
  until "Reload page"; 4400/4426 → "Reload app" banner; 4429 → toast, reconnect after 5 s; 1012/1013 and drops →
  y-websocket retries. Handshakes refused before the upgrade (HTTP 401/404 arrive as 1006) trigger a throttled REST
  check of the page after 2 failures (404 → leave like a trash, 401 → login). `beforeunload` warns only while
  disconnected with local edits typed offline or within 3 s before the drop (and for unsaved title edits).
  Presence: avatars (max 4 + "+n", `aria-label` "Also here: …") from awareness, deduped by the server-stamped user id,
  without the own user (other tabs included). Colors come from the server (`hub.rs::room_color`): everyone on a page gets
  a different one of 9 while one is free (their usual, id-hashed color if nobody has it), all tabs of a user share it,
  and avatar ring, caret and marker use it. A full ring means editing (a tab publishes a cursor), a faint ring viewing;
  hover shows "Name · Editing/Viewing", a click opens the shell profile popover, with "Go to cursor" (scroll + name
  flag, `revealCollabCursor`) for someone editing. Carets carry `data-collab-user` (`renderCollabCursor`); each block
  holding someone else's caret gets a right-margin marker with initials (+n) in their color (`collabBlockPresence`). Restore from history, Notion import and REST content PATCH arrive
  live through Y; the history pane's restore only applies the returned title/icon (+ toast, list refresh).
  E2E: `target/orbit-smoke/collab.mjs [base] [label]` (two users, REST PATCH live, typing both ways with latency,
  presence colors, reload, restore both ways, trash notice, CSP/console check; `COLLAB_DB` backdates the test page so a
  version exists).
- Trash: lists pages trashed directly with their space; Restore brings back the subtree trashed with them (to the root of its
  space if the parent is gone). "Delete forever" (row, danger confirm) → `DELETE /pages/{page_id}/permanent?expected_version=`
  hard-deletes a directly-trashed page and the sub-pages trashed with it (live or not-directly-trashed pages: 409
  `page_not_trashed`); "Empty trash" (pane header, confirm with the count) → `POST /pages/trash/empty` → `{ purged }` deletes
  every trashed page the caller may purge. Rule: own private pages by their owner (others get 404, as everywhere);
  teamspace pages only by workspace owners/admins (members: 403 `workspace_action_forbidden`, and the UI hides the button;
  Empty trash leaves them alone). Files (blobs quarantined, then reconciled), favorites and the search entry go too; sub-pages
  trashed separately earlier keep their own entry and restore to their space root. Audited `page.purged` (`{"pages": n}`).
- Search: the command palette searches page titles and body text (debounced, only for a non-empty query), shows
  "Page · <space>", the title and a body snippet with matched words in bold, and opens `/docs/:id`. Server: SQLite FTS5
  (migration 0027: `page_search` over title + `content_text`, tokenizer `unicode61 remove_diacritics 2`, kept in sync by
  triggers; rowids come from `page_search_rows`, stable across VACUUM). Input is split on whitespace, words without a
  letter/digit dropped, each word quoted (FTS syntax is plain text), the last one a prefix, all must match (max 16 words,
  200 chars). Live, visible pages of the workspace only; ranked by `bm25` with title ×10. Results carry `snippet` (plain text,
  ~24 words, `…` where cut; the body start for title-only hits), `snippet_highlights` and `title_highlights` as
  `TextRange { start, end }` in UTF-16 code units (JS string indices, end exclusive); the UI renders them as `<strong>`
  text nodes (no HTML). On Postgres, `page_search` is a table with a `tsvector` column in the `orbit_search` text search
  configuration (unaccent + simple), ranked by `ts_rank` with titles weighted 10x.
- Obsidian import (`pages/ObsidianImportPane.tsx`, `components/ObsidianImportChooser.tsx`, `api/obsidianImports.ts`;
  server `obsidian/` and `collab::markdown_import::obsidian_to_blocks`, routes `/imports/obsidian`, migration 0072):
  Documents header "…" → "Import from Obsidian" and Settings → Import open `/docs/import/obsidian`. Upload one `.zip`
  of a vault (progress, cancel) → `obsidian.scan` checks the archive safely (`obsidian/vault.rs`: no extraction, `..`,
  absolute paths, symlinks, encryption, entry/size/ratio caps fail it with a message shown on the page; `.obsidian/`,
  `.trash/`, `__MACOSX/`, dotfiles, canvases, CSV, oversized files and case duplicates are skipped and counted; one
  top-level folder is stripped) → tree (folders that hold notes become pages; `Folder/Folder.md` is the folder page's
  content; a folder without one lists its sub-pages) with the Notion import's tree picker and destination fields
  (`components/ImportTreePicker.tsx`, `ImportDestinationFields.tsx`, `ImportParts.tsx`, `import/*.ts`, shared) →
  `obsidian.import`: pass 1 creates pages parents first, pass 2 converts each note with wikilinks resolved like
  Obsidian (relative path, vault path, else unique file name; `Note#Heading` links to the note, unresolved links stay
  text), linked and embedded files attached as page files (images → image blocks, other files → file blocks, unlinked
  attachments skipped), `![[Note]]` → link, `> [!type]` callouts → `callout` blocks, `==x==` → yellow background,
  `%%comments%%` and frontmatter dropped, the Notion page size cap. Same lifecycle rules as the Notion import (private,
  resumable, cancel keeps filled pages and trashes unfilled ones, trial page limit, read-only stop, 24 h expiry); the
  archive is deleted in every final state.
- Notion import (`pages/NotionImportPane.tsx`, `components/NotionImportChooser.tsx`, `api/notionImports.ts`,
  `import/selection.ts`; server `notion/import.rs`, routes `/imports/notion`): Documents header "…" → "Import from
  Notion" opens `/docs/import`, a pane next to the tree like Trash. Steps follow the import's status at
  `/docs/import/:importId`: Connect (password field for a Notion personal access token, sent once, never cached client-side;
  problems map to sentences: invalid token, app key missing, import in progress + link to it, Notion unavailable) →
  Scanning (polls every 2 s) → Choose pages (tri-state tree: checking a page includes its sub-pages, unchecking one also
  un-checks its ancestors; filter, Select all / Clear, "N pages selected", truncated/incomplete warnings; destination
  teamspace or Private plus optional "Inside page" of that space; sends `{all:true}` or the minimal checked roots) →
  Importing (progress bar, failed count, Cancel with confirmation) → Complete / Failed / Cancelled / Expired (stats, conversion
  notes, failures with titles and errors, links to the imported top-level pages, "Open imported pages", "Import more").
  The connect step lists the member's 20 recent imports. Imports are private to their creator. Import query keys live outside
  the workspace prefix (polled, not refetched on every realtime event); pages are refreshed when an import ends.
  `GET /imports/notion/{id}` is light (no scan tree; also list/create/start/cancel); `?include=tree` adds it. The pane polls
  the light detail every 2 s only while scanning/queued/importing and fetches the tree once, under its own query key, when
  the import is `ready`. Notion callouts become `callout` blocks (emoji icon, color, nested children; a non-emoji icon
  becomes 💡 and counts as `callout_icon` in the report).
  A Notion data source becomes an Orbit database (`notion/database.rs`, mapping in the databases spec section 14):
  typed properties (computed Notion types as static values), option colors, row values, one table, board, list or calendar view for
  each Notion view (AND filters and sorts kept), rows as row pages in Notion order (max 5,000). A database with several
  data sources is one tree node and one database for each data source. An inline database is a database sub-page,
  and its block becomes a `database` block of the database's own views. Pass 1 fetches each data source before writing and creates databases and
  rows through the databases repository; property and option ids derive from the Notion ids, so a resumed import
  finds them. The report adds `databases_imported`, `rows_truncated`, database lossy reasons and skipped
  `property:<type>`; web and mobile render them as sentences (`@orbit/database/notionImport`).
  When an import fails (e.g. revoked token) or is cancelled, the pages it had created but not filled go to the trash (one
  audited batch per top-level page; a subtree is kept if anything in it has content or is not from the import) and the report
  counts them (`unfilled_pages_trashed`). Filled pages stay.
- Version history (`components/PageHistoryPane.tsx`, `api/pageVersions.ts`; server `repositories/page_versions.rs`,
  migration 0028 `page_versions`): page "…" → "Version history" opens a right pane next to the editor (full screen on
  phones): "Current version" on top, then versions grouped by day (time, author avatar + name, "Before a restore" /
  "Imported from Notion"), "Load older versions" (cursor pages of 50); the selected version (newest by default) is
  previewed read-only with BlockNote (same lazy chunk, never collaborative); "Restore this version" asks first, flushes
  pending title/icon/cover edits, then every open editor receives the restored content live (Yjs replace), this one takes
  the restored title/icon from the response, the pane closes and a toast confirms. Arrow keys/Home/End move through the
  list, Escape closes. No history yet: "Versions appear after you edit this page."
  Snapshot rule (server, inside the save transaction): an edit that changes title or content stores the *previous* state
  (title, icon, content, author = its last editor) as `auto` when the newest version, or the page itself if it has none,
  is at least 10 minutes old; blank states and states equal to the newest version are skipped. Restore stores the current
  state as `restore` first, then writes through `page_versions::replace_content` (the single server-side document
  replacement; it also replaces the live collaborative document); Notion imports store the imported state as `import`;
  duplicates start without history; versions go with their page. Retention (`workspace.retention` job): all from the last
  30 days, then the newest per UTC day up to a year, always the newest 20 per page. API: `GET /pages/{id}/versions`
  (`?cursor&limit` 1–100) → `{ items, next_cursor }` without content, `GET /pages/{id}/versions/{vid}` (with content),
  `POST /pages/{id}/versions/{vid}/restore` `{ expected_version }` → `Page` (409 on a stale version, audited
  `page.restored_version` with `version_id`). Visibility = the page's: hidden or trashed pages answer `page_not_found`
  like unknown ids; an unknown version of a visible page is `page_version_not_found`. Cover and place are not versioned.
- Real-time co-editing, server side (`apps/server/src/collab/`, migration 0029 `page_collab_*`; web side above): one in-memory Yjs document ("room", yrs 0.28) per open page, always on.
  Socket `GET /api/v1/workspaces/{ws}/pages/{id}/collab?v=1&epoch=<Page.collab_epoch>` speaks y-sync v1 (y-websocket 3:
  `new WebsocketProvider(origin + '/api/v1/workspaces/{ws}/pages/{id}', 'collab', doc, { params: { v: '1', epoch } })`,
  fragment `prosemirror`). Handshake: one allowed `Origin` (platform layer, 403), session cookie (401), then membership +
  live page visible to the user, else 404 exactly like an unknown id. A WebView socket (mobile editor) that cannot send
  the cookie instead appends `ticket=` from `POST /api/v1/auth/socket-ticket` (`socket_ticket.rs`: stateless, HMAC
  under an HKDF subkey of the app key, 60 s, session re-checked); with a ticket and no Cookie any Origin passes, and a
  ticket plus a cookie or bearer is 401. After the upgrade: `v` ≠ 1 → 4426, stale/missing
  epoch → 4409. Close codes (4400–4499 terminal for y-websocket): 4400 malformed, 4401 session ended, 4403 no access (moved
  into someone's Private, removed from the workspace), 4404 trashed/deleted, 4409 reset, 4413 frame > 1 MiB or document
  > 8 MiB (checked before applying), 4429 > 200 sync / 30 awareness msgs/s; 1012 restart, 1013 overloaded (50 sockets per
  page, 20 per user, 2000 per process, 256 MiB document budget, lagging socket). Every socket re-checks access every
  15 s and at once after page move/trash/purge, empty trash, teamspace delete, member removal, workspace delete, user
  suspension and session revocation. Pings every 20 s; accepted TCP sockets use TCP_NODELAY (`App::serve`).
  Awareness: the server echoes updates to everyone incl. the sender, overwrites `state.user` with
  `{ id, name: display name, color: one of 8 by user id }`, lets a socket speak only for its own client ids (≤ 4) and
  broadcasts their removal when it closes.
  Storage: first open converts `content_json` in Rust (`collab/blocknote.rs`, schema table `blocknote-schema.json`
  generated from `pageEditorSchema`; parity goldens in `apps/server/tests/fixtures/collab`, both directions) inside the
  room lock, so no client ever binds to an empty document. Updates are applied in memory, broadcast, and written as one
  merged row per 150 ms / 64 updates; the log is folded into the snapshot every 500 updates or 1 MiB and on eviction
  (30 s after the last socket closes, LRU above the budget). Shutdown writes everything and closes sockets with 1012;
  clients re-push what the server missed on reconnect. Projection: 2 s after the last change (≤ 10 s while typing), on
  eviction and on demand (page GET, duplicate, restore) the document is written to `content_json`/`content_text` through
  `page_versions::project_collab_content` — history rule of edits, `updated_by` = last editor, `pages.version` unchanged,
  at most one `page.updated` audit/outbox event per page per 10 min. Unsafe links / image-file URLs arriving in Yjs updates
  are repaired by a server transaction that everyone receives. Server-side content writes (version restore, REST PATCH
  `content`, Notion import) lock the room, apply a normal Yjs replace transaction logged in the same DB transaction and
  broadcast it after commit (open editors switch live); pages never opened only get `content_json`. `Page.collab_epoch`
  changes on converter/schema changes (stored document rebuilt from JSON) and on backup restore (all epochs and the
  generation of never-opened pages rotate), so clients holding newer state reset instead of re-pushing it.
- Page options (migration 0031: `pages.full_width`, `locked_at`, `locked_by` → users SET NULL; `page_visits`):
  `Page` carries `full_width`, `locked_at`, `locked_by { id, display_name }` and `updated_by_user { id, display_name }`.
  Full width: PATCH `full_width` with `expected_version` (any member who can see the page; a layout change, not an edit:
  `updated_by`/`updated_at` stay; allowed while locked); page "…" → "Full width" (checkbox item, saved through the
  autosaver) makes the editor column use the pane width.
  Lock: `POST /pages/{id}/lock { locked }` → `Page` (any member who can see the page; setting the current state is a no-op;
  a change bumps the version, audited `page.locked` / `page.unlocked`); page "…" → "Lock page" / "Unlock page". While
  locked, PATCH title/icon/cover/content, version restore and Notion import fills answer 423 `page_locked` (checked before
  the version); moving, trashing, duplicating (the copy is unlocked), favorites and full width stay allowed. Co-editing:
  the room holds `locked` (loaded from the page; flipped by `CollabHub::lock_change` holding the room lock across the
  transaction); while locked, client Update/SyncStep2 messages are ignored (not applied, broadcast or stored), SyncStep1
  and awareness still work; lock and unlock close every socket with **4423**, which the web treats like 4409 (refetch the
  page, fresh Y.Doc, reconnect), so refused edits never resurface. Web: locked pages render read-only (editor
  `editable=false`, title read-only, icon picker disabled, no Add icon/cover or cover controls) with a "🔒 Locked by
  <name|you>" pill + Unlock above the title; unsaved title/icon/cover edits are dropped when the lock arrives.
  Header "Edited <just now | n minutes/hours/days ago | on Mon d> by <name | you>" (`lastEdited.ts`, hidden below
  1100 px): the server's `updated_by_user`/`updated_at`, overridden by live edits seen on the Y doc (local typing → you;
  remote updates → the awareness user of the writing client id), re-evaluated every 30 s.
  Recent pages: `POST /pages/{id}/visit` (204; sent once after 1.5 s on a page, so background refetches never count;
  upsert, newest 50 kept per member and workspace) and `GET /pages/recent?limit=1..50` (default 10) → `{ items: RecentPage }`
  with live pages the caller can see now only (a page moved into someone else's private space or trashed drops out).
  Private to the member, not audited. Shown as a "Recent" group (6) at the top of the command palette while the query is
  empty; not in the docs sidebar (Favorites + spaces already fill it; the palette is one keystroke away).
  E2E: `target/orbit-smoke/options.mjs`.
- Comments + @mentions (migration 0030: `page_threads`, `page_comments`, `page_comment_mentions`; `notifications`
  rebuilt with optional `task_id` and `page_id`/`page_thread_id`/`page_comment_id`, kind `page_comment_mentioned`):
  server `repositories/page_comments.rs` + `page_comment_routes.rs`; web `features/docs/comments/` (`threadStore.ts`,
  `api.ts`, `mentions.ts`, `CommentEditor.tsx`, `CommentComposer.tsx`, `ThreadCard.tsx`, `ThreadList.tsx`,
  `CommentsPanel.tsx`). REST under `/pages/{page_id}/threads`: GET (open and
  resolved threads with comments oldest first) / POST `{ body, quote }` → 201 `PageThread`; `DELETE /threads/{id}` (thread
  creator); `POST /threads/{id}/comments { body }` → 201 `PageComment`; `PATCH|DELETE /threads/{id}/comments/{cid}` (author
  only, else 403 `page_comment_forbidden`; delete keeps a placeholder, the last live comment takes the thread with it);
  `POST /threads/{id}/resolve|reopen` (anyone who sees the page; `resolved_by`/`resolved_at`). Access = the page's (hidden,
  trashed or foreign pages answer 404 `page_not_found`, like an unknown id). Bodies are comment-editor BlockNote blocks,
  cleaned server-side (paragraphs; text with bold/italic/underline/strike/code, safe links, `mention { userId, name }`;
  ≤ 10 000 chars); `body_text` for plain text. Mentions come from the body and count only for members who can see the page
  (every member for teamspace pages, only the owner for private pages); newly mentioned ones (never the author, once per
  comment) get an Inbox notification. The Inbox lists "You were mentioned in a comment … on “<page>”" and opens
  `/docs/<page>?thread=<id>` (Comments panel open, thread selected); entries hide while the page is trashed or out of the
  recipient's reach, and go when the comment is deleted. Every change is audited (`page.thread_created|resolved|reopened|
  deleted`, `page.comment_created|updated|deleted`), so other viewers refresh through workspace events.
  Anchors: BlockNote's `comment` mark (`threadId`, `orphan`) inside the collaborative document (y-prosemirror key
  `comment--<hash>`), set by the author's editor; the JSON projection never contains it (Rust converter ignores it; golden
  `tests/fixtures/collab/marks/comment_marks.json`). Every collaborative editor loads `CommentsExtension` (without the
  mark y-prosemirror would drop the anchored text) and mounts only after the threads query settled (an empty store would
  flip every anchor to orphan). Resolved/deleted threads keep their mark as `orphan` (no highlight; reopen restores it).
  UI (Orbit components on BlockNote's comment data flow; BlockNote's stock thread/composer/sidebar are not used): select
  text → "Comment" in the formatting toolbar → floating composer: a textarea-style field (2–8 lines, "Add a comment… Use @
  to mention", "@" button opening the member picker), Cancel + primary "Comment" (disabled while empty); Enter is a new
  line, Cmd/Ctrl+Enter sends (also replies and edits), Esc cancels. Thread card (floating next to a clicked highlight, or
  in the panel): quoted anchor text on top (amber left border, 2 lines), Resolve/Reopen icon button, comment rows (avatar,
  name, relative time, "edited", kebab with Edit/Delete on own comments), "Resolved by X · time" on resolved threads
  (dimmed, no reply field), a one-line "Reply…" field that expands on focus. Header comment button with an open-count
  badge toggles the "Comments" panel (segmented Open / Resolved tabs with counts, empty states; cards in document order,
  anchorless threads last and labelled "Text removed"; clicking a card scrolls to and highlights its anchor; while the
  panel is open, threads show there instead of floating cards). Anchors: amber wash + underline, stronger when selected
  (light and dark). Comment placeholders are static CSS (CSP). No reactions. REST content writes, version restores and
  converter rebuilds replace the document without marks (their threads show as "Text removed" in the panel). E2E:
  `target/orbit-smoke/comments.mjs`; visual check `target/orbit-smoke/comments-ui.mjs` (dark + light screenshots).
- @mentions in the page body (migration 0032: `notifications` rebuilt with kind `page_mentioned` and optional
  `page_block_id`): inline content `mention { userId, name }` (`name` = snapshot for export/fallback) in
  `pageEditorSchema` (`editor/MentionInline.tsx`, `PageMentionChip.tsx`, `mentionItems.tsx`, `MentionMenu.tsx`,
  `pageMentions.ts`). Typing "@" at a word start (not inside "a@b", never in code blocks) opens BlockNote's suggestion menu
  (next to "/"): members who can see the page (teamspace page: every active member, yourself last; private page: only you,
  under the hint "Only you can see this private page"), filtered by name/handle/email (≤ 10), avatar initials, keyboard
  navigable; picking inserts a non-editable chip "@Name" (primary text on a subtle primary wash) plus a space. Chips show the
  member's current name, the stored one while members load, "Unknown user" once the member is gone. Works in co-editing:
  the Rust converter (`collab/blocknote.rs`) writes/reads mentions as y-prosemirror atom elements (`inline` section of
  `blocknote-schema.json`; parity golden `tests/fixtures/collab/editor_mentions.json`); `sanitize.rs` keeps only a UUID
  `userId` (lowercased) and a ≤ 100-char `name` (invalid ones become `@name` text in JSON and are removed from live
  documents). `content_text` has "@Name" (search finds mentions); Markdown export writes "@<current name>".
  Notifications (`repositories/page_mentions.rs`): the collab projection and REST content edits (not imports, restores or
  duplicates) diff the mentioned user sets before/after; a user newly mentioned on the page gets `page_mentioned` ("<editor>
  mentioned you in “<page>”", actor = editor, `page_block_id` = first block with the mention) unless they are the editor,
  cannot see the page (private page of someone else, non-member, suspended) or already got one for this page in the last
  10 minutes. Moving or re-projecting a mention never notifies. Each batch is audited `page.mentioned` (workspace events
  refresh the Inbox). The Inbox opens `/docs/<page>?block=<id>`; the page scrolls to and flashes that block, then drops the
  parameter. Notion import: user mentions whose `person.email` matches an active member become `mention`s, others stay
  "@Name" text. `MemberRecord.suspended_at` lets the picker skip suspended members. E2E `target/orbit-smoke/mention.mjs`.
- Links: the editor accepts http(s), mailto and exact in-app page links `/docs/<uuid>` (what the import writes between
  imported pages); clicking one navigates in the app (Cmd/Ctrl/Shift-click opens a new tab). `javascript:`, `data:` and
  other relative paths are dropped.

Not supported yet: video and audio blocks, offline storage of co-edited content (IndexedDB; edits live only in the open
tab while disconnected), presence dots in the page tree, teamspace membership or per-page sharing, comment reactions, templates, export and ZIP import. The production CSP allows external https images (`img-src 'self' data: blob: https:`), so external image
blocks and cover URLs load; the image host then sees the viewer's IP. Plain http images stay blocked.

## Mail

Core files: server `crates/mail` (`orbit-mail`: SMTP, SPF/DKIM/DMARC, MIME, HTML sanitizing),
`apps/server/src/{mail_listener,mail_intake,mail_dns,mail_image_proxy}.rs`, `repositories/mail/`, `mail_routes/`;
web `features/mail/` (`pages/MailPage.tsx`, `mailLib.ts`, `api/`, `components/{FolderRail,ThreadList,ThreadView,
MessageItem,MailHtmlFrame,SafetyBanner,MailSearch}.tsx`), settings in `features/settings/pages/mail/`.
Design: `docs/superpowers/specs/2026-10-23-mail-slice-1-design.md` (local file). Operator guide:
`docs/operations.md#mail`.

Slice 1 receives and reads mail; it does not send. Server (migration `20261111122204_mail`):

- Provider intake (`mail_delivery.rs`, `mail_routes/{inbound,providers}.rs`, `crates/mail/src/inbound/`): PostShiba
  (recommended; signed JSON webhook, then the raw message from its API), Cloudflare, Postmark, Mailgun, Resend and SES post to `/api/v1/mail/inbound/{kind}/{id}`; instance and workspace providers
  (`mail_providers`), a route per domain (`mail_domains.provider_id`), rejected deliveries listed for 30 days.
- SMTP listener on port 2525 when `ORBIT_MAIL_ENABLED=true`, relay mode (default, trusts the relay's
  `Authentication-Results`) or direct mode (own SPF/DKIM/DMARC checks, STARTTLS, certificate reload every 60 s).
- Domains per workspace, verified by `_orbit-verify` TXT (`mail.dns_check` every 10 min while pending, daily after);
  verified → suspended (7-day grace) when the record goes. Root switch **Mail** in Admin → Inbound mail (`instance_settings.mail_enabled`, off by default): off hides
  the app, its menu items, shortcuts and settings (`useMailEnabled`, `MailGate`, from `/api/v1/auth/options`) and the
  workspace mail API answers `403 mail_disabled` (`Call::enter`); mail for existing domains still arrives.
- Personal and shared mailboxes, aliases, `+tag`, catch-all, labels, bulk personal mailboxes.
- Intake stores the raw message as an encrypted blob (per-workspace key) and enqueues `mail.ingest`; ingest threads,
  dedupes per mailbox, indexes search, and sends `thread.updated` on the live socket (topic `mail`).
- DMARC fail → Spam. Trash purged after 30 days (`mail.purge`). HTML is sanitized on each view and served in a
  sandboxed frame with signed URLs; remote images go through the image proxy.
- Admin health card (listener, certificate, client IP, PTR) and a one-time `check-…@domain` test address.
  Failed messages list with Retry.

Web client (`/mail`, sidebar entry with a **Beta** tag; read-only, no compose or reply yet):

- Three panes: the folder rail (Inbox of all mailboxes, each mailbox with its unread count from `GET counts`, Starred,
  Archive, Spam, Trash, labels; with a mailbox open the folders are that mailbox's), a virtual thread list with cursor
  pages, and the reader. Routes `/mail`, `/mail/<folder>`, `/mail/<mailbox>/<folder>`, `/mail/label/<id>`,
  `/mail/starred`, `/mail/search?q=`, each with `/t/<thread>` for the open thread (`parseMailPath` in `mailLib.ts`).
- Rows: senders, subject, snippet, time, label chips, star, unread dot, attachment icon; `x` or the row checkbox
  selects, and a bulk bar archives, moves to spam or trash, restores, marks read or unread, stars and labels.
- Opening a thread marks it read; only the newest message is expanded. HTML shows in a sandboxed iframe from a
  short-lived `view-url` (height from the frame's own `postMessage`, 80–20,000 px); plain text renders as text.
- Banners: Failed authentication (DMARC fail), Lookalike name, Outside reply (`external`), External sender, plain
  text after a parse warning; In Spam (Not spam) and In Trash (Restore). The sender line shows the address and the
  DMARC-aligned domain.
- Remote images stay off until **Load images** (offered on every HTML message whose sender has no preference, since
  the view cannot report how many it removed); **Always for this sender** (only when DMARC passed) saves a sender pref.
- Message menu: Show original (first 1 MB as text), Download `.eml`, Print (the conversation, via `lib/print.ts`).
  Attachments: images open in the viewer (`?inline=1`, PNG/JPEG/GIF/WebP/AVIF), other files download.
- Search box with operator and value completion (`from:`, `to:`, `subject:`, `label:`, `in:`, `is:`, `has:`,
  `before:`, `after:`). Live updates from the `mail` topic; `app/shell/AppShell.tsx` passes `emitMailEvent` and
  `resyncMail` to `ChatProvider`, so chat does not import mail.
- Shortcuts (context `mail`): `j`/`k`, `Enter`/`o`, `Esc`, `e`, `#`, `!`, `s`, `u`, `l`, `x`, `/`, `g s` (starred),
  `g t` (trash), and global `g i` (mail). `/`, `g s` and `g t` shadow the global search, settings and tasks keys on
  the mail page (`Command.shadows`). Mail commands also show in ⌘K.
- Empty states fill the pane: **Set up mail** for managers without a mailbox, "Ask an admin for a mailbox" for
  members, and one text for each empty folder, label, Starred and search.

Not done yet (mail stays **Beta** until slices 1 to 5 are done):

- Checked only by automated tests: no real mail from Gmail or Outlook in direct or relay mode, no real PostShiba,
  Cloudflare, Postmark, Mailgun, Resend or SES account, no click-through of the UI, no `docker build`.
- Mobile has no mail screens: the mail app, its settings, the Mail badge and the PDF preview are web only.
- The Resend download host is not pinned (Resend's docs do not name it); the fetch must be `https`, public and
  without redirects.
- Rejected deliveries for domains that no workspace has are stored but not listed (Admin has no such list).
- Next slices: 2 send (compose, reply, forward, DKIM signing, sending through a provider's SMTP, send queue,
  bounces), 3 spam and safety, 4 client access (IMAP server, SMTP submission, app passwords), 5 migration (import
  from Google Workspace and Fastmail), 6 team inbox, 7 rules and admin.

## Chat

Core files: `features/chat/pages/ChatPage.tsx`, `features/chat/components/*`, `features/chat/api/*`.
Design: `docs/specs/2026-10-01-chat-redesign-design.md` (UI) and
`2026-10-02-chat-backend-design.md` (backend); both are local files.

Chat runs on the server and is on in every build. `lib/chatEnabled.ts` is the one switch that
turns its routes, navigation and shortcuts off.

- One chat space for each workspace: public and private channels, direct messages (1:1 and
  group), shared categories, personal favorites. `#general` has every member.
- Threads are the only reply model (side pane and full view), with follow state for each member.
- Reactions, pins, files, mentions (`@user`, `@channel`, `@here`, `#channel`), search, the
  Unreads and Threads views, a notify level for each conversation. Unreads also has a group for each thread with
  unread replies, followed or not, in a joined conversation that is not muted (`GET …/chat/unread-threads`,
  `unreadGroups` in `lib/sidebar.ts`). Following only decides notifications and the Threads view. In a thread with no
  `chat_thread_members` row, the replies after the later of `chat_members.joined_at` and `threads_read_to` (migration
  0094: the newest message at upgrade) are unread; reading the thread or "Mark all as read" writes a row that does not
  follow. A reply also sent to the channel shows in the channel's group only.
- Unread and mention counts come from the server (counters, not count queries).
- Live updates, typing and presence come over the workspace's live socket
  (`GET /api/v1/workspaces/{id}/live`).
- A mention in a channel makes an Inbox item; reading the channel reads it.
- `features/chat/api/client.ts` is the contract and `httpClient.ts` is its implementation.
- URLs in a message get a card (`components/messages/LinkEmbed.tsx`). The server reads the page
  (`GET …/chat/link-preview`, `apps/server/src/link_preview.rs`): an X post through the FxTwitter API
  (its video plays in the card), a YouTube video through oEmbed (player on click), any other page
  through its Open Graph tags. Only public addresses are read (SSRF). The CSP names the two media
  hosts (`frame-src` YouTube, `media-src` `video.twimg.com`).
- A copy of chat stays on the device (`lib/localCache.ts`, `features/chat/api/persist.ts`): lists and
  the newest messages in IndexedDB (shown at once, then confirmed by the server), images of messages in
  the service worker's cache (`public/sw.js`). Account settings > Storage shows it, limits the images
  and clears it; sign-out clears it.
- Messages: an inline reply quotes a message of the same conversation (`reply_to_id`); a forward is a
  copy of the text and files in another conversation (`POST …/messages/{id}/forward`) and cannot be
  edited. Emoticons (`:D`) become emoji, a typed task ID (`GEN-1`) of a real project shows as a chip
  and a card, a message of only emoji (1 to 30) shows them large, and `<https://…>` is a link with no
  card. A backslash keeps `\:D`, `\:name:` and `\GEN-1` as text.
- Incoming webhooks (`docs/chat-webhooks.md`): a tool posts JSON to a secret URL
  (`POST /api/v1/hooks/chat/{id}/{secret}`, `chat_routes/hooks.rs`) and a message shows in one channel as that
  tool (`message.bot`: name, icon, an "App" badge). The channel's creator and owners and admins manage them in
  Edit channel > Integrations; only a hash of the secret is stored, so the URL shows once. `author_id` of such a
  message is the member who made the webhook, but it is unread for them too, makes no Inbox item, and nobody
  can edit it (`MessageRecord::sender_id`, `lib/bot.ts`). All bot posts go through `ChatRepository::post_as_bot`. A webhook may send Slack Block Kit `blocks` (header, section, context, image,
  divider, markdown, rich_text; `repositories/chat/blocks.rs`, stored in `chat_messages.blocks`): they show in place
  of the body (`MessageBlocks.tsx`; rich text becomes Markdown in `lib/blocks.ts`), and the body is their plain text. The hook is a drop-in for Slack's (`repositories/chat/slack.rs`): mrkdwn becomes
  Markdown, legacy `attachments` become `attachment` blocks, form `payload=` works, unknown block types are left out,
  long text is cut, `icon_emoji` is the avatar (`bot.icon_emoji`), and the answer is plain `ok`. Buttons without a URL are interactive when the webhook has a request URL
  (Integrations > Interactivity): `chat_routes/interactions.rs` sends a Slack-signed `block_actions` payload (3 s, no
  redirects, public addresses only outside development), and the integration answers through
  `/api/v1/hooks/chat/{id}/responses/{token}` (5 uses, 30 min): replace, delete, `in_channel`, or an ephemeral reply
  (live-only `message.ephemeral` event, `EphemeralMessages.tsx`). The signing secret is encrypted with the app key.
  With events on (`chat_webhooks.events`, the Interactivity switch), the webhook is a two-way bot
  (`chat_routes/events.rs`): each member's new message in its channel goes, signed, as a Slack `event_callback`
  (`app_mention` when the text has `@<webhook name>`, else `message`) to the request URL in the background; its URL
  secret reads the channel and threads (`GET /api/v1/hooks/chat/{id}/messages`, `Authorization: Bearer`); and
  `thread_ts` in a post makes a thread reply. A webhook without events only posts. A bot reply adds no avatar to
  the thread summary and makes its creator follow nothing (`after_reply`).
- Custom emoji belong to the workspace (Workspace Settings > Emoji, owners and admins): `:name:` stays
  text in the body and in reactions and becomes the image when it is shown.
- Stickers belong to the workspace too (Workspace Settings > Stickers, owners and admins; 512 KiB, 100 for
  each workspace). The sticker button of the composer sends one at once as its own message
  (`chat_messages.sticker_id`); a deleted sticker shows "Sticker was deleted".
- A lost connection keeps the app on screen (`lib/connection.ts`, `app/shell/ConnectionBanner.tsx`):
  a banner shows while the sockets and core queries reconnect; after a minute "Orbit is unavailable" covers
  the app (`app/ConnectionScreen.tsx`, the animated logo and Retry) until the connection is back. A page
  shows its error state only with no data or on an answer that is not a connection failure.

## Notifications, profile, and settings

- Inbox can mark individual/all notifications read and links to resources (tasks; docs comment mentions open the page
  with the thread; page body mentions open the page at the mentioned block).
- Inbox and Profile remain mock-backed.
- Profile edits the current mock user's name, email, and title.
- Account → Profile changes the email (`POST /api/v1/auth/me/email`, password or a fresh sign-in; a one-use 60-minute
  link to the new address, confirmed on `/confirm-email` via `POST /api/v1/auth/email-change/confirm`; other sessions
  are signed out, the old address gets a notice, and on Orbit Cloud the owned workspaces' Stripe customers get the new
  address through the `billing.stripe_customer_email` job). Taken addresses get the same answer and no email.
- Admin → Users → Manage → Change email (web and mobile; `PUT /api/v1/admin/users/{user_id}/email`, admins only,
  with or without a mail server) sets the new address at once, verified, with no confirmation email. It shares
  `apply_email_change` (`repositories/account.rs`) with the link confirmation: pending email change and password
  reset links are deleted, the account is signed out everywhere (an admin changing their own email keeps the current
  session), Stripe jobs are queued, and the audit event `account.email_changed` has `changed_by: admin` (`self` for
  the link). The old address gets the notice only when a mail server is saved. Errors: `invalid_email`, `same_email`
  (422), `email_taken`, `account_deleted`, `root_account` (409), `user_not_found` (404). The menu shows the action on
  the admin's own row too (only that action); never on deleted accounts.
- Admin → Users on Orbit Cloud never acts silently (`AccountNotice` in `workspace_routes.rs`): Change email, the
  recovery link (`POST …/recovery-link`) and Reset two-factor (`POST …/two-factor/reset`) answer 409 `smtp_required`
  without a mail server (non-admins still get 403), and email the person after the commit (`send_later`, a failed send
  only logs a warning): the old and the new address (`email_changed`, `email_set_by_admin`), the reset link to the
  account (`admin_recovery`, 30 minutes; the answer is 202 `{sent_to, expires_at}` with no `url`), and a two-factor
  notice (`two_factor_reset`). Self-hosted keeps 201 `{url, expires_at}` and its old notices. Web and mobile read
  `billing_enabled`/`email_enabled` from `/api/v1/auth/options`: on Cloud the item is "Send password reset email" and
  shows "Sent to …" (web toast, mobile note) instead of the link dialog, and without SMTP the three items are disabled
  with "Needs a mail server on Orbit Cloud" (mobile `adminActionBlocked`).
- Orbit Cloud always needs a mail server (`repositories/instance_settings.rs`): `DELETE /api/v1/admin/settings/smtp`
  answers 409 `smtp_required` and Admin → Email hides Remove (web and mobile; saving a replacement works), and
  opening registration needs SMTP even with a sign-in provider (409 `email_not_configured`). While Cloud has no mail
  server (right after setup), installation admins see a red banner linking to Admin → Email (web `MailServerBanner`
  above the top bar, mobile above the dock; computed from `billing_enabled` and `email_enabled` in `/api/v1/auth/options`),
  and the server logs one `warn!` at startup. Self-hosted servers keep "SMTP or provider" and can remove SMTP.
- Account → Profile → Delete account (`DELETE /api/v1/auth/me`, typed email + password or fresh sign-in) anonymizes the
  account (`users.deleted_at`, "Deleted user", `deleted-<id>@deleted.invalid`) and removes its sign-in methods,
  sessions, tokens, personal data and memberships; refused with `owns_workspaces` (lists them) and `root_account`.
  Admin → Users shows it as Deleted and cannot reinstate it. Mobile has both flows in Settings → Profile.
- Workspace Settings: General, Members, Sessions.
- Workspace creation, members, invitations, fixed roles, ownership transfer, and sessions use persistent APIs.
- Sessions lists the current account's active devices. It shows device/browser and last active only, with controls to revoke any non-current session.
- Chat administration remains separate at `/chat/settings`.
- Workspace Settings → Advanced → Export (owner/admin, `workspace.export`; also on read-only billing workspaces):
  `POST /workspaces/{id}/exports` queues a `workspace.export` job (one queued/running export per workspace, else 409
  `export_in_progress`; adjustable job limits); `GET …/exports` lists unexpired ones (polled while active);
  `GET …/exports/{export_id}/download` streams the ZIP (owner/admin only, audited `workspace.export_downloaded`).
  The job (`apps/server/src/workspace_export/`) writes `workspace.json`, `tasks/<KEY>-<n>.json` (comments,
  attachments, activity, trash flag), `pages/<Teamspace|Private>/…md` (page-export converter) + `pages/comments.json`,
  `chat/<channel>.json` (public channels plus private channels/DMs the requester is in), `files/<blob>/<name>` and
  `README.txt`, streamed to a temp file, then stored at `exports/{ws}/{id}.zip` in the attachment tier (workspace
  bucket, installation bucket, or `<backups>/exports/`). Retention deletes it 7 days after it is ready. Layout:
  `docs/operations.md` → Workspace export. Mobile has no export screen.

## Mobile presentation

- Tasks, Docs, Mail, Chat, DMs, and Inbox use their own compact headers instead of the global topbar.
- Chat uses compact messages, avatars, attachment grids, date separators, and pinned system notices; header search is an icon that expands when used.
- Docs drop BlockNote's side-menu gutter on phones; the page list and the page are separate screens.
- The mobile app (`apps/mobile`) edits the page body with BlockNote in an Expo DOM component
  (`src/components/docs/PageEditor.dom.tsx`, a web view): `@orbit/editor`'s schema bound to the page's Y.Doc over the
  same collab socket as the web, so both co-edit one document. The socket uses `ticketWebSocket`: the native side
  fetches `POST /auth/socket-ticket` with its bearer token before every connect and reconnect (401 → close 4401).
  BlockNote's own menus are off (`BlockNoteViewRaw`); a native bar above the keyboard (`EditorToolbar.tsx`) sends
  commands (styles, block types, lists, indent, image, divider, undo/redo) through the DOM ref, and the editor reports
  its selection state, caret line and height back. The page scrolls natively (the web view is as tall as its content)
  and the screen scrolls the caret above the bar. Close codes map to `PROBLEM_RESPONSES` (`src/data/page-editor.ts`):
  4409/4423 reload the page and reconnect, 4402 read-only, 4403/4404/4401 stop. Orbit files load through the native
  side as data URLs. Mobile never PATCHes page content (`savePage` takes title, icon and cover only).
- Shared confirmation and input modals remain vertically and horizontally centered at mobile sizes.
