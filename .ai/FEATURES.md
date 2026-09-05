# Feature behavior

## Persistence status

Milestone one persists setup, login and recovery, sessions, workspaces, memberships, invitations, projects, statuses, labels, tasks, comments, attachments, trash, and audit records. Those flows use the generated `/api/v1` client and never substitute mock records after an error.

Home, Docs, Mail, Chat, direct messages, Inbox, Profile, webhooks, custom emoji administration, typing, and realtime behavior still use isolated frontend seed data. Their routes show a `Mock data` badge in every build. SMTP sending, inbound SMTP, mailboxes, WebSockets, presence, and typing transport are follow-up milestones.

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

Core files: `DocsPage.tsx`, `components/DocTree.tsx`, `components/DocEditor.tsx`, `components/BlockEditor.tsx`.

This feature is mock-backed and does not persist across a reload.

- Hierarchical page tree with create, delete confirmation, deep subtree deletion, reorder, and nesting drag-and-drop.
- Navigable page breadcrumbs.
- Page emoji icons use the shared chat emoji picker, including custom emoji.
- Cover banners support upload/URL, change/remove, and focal-point repositioning.
- Blocks render markdown while idle and become editable on click.
- Slash menu includes text headings, lists, todo, quote, code, divider, page creation/linking, embed, image, and file.
- Mentions reuse chat autocomplete but omit the right-side handle column.
- Pasted/dropped files create media blocks.
- Image blocks fill the editor column and open a lightbox.
- Bare GitHub issue/PR/repository links render compact GitHub-style references through the shared markdown renderer.

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
- Docs use matching compact body typography and scaled headings while retaining block editing and media behavior.
- Shared confirmation and input modals remain vertically and horizontally centered at mobile sizes.
