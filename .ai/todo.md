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

## Review notes
- Browser-default `text-align: center` on `<button>` leaked into row labels — fixed in base.css reset.
- Headless Firefox `--screenshot` can paint stale CSS variable values (tabs looked purple in dark mode); live computed styles confirmed correct yellow accent. Do not chase that artifact again.
- Mobile master/detail is route-driven: list route vs `:id` route with back buttons at `@media (max-width: 899px)`.

## Later (backend phase)
- [ ] Replace mock store with HTTP API + WebSocket events (Rust/Axum, SQLite)
- [ ] Auth + permissions
- [ ] Global search over real FTS5
