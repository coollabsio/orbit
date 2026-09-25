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
- Sidebar: 48px "Documents" header (its "+" creates a page in the default teamspace), a "Teamspaces" label with "+" (create
  dialog), one collapsible section per teamspace (hover "+" adds a page there; "…" menu: Rename inline, Delete teamspace for
  owners/admins only — refused with a toast while it has pages or is the last one), then the Private section (lock, hover "+").
  Empty sections show "No pages inside". Collapsed sections persist per workspace in localStorage.
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
- Editor: BlockNote 0.55 (rich text, markdown shortcuts, nested blocks, drag handles, undo, slash menu), lazy-loaded in its own
  chunk. Custom `page` block links to a page; it greys out as "Missing page" when the target is trashed or gone.
  Slash items "Sub-page" (creates a child page, inserts the link, opens it) and "Link a page" (picker over the tree).
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
- Header: breadcrumbs prefixed with the space (teamspace name, or a lock + "Private"; the mobile top bar too), save status
  (Saving… / Saved / Save failed + Retry / Conflict), page menu with "Move to" (teamspaces + Private, current disabled; moves to
  the end of that space's root) and "Move to trash".
- Title input (empty title shows "Untitled" everywhere), emoji icon picker, cover (upload an image to the page, or an http(s)
  URL; `cover_url` also accepts a file url of the same page only) with remove/change and focal-point repositioning.
- Autosave: title and content debounce 800 ms; icon/cover save immediately; blur, page switch, unmount and tab hide flush.
  One PATCH in flight at a time; each save sends the latest `expected_version`. A 409 shows a banner:
  "Reload page" (discard local edits, load the server page) or "Overwrite" (re-send the local page on the current version).
- Remote updates (realtime refresh, other tabs) replace the document only when there are no unsaved local edits; metadata-only
  changes (e.g. a move) just advance the version.
- Trash: lists pages trashed directly with their space; Restore brings back the subtree trashed with them (to the root of its
  space if the parent is gone).
- Search: the command palette searches page titles and body text (debounced, only for a non-empty query), shows
  "Page · <space>" and opens `/docs/:id`.
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
  When an import fails (e.g. revoked token) or is cancelled, the pages it had created but not filled go to the trash (one
  audited batch per top-level page; a subtree is kept if anything in it has content or is not from the import) and the report
  counts them (`unfilled_pages_trashed`). Filled pages stay.
- Links: the editor accepts http(s), mailto and exact in-app page links `/docs/<uuid>` (what the import writes between
  imported pages); clicking one navigates in the app (Cmd/Ctrl/Shift-click opens a new tab). `javascript:`, `data:` and
  other relative paths are dropped.

Not supported yet: images, files, video and audio blocks (and cover uploads), real-time co-editing and presence, teamspace membership
or per-page sharing, teamspace icons/reordering in the UI, version history, comments, templates, export, ZIP import, and permanent deletion from the page trash. External cover URLs are
blocked by the production CSP (`img-src 'self' data: blob:`) until covers go through the upload layer.

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
