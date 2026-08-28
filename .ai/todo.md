# Frontend (mock data) — build log

Goal: complete the full frontend with mock data, reference-style shell/layout, Coolify visual language, responsive for mobile browsers. Backend comes later.

## Done
- [x] Design tokens (Coolify ladder: purple accent light / yellow accent dark), base styles, shared utility classes
- [x] Mock data layer: types, seed (6 users, 3 projects, 12 tasks, 9 docs, 9 mail threads, 5 channels, 7 notifications), store (useSyncExternalStore), actions
- [x] App shell: sidebar (224px), mobile top bar + drawer + bottom dock, theme toggle, command palette (Cmd/Ctrl+K)
- [x] Tasks: grouped list, tabs, filters (?project= param), detail panel, comments/activity, new-task modal
- [x] Docs: tree sidebar, block editor (10 block types, click-to-edit, todo toggles), breadcrumbs
- [x] Mail: folder rail, thread list, reader with collapse, reply, compose modal, star/archive/trash
- [x] Chat: channel rail + members, grouped messages, day separators, external actors (Discord/GitHub), reactions, replies, composer
- [x] Home dashboard, notification Inbox, Settings (profile, theme, members, feature toggles, about)
- [x] Verified: oxlint clean, tsc clean, vite build clean, headless-browser screenshots of all areas in dark + light + mobile (390px)

## Settings 1:1 Coolify (2026-08-27)
- [x] tokens.css: port Coolify surface ladder (oklch), text/nav/accent tokens for light + dark
- [x] utilities.css: button/input/select/label/menu-item/nav-section/badge/modal/popover/data-table/checkbox to Coolify values
- [x] cards.css: `.layer-card` = `.application-settings-section` (header strip + nested body ring)
- [x] Modal.tsx: Coolify modal shell (section card + X close, footer inside body)
- [x] Settings: sub-nav layout (210px) + routes: General (Profile/Appearance/About), Members (table, invite), Features (checkbox rows)
- [x] mock actions: setUserRole / removeUser / updateUserProfile for Members "Manage" menu + Profile form
- [x] index.html: apply `.dark` before first paint (no light flash; also fixes headless screenshot artifact)
- [x] verify: tsc, oxlint, vite build, headless screenshots dark/light/mobile + modal CSS test page
- [x] Listbox component (Coolify x-forms.listbox) replaces every native <select> (Members, General, NewTask)
- [x] /profile page (Coolify livewire/profile) + sidebar account menu (x-top-user-menu: Profile, Appearance, Log out)
- [x] EmptyState = Coolify x-empty (dashed card + icon tile, sm/base) with pane-centering wrap

## Chat features from the chat reference (2026-08-28)
- [x] Mention autocomplete: @ popup (substring match, ↑/↓ wrap, Enter/Tab accept, Esc close), inserts `@Display Name ` like the chat reference
- [x] Channel + category settings: right-click → Edit Channel (name/topic/emoji) / Edit Category (name/emoji) in Coolify modals
- [x] Reorder: the chat reference mouse-drag DnD with drop lines; channels reorder inside their category, categories reorder globally
- [x] Threads: thread = message (`threadRootId`/`startsThread`/`threadTitle`); Create/Open Thread from toolbar + context menu; ThreadPreview card; resizable ThreadPanel with rename; NewThreadPanel from composer +; Threads popover in header; `?thread=<id>` deep link
- [x] Markdown + code: fenced ``` blocks with copy button + JS/Rust token colors, headings, quotes, lists, bold/italic/inline code/links
- [ ] Not ported: thread follow/unfollow + followed-thread rows in sidebar, full-screen thread route, Discord `<t:>` timestamps

## Webhooks from the chat reference, UI only (2026-08-28)
- [x] Data: `Webhook` + `Embed` types, `webhooks` seed, actions create/update/delete/sendWebhookMessage (mock POST)
- [x] Chat: `authorType: 'webhook'` → amber webhook avatar, green "Webhook" badge, embed cards (accent border, author/title/desc/fields/image/footer/thumb), deletable
- [x] Settings › Integrations › Webhooks: the chat reference WebhooksTab accordion (icon upload, name, channel, Copy Webhook URL, Delete + confirm)
- [x] Settings › Integrations › Webhook Builder: the chat reference builder (destination, message, embed, color presets, fields, timestamp toggle) + live preview + JSON payload; Send posts into the channel

## Polish round (2026-08-29, 10 user items)
- [x] 1 spacing: 48px brand row + 48px `pane-header` on every second sidebar (mail folders, docs tree, tasks projects)
- [x] 2 purple everywhere in dark mode (`--accent` violet, `.count-badge` solid purple like chat)
- [x] 3 empty state fills the pane area
- [x] 4 mail rail: header, spacing, "Custom folders" section (Customers/Invoices/Hiring) + inline "New folder"
- [x] 5 Webhook Builder removed (page, route, nav, CSS, mock send action)
- [x] 6 mock users = Coolify team: ShadowArcanist, Andras, Adiology, Cinzya, Peak (ids/emails/prose renamed)
- [x] 7 /settings/features removed
- [x] 8 chat context menus render through a portal (message rows keep an animation transform, which broke position:fixed)
- [x] 9 docs @mention autocomplete (shared `useMentionAutocomplete` + `MentionPopover`), mention pills in block view
- [x] 10 tasks second sidebar `ProjectRail` (All projects + per-project open counts); project dropdown filter removed

## Chat polish + the chat reference attachments/pins/files (2026-08-29)
- [x] empty state without background; mail rail titled "Mail"; chat rail header "Chat ˅"; /chat opens the first channel
- [x] attachments: mock upload (+ menu, paste, drag & drop overlay), pending strip, image grid + file cards, ImageViewer (zoom/download)
- [x] pins: pinnedAt/pinnedBy, "X pinned a message" notice (right-click → delete), Pins popover with search + jump
- [x] Files view (Back to messages, search, kind/sort listboxes, Media grid, Documents rows, delete)
- [x] header items in the chat reference order: Files, Threads, Pins, Members, inline search (filters the timeline)
- [x] search = the chat reference SearchOverlay side panel (260px resizable, N RESULTS, groups per channel/thread, cards → jump via ?message_id=, ?q= deep link); timeline no longer filtered
- [x] chat header spans the full width; member list renders as ChatArea `rightPanel` under it (the chat reference structure)
- [x] the chat reference polish pass: filled header icons (reicon `weight="Filled"`), 32px buttons at 8px gaps, soft borderless search field with 16px gutter, neutral member names

## Review notes
- Browser-default `text-align: center` on `<button>` leaked into row labels — fixed in base.css reset.
- Headless Firefox `--screenshot` can paint stale CSS variable values (tabs looked purple in dark mode); live computed styles confirmed correct yellow accent. Do not chase that artifact again.
- Mobile master/detail is route-driven: list route vs `:id` route with back buttons at `@media (max-width: 899px)`.

## Later (backend phase)
- [ ] Replace mock store with HTTP API + WebSocket events (Rust/Axum, SQLite)
- [ ] Auth + permissions
- [ ] Global search over real FTS5
