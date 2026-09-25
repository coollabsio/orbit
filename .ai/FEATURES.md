# Feature behavior

## Persistence status

Milestone one persists setup, login and recovery, sessions, workspaces, memberships, invitations, projects, statuses, labels, tasks, comments, attachments, trash, and audit records. Those flows use the generated `/api/v1` client and never substitute mock records after an error.

Docs pages persist too (tree, content, trash, search; see Docs below).

Home, Mail, Chat, direct messages, Inbox, Profile, webhooks, custom emoji administration, typing, and realtime behavior still use isolated frontend seed data. Their routes show a `Mock data` badge in every build. SMTP sending, inbound SMTP, mailboxes, WebSockets, presence, and typing transport are follow-up milestones.

## Global shell

- First sidebar groups Workspace, Personal, and Manage navigation.
- It collapses to a 56px icon rail; state persists in `orbit:sidebar_collapsed`.
- Search opens the command palette. `Cmd/Ctrl+K` toggles it.
- User menu provides Profile and theme selection.
- Mobile uses a bottom dock and a full navigation drawer.
- Topbar breadcrumbs are route-derived in `Topbar.tsx`.
- Theme defaults to dark and persists under `localStorage.theme`.

## Home

`features/home/HomePage.tsx`

- Greets the current user and summarizes assigned tasks, unread mail/chat, and notifications.
- Shows My tasks, Inbox, Recent mail, and Active channels cards.
- Content has extra top breathing room and a restrained staggered entrance.
- The Home topbar contains search, theme, and settings actions only; the global New menu is intentionally omitted.

## Tasks

Core files: `TasksPage.tsx`, `components/TaskList.tsx`, `components/TaskBoard.tsx`, `components/TaskDetail.tsx`, `ProjectSettingsPage.tsx`.

- Project rail: color square, project name, hover settings gear.
- The task-list title is a project picker: All tasks or any project can be selected without opening the project rail.
- List and Kanban layouts share filtering and sorting.
- Sorting: manual, priority, created, updated, title.
- Board cards support cross-status and within-column drag placement with a visible placeholder.
- Inline status and priority changes avoid opening task detail.
- Multi-select displays a bottom bulk toolbar for status, priority, assignee, and labels.
- New task creates an empty record and opens task detail with title autofocus; no creation modal.
- Task detail supports title, description, status, priority, assignees, labels, project, due date, activity, comments, replies, and attachments.
- Description attachments support paste, drop, file selection, image lightbox, compact file chips, and removal.
- Task comments reuse the chat composer, mentions, channel suggestions, markdown, emoji, and attachments.
- Projects, workflows, labels, tasks, comments, and task attachments persist in SQLite.
- Writes include the observed record version. Stale writes restore the server record and expose a conflict prompt.
- Bulk and reorder requests are one atomic request of at most 100 items. Empty unchanged choices are local no-ops.
- Task and project trash have server-backed restore flows and 30-day retention.

### Project workflows

- Statuses are per-project entities, not a fixed enum.
- Each status has category, name, description, color, and order.
- Categories are Unstarted, Started, Completed, Cancelled.
- Project settings can add, edit, delete, and reorder statuses inside a category.
- General project settings use a local draft and bottom unsaved-changes bar.

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
- Editor: BlockNote 0.55 (rich text, markdown shortcuts, nested blocks, drag handles, undo, slash menu), lazy-loaded in its own
  chunk. Custom `page` block links to a page; it greys out as "Missing page" when the target is trashed or gone.
  Slash items "Sub-page" (creates a child page, inserts the link, opens it) and "Link a page" (picker over the tree).
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
  `CollabConnectContext` for tests, `useCollabSession.ts`, `presence.ts`, `PresenceAvatars.tsx`, `palette.ts`; test fake
  `src/test/fakeCollab.ts`): DocEditor opens one Y.Doc + y-websocket provider per page/epoch (yjs + y-websocket load on
  demand, `disableBc`, backoff ≤ 5 s) and mounts `PageEditor` with `collab` (`withCollaboration`, no `initialContent`,
  Yjs undo, caret labels `showCursorLabels: 'activity'`) only after the first sync; both are destroyed on page switch.
  Header status: "Live" (green dot) / "Connecting…" (before the first sync and for the first 2 s of a drop) / "Offline —
  changes will sync" / "Access lost" / "Not syncing". Close handling: 4404/4403 → the editor turns read-only, a toast
  ("“X” was moved to the trash." / "You no longer have access to “X”."), the page leaves the tree and the app goes to
  /docs (not for the user's own "Move to trash", which announces itself); 4409 → refetch the page (new epoch) and
  reconnect with a fresh document; 4401 → the app-wide unauthorized flow (login); 4413 → "too large" banner + read-only
  until "Reload page"; 4400/4426 → "Reload app" banner; 4429 → toast, reconnect after 5 s; 1012/1013 and drops →
  y-websocket retries. Handshakes refused before the upgrade (HTTP 401/404 arrive as 1006) trigger a throttled REST
  check of the page after 2 failures (404 → leave like a trash, 401 → login). `beforeunload` warns only while
  disconnected with local edits typed offline or within 3 s before the drop (and for unsaved title edits).
  Presence: avatars (max 4 + "+n", tooltips, `aria-label` "Also here: …") from awareness, deduped by the server-stamped
  user id, without the own user (other tabs included); colors = `collabColor(user id)`, the same hash and 8 colors as
  `hub.rs::user_color`, so avatar and caret match. Restore from history, Notion import and REST content PATCH arrive
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
  text nodes (no HTML).
- Notion import (`pages/NotionImportPane.tsx`, `components/NotionImportChooser.tsx`, `api/notionImports.ts`,
  `notionImport/selection.ts`; server `notion/import.rs`, routes `/imports/notion`): Documents header "…" → "Import from
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
  live page visible to the user, else 404 exactly like an unknown id. After the upgrade: `v` ≠ 1 → 4426, stale/missing
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
- Links: the editor accepts http(s), mailto and exact in-app page links `/docs/<uuid>` (what the import writes between
  imported pages); clicking one navigates in the app (Cmd/Ctrl/Shift-click opens a new tab). `javascript:`, `data:` and
  other relative paths are dropped.

Not supported yet: video and audio blocks, offline storage of co-edited content (IndexedDB; edits live only in the open
tab while disconnected), presence dots in the page tree, teamspace membership or per-page sharing, comments, templates, export and ZIP import. The production CSP allows external https images (`img-src 'self' data: blob: https:`), so external image
blocks and cover URLs load; the image host then sees the viewer's IP. Plain http images stay blocked.

## Mail

Core files: `MailPage.tsx`, `components/FolderRail.tsx`, `components/ThreadList.tsx`, `components/ThreadView.tsx`.

This feature is mock-backed. No outbound or inbound SMTP service runs in milestone one.

- Three-pane desktop layout: folders, thread list, reader.
- Custom folders can be created inline.
- Threads can be dragged onto system/custom folders with a compact drag ghost.
- Reader toolbar supports star, unread, move-to-folder, archive, and trash.
- Messages expand/collapse and offer Reply and Forward.
- Forward opens compose with quoted sender/date/subject/body.
- Reply composer is hidden until Reply is clicked; it is free-flow rather than boxed.
- Replies support text, attachment-only sending, file picker, paste, drop, compact file chips, inline images, removal, discard, and Cmd/Ctrl+Enter.

## Channel chat

Core files: `ChatPage.tsx`, `components/ChannelSidebar.tsx`, `components/ChatArea.tsx`, `components/Message*`, `components/Thread*`.

This feature is mock-backed. WebSocket delivery, replay, presence, and typing transport are deferred.

- Resizable category/channel sidebar; width persists.
- Editable/reorderable categories and channels with emoji.
- Timeline groups messages, supports compact consecutive rows and day separators.
- Markdown supports headings, lists, quotes, inline/fenced code, links, mentions, channel mentions, custom emoji, embeds, and GitHub link chips.
- Channel mentions navigate in-app; internal application links use client navigation.
- Hover toolbar intentionally shows only three quick reactions plus `…`; the context menu owns all other actions.
- Add Reaction opens the full shared emoji picker.
- Reactions, replies, edit/delete, pin/unpin, copy, threads, attachments, image viewer, files view, search, and typing indicators work.
- Thread roots are messages; followed threads appear under channels.
- Chat Settings replaces the channel sidebar and contains Roles, Webhooks, and Emoji. There is no chat Danger Zone.
- Custom emoji can be uploaded, renamed, deleted, selected, and rendered app-wide.
- On mobile, `/chat` is the channel list and a conversation's Back button returns there; desktop still opens the first channel automatically.

## Direct messages

`features/chat/DMPage.tsx` deliberately reuses channel-chat components.

This feature is mock-backed.

- DM entry lives under Personal in the first sidebar.
- Resizable conversation sidebar persists under `orbit:dm_sidebar_width`.
- Rows show avatar, fixed online/offline status dot, latest preview/time, active state, and unread count.
- New message modal searches users by name/handle and intentionally has no status dots.
- DM header shows only avatar, presence, and user name—never profile title/description.
- Conversation behavior reuses messages, composer, attachments, emoji, reactions, replies, search, files, pins, and threads.

## Notifications, profile, and settings

- Inbox can mark individual/all notifications read and links to resources.
- Inbox and Profile remain mock-backed.
- Profile edits the current mock user's name, email, and title.
- Workspace Settings: General, Members, Sessions.
- Workspace creation, members, invitations, fixed roles, ownership transfer, and sessions use persistent APIs.
- Sessions lists the current account's active devices. It shows device/browser and last active only, with controls to revoke any non-current session.
- Chat administration remains separate at `/chat/settings`.

## Mobile presentation

- Tasks, Docs, Mail, Chat, DMs, and Inbox use their own compact headers instead of the global topbar.
- Chat uses compact messages, avatars, attachment grids, date separators, and pinned system notices; header search is an icon that expands when used.
- Docs drop BlockNote's side-menu gutter on phones; the page list and the page are separate screens.
- Shared confirmation and input modals remain vertically and horizontally centered at mobile sizes.
