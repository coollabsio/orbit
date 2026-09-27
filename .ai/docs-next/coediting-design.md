# Docs real-time co-editing: design (spike-backed, 2026-09-25)

> **Decisions by the user (2026-09-25):** (1) server-side Rust converter — yes; (2) projection does not bump
> `pages.version` — yes; (3) audit at most one `page.updated` per page per 10 min for collaborative edits — yes;
> (4) co-editing is **always on — no `ORBIT__DOCS__COLLAB` flag, no off mode** (ignore §14's flag; route always exists,
> web always uses collab for content); (5) max 50 connections per page — yes; (6) no offline IndexedDB in v1 — later.

Spike: `target/coedit-spike/` (standalone Cargo `[workspace]` + bun package, no product code touched).
Reproduce everything with `target/coedit-spike/run-all.sh`. Results: Rust 1/1, bun 15/15 (yjs interop 10, BlockNote e2e 5).

## 1. Proven versions and APIs
- Server: `yrs = { version = "=0.28.0", features = ["sync"] }` (+ axum 0.8 `ws`, sqlx 0.8 sqlite). Used:
  `yrs::sync::{Awareness, Message, SyncMessage, MessageReader}`, `Message::encode_v1()`, `Update::decode_v1`,
  `Doc::observe_update_v1(key, |txn, e| e.update)` (captures exactly the integrated delta; empty for duplicates),
  `txn.encode_state_as_update_v1(&StateVector::default())`, `Awareness::{apply_update, remove_state, update_with_clients}`,
  `ClientID::new(u64)`, `XmlFragmentRef/XmlElementRef/XmlTextRef` + `XmlTextRef::diff(txn, YChange::identity)`,
  `insert_with_attributes`, `insert_attribute(txn, k, Any)`. Tags: 0 sync, 1 awareness, 2 auth, 3 query-awareness.
- Client: `yjs 13.6.33`, `y-websocket 3.1.0`, `y-protocols 1.0.7`, `y-prosemirror 1.3.7` (all four must be added to
  apps/web; they are optional peers of `@blocknote/core` and are not installed today).
- **BlockNote 0.55 has no `collaboration` option on `useCreateBlockNote`** (the todo's stack note is outdated). Use
  `import { withCollaboration } from '@blocknote/core/yjs'` →
  `useCreateBlockNote(withCollaboration({ schema, collaboration: { provider, fragment, user: { name, color }, showCursorLabels?, renderCursor? }, _tiptapOptions: { injectCSS: false } }))`.
  It forces `initialContent = [{ type: 'paragraph', id: 'initialBlockId' }]` and disables `history` (Yjs undo instead).
- Conversions, `@blocknote/core/yjs` (editor may be headless: `BlockNoteEditor.create({ schema })`, never mounted):
  `blocksToYDoc(editor, blocks, fragmentName = 'prosemirror'): Y.Doc`, `blocksToYXmlFragment(editor, blocks, fragment?)`,
  `yDocToBlocks(editor, ydoc, fragmentName = 'prosemirror')`, `yXmlFragmentToBlocks(editor, fragment)`.
  (`@blocknote/core/y` is the `@y/y` 14 variant; do not use.) Fragment name: `prosemirror` (the utils' default).
- y-websocket 3.1: `new WebsocketProvider(serverUrl, room, doc, { params, WebSocketPolyfill, disableBc, maxBackoffTime, shouldReconnect })`,
  URL = `serverUrl + '/' + room + '?' + params`. Close codes **4400–4499 are terminal**: `shouldConnect = false`, event
  `closed {code, reason}`; `provider.connect()` resumes. Other closes reconnect with 100 ms·2ⁿ backoff ≤ `maxBackoffTime`.
  A client closes its socket if it receives nothing for 30 s → the server must echo awareness to the sender too.
- Cursor DOM: `.bn-collaboration-cursor__caret/__label` with inline `style` attributes (allowed by `style-src-attr 'unsafe-inline'`).

## 2. Spike numbers (localhost, release build, NVMe)
| What | Result |
|---|---|
| y-sync keystroke A→server(+SQLite insert)→B, yjs clients | p50 0.12 ms, p95 0.22–0.29 ms, max 1.9 ms |
| same **without TCP_NODELAY** | max 41 ms (Nagle + delayed ACK); awareness removal 43 ms vs 2 ms |
| same with `synchronous=FULL` (product default) | p50 1.7 ms (one fsync per update) |
| BlockNote editor→editor in Chromium (2 tabs) | p50 1.6–1.9 ms, p95 2.6–3.0 ms, max 4.8 ms |
| Keystroke update / awareness message | 20–23 B (+3 B framing) / ~109 B avg (user + cursor) |
| 83-block page (all Notion goldens): JSON / Y state | 33.3 KB / 34.7 KB |
| JSON→Y: JS `blocksToYDoc` / Rust port | 8.4 ms, 34.3 KB / 0.9 ms, 38.4 KB |
| Y→JSON: JS `yDocToBlocks` / Rust port | 2.8–3.5 ms / 0.5–0.8 ms |
| Live replace (Rust, 2 blocks) | 0.35 ms, 556 B update |
| Late joiner (120 blocks), reconnect after restart | 1.6 ms / ~300 ms (backoff) |
| Room load / compaction | ≤0.6 ms / ≤0.4 ms |
| RAM per loaded doc | 590 KiB for the 35 KB page (~17× state); 20k random 1-char edits: 370 KB state, 3.1 MiB; 20k appended chars: 29 KiB |

Hazard proven: two editors bound to an **empty** fragment that type at once **diverge** (both keep `initialBlockId`,
"AAA" vs "BBB") and y-prosemirror logs `RangeError: Position 15 out of range`. An initialized empty page (one
paragraph) converges ("AAABBB", one block). → The server never serves an empty fragment.

## 3. Architecture
- New module `apps/server/src/collab/` (`hub.rs` rooms + sockets, `store.rs` SQL, `blocknote.rs` converter,
  `blocknote-schema.json` generated table). `CollabHub` is created in `router.rs` next to the realtime router and
  shared (Arc) with `PageRepository` callers that replace content.
- Route: `GET /api/v1/workspaces/{ws}/pages/{page_id}/collab` (WebSocket). Client:
  `new WebsocketProvider(`${wsOrigin}/api/v1/workspaces/${ws}/pages/${pageId}`, 'collab', ydoc, { disableBc: true, maxBackoffTime: 5000, params: { v: '1', epoch } })`.
- One room per page: `Mutex<RoomState { awareness(doc), conns, seq, pending batch, dirty_since_projection, epoch }>`
  + `broadcast::Sender`. Connection task = `select!` over socket, room broadcast, 20 s ping (copy of `realtime.rs`).
  On open: SyncStep1(server sv) + all awareness. Handling: SyncStep1 → SyncStep2 reply; SyncStep2/Update → apply,
  take observer delta, queue for persistence, broadcast to others; Awareness → apply, remember client ids of the
  connection, broadcast to **everyone incl. sender**; on close: `remove_state` + broadcast null states.
- Serve with **TCP_NODELAY** (`axum::serve(listener.tap_io(|s| s.set_nodelay(true)), …)`); it is not set today.

## 4. Wire, cookies, Origin, CSP
- y-sync v1 binary frames, unchanged y-websocket client. `v` param = protocol version (4426 if unsupported).
- Cookie: the browser sends the session cookie on the same-origin handshake (SameSite=Lax is fine); handler copies
  `realtime::connect`: session cookie → `authenticate_session` → membership. Origin: the existing `OriginPolicy::permits`
  already requires one allowed `Origin` for any request with `Upgrade` (spike mirrors it: bad Origin 403, no cookie 401).
- CSP: no `connect-src` → `default-src 'self'` applies; Chromium accepted the same-origin `ws:` socket under the exact
  production CSP (no violations from collab; the only ones were BlockNote's placeholder `<style>`, already handled by
  PageEditor.css). CSP3 lets `'self'` match ws/wss; very old Safari did not → verify on Safari, else add
  `connect-src 'self' wss://<public host>` from `ORBIT__HTTP__PUBLIC_ORIGIN`.

## 5. Authorization
- At connect: session valid, member of `ws`, page live (`deleted_at IS NULL`) and `VISIBLE` for the user
  (`page_in_tx`). Otherwise HTTP 401/403/404 before upgrade.
- While connected: event-driven + periodic. `CollabHub::revalidate(page_ids)` is called after commits of page move
  (incl. to someone's Private), trash, purge, teamspace delete, membership removal/suspension, session revoke; each
  connection re-runs the visibility query and closes on failure. Plus a 15 s timer per connection (session expiry).
- Close codes (all terminal for y-websocket): 4401 session gone (UI → login), 4403 no access (moved to private /
  removed), 4404 page trashed/purged, 4409 reset (drop local Y.Doc, refetch, reconnect), 4413 too large, 4429 rate
  limited. Transient: 1012 restart, 1013 overloaded/lagged. Read-only mode: not needed now.
- Awareness is client-controlled: the server rewrites `state.user` to `{ id, name, color }` from the session before
  broadcasting, so names cannot be spoofed.

## 6. Persistence (migration after 0028; bump `SUPPORTED_SCHEMA_VERSION`)
```sql
CREATE TABLE page_collab_docs (          -- one row per initialized page
  page_id TEXT PRIMARY KEY REFERENCES pages(id) ON DELETE CASCADE,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  snapshot BLOB NOT NULL,                -- encode_state_as_update_v1
  snapshot_seq INTEGER NOT NULL,         -- last log seq folded in
  epoch TEXT NOT NULL,                   -- random; changes on reset / backup restore
  converter_version INTEGER NOT NULL,    -- blocknote-schema.json hash/version
  projected_seq INTEGER NOT NULL,        -- last seq written to pages.content_json
  updated_at INTEGER NOT NULL);
CREATE TABLE page_collab_updates (       -- append log since the snapshot
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
  workspace_id TEXT NOT NULL,
  data BLOB NOT NULL, user_id TEXT REFERENCES users(id) ON DELETE SET NULL, created_at INTEGER NOT NULL);
CREATE INDEX page_collab_updates_page ON page_collab_updates(page_id, seq);
```
Plus same-workspace triggers like `page_versions`. Draft/test on a /tmp DB copy first (dev watcher applies on compile).
- Writes: per-room write-behind batch, flushed every 150 ms or 64 updates, as one row (`yrs::merge_updates_v1`) in one
  transaction. Measured: FULL sync costs ~1.6 ms/commit; batching bounds fsyncs per page to ~7/s and keeps SQLite's
  single writer free for the app. Crash window ≤150 ms is recovered from clients: y-sync step 1/2 on reconnect pushes
  whatever the server lacks (proven: edits made while the server was down arrived after restart).
- Compaction (one tx: upsert snapshot at seq N, delete log ≤ N): every 500 updates or 1 MiB of log, and on room
  eviction. Load = snapshot + log in seq order (≤0.6 ms for a real page).
- Backup (`VACUUM INTO`) includes both tables. A restored backup is older than connected/open clients → they would
  re-push post-backup content. Restore therefore rotates every `epoch`; clients send `epoch` (from the page GET) and a
  mismatch closes with 4409. Retention: none needed (compacted); rows go with the page (FK cascade on purge), trashed
  pages keep their state.

## 7. First load: server-side conversion (single initializer)
- Decision: **the server converts `content_json` → Y itself**, lazily, inside the room lock when a page without
  `page_collab_docs` row is first opened (or first replaced). No client claim, no race, no empty fragment ever.
- Feasible and proven: `blocknote.rs` (~540 lines, both directions) ports BlockNote 0.55 `blockToNode` + y-prosemirror's layout
  (`blockGroup > blockContainer{id} > <type>{props} [> blockGroup]`; text runs = one XmlText with marks as formatting
  attrs: boolean styles `{}`, string styles `{stringValue}`, `link {href}`; `hardBreak` elements; tables
  `table > tableRow > tableHeader|tableCell{colspan,rowspan,colwidth,…} > tableParagraph`; `codeBlock` plain text;
  missing props = PM defaults). Editors opened on the Rust-built doc show **exactly** the normalized document for all
  83 Notion-golden blocks (callout, page, tables, lists, links, styles, emoji, hard breaks).
- Schema facts come from `blocknote-schema.json`, generated by a bun test from `pageEditorSchema`
  (`schemaTable()` in the spike: PM attr order, prop flags, PM default per attr, style kinds). A test fails when the
  generated file drifts → BlockNote upgrades and schema changes cannot silently break the converter.
- Fallback (also proven): client builds the update with `blocksToYDoc` and POSTs it; server accepts only if the page
  has no state (first writer 204, second 409). Keep as plan B if the port ever lags a BlockNote upgrade.
- Sanitize on the way in with the same rules as `toEditorContent` (unknown types, unsafe links, file URLs).

## 8. JSON projection (search, history, API, duplicate)
- Decision: **server-side Y → BlockNote JSON in Rust** (port of `nodeToBlock`; proven equal to the live
  `editor.document` after concurrent edits incl. styles, links, `\n`, nested check items, page blocks). Why not
  client-sent JSON: needs a live and honest client, N clients race to write, stale tabs overwrite newer content, and a
  closed tab leaves `content_json` behind the real doc. The server is the single writer and always has the state.
- When: 2 s after the last update, at most every 10 s during continuous typing, on room eviction, and on demand
  (`hub.flush(page)` before duplicate/export/version snapshot). Writes `content_json` + `content_text` through one repo
  function that also runs Part 1's history rule (snapshot previous state if the newest is >10 min old), sets
  `updated_by` (last editor in the batch) and `updated_at`. It does **not** bump `pages.version` (that stays the
  metadata concurrency token) and records at most one `page.updated` audit/outbox event per page per 10 min, so the
  workspace-wide invalidation storm does not happen while people type.
- The projection also re-sanitizes; if the Y doc holds an unsafe link/file URL (a member can write raw Y updates),
  the hub applies a server-authored repair transaction (unwrap link / blank URL), broadcast like any update.

## 9. Server-side replacement (restore / Notion import / API PATCH content / duplicate)
- Part 1's `replace_content(...)` becomes: lock room → `blocknote::replace_blocks` as a normal Y transaction (delete
  fragment children, write the new blocks) → persist the delta + write `content_json` in the same DB transaction →
  broadcast → unlock. Proven: both open editors switch to the restored document **live** (no reload, no close), and
  typing continues. If no room is live and no collab row exists, just write `content_json` (lazy init later).
- REST `PATCH content` stays for API clients and routes through the same function (expected_version still guards).
- Duplicate: `hub.flush(source)` first, then copy `content_json`; the copy initializes lazily.
- 4409 reset (drop state, clients reload) remains only for converter-version changes and backup restores.
- Title / icon / cover stay REST (PATCH + `expected_version`), unchanged.

## 10. Web client
- `PageEditor` gets `collab: { provider, fragment, user }`; DocEditor creates `Y.Doc` + provider per page, waits for the
  first `sync` before mounting (never bind an editor to an unsynced doc), destroys both on page switch.
- Content leaves `PageAutosaver`: no content PATCH, no content conflict banner, `replaceContent` from realtime refetch
  disabled while collab is active. Autosaver keeps title/icon/cover (conflicts become rare; `rebaseOnto` stays).
  Header status: Live / Connecting… / Offline — changes sync when back / Access lost.
- Presence: header avatar stack from `provider.awareness.getStates()` (dedupe by user id, hide self, max 4 + "+n");
  colored carets with names by BlockNote's default renderer, `showCursorLabels: 'activity'`; color = hash(user id) over
  8 theme-safe colors (dark + light).
- Offline/reconnect: edits stay in the Y.Doc and flush via SyncStep2 on reconnect. No IndexedDB persistence in v1 →
  `beforeunload` warning while disconnected with local changes. Terminal closes map to UI (4403/4404 → read-only
  notice + navigate, 4409 → silent reload, 4401 → login).

## 11. Limits
- WS frame/message ≤ 1 MiB (= `PAGE_BODY_LIMIT`), else 4413. Doc state ≤ 8 MiB: check `state + update` before
  applying; refuse with 4413 (never apply-then-roll-back).
- ≤ 50 connections per page, ≤ 20 per user, ≤ 2 000 per process (1013 above). Per connection token bucket:
  200 sync msgs/s, 30 awareness msgs/s (4429). Handshakes count against `general_per_minute`.
- Broadcast channel capacity 1 024; a lagging socket is closed (1013) and resyncs on reconnect.

## 12. Memory, cache, processes
- Room lives while connected; after the last disconnect: flush, project, compact, keep 30 s (tab switches), evict.
  RAM ≈ 17× encoded state (590 KiB for a 35 KB page) → 256 MiB budget ≈ 400 open pages; LRU-evict idle rooms first,
  refuse new rooms (1013) above the budget.
- Single process is guaranteed today (the platform holds an exclusive DB ownership lock), so an in-memory hub is
  correct. Multi-node later needs sticky routing by page id or a pub/sub relay — out of scope.

## 13. Tests
- Rust integration tests with a Rust yrs client over tokio-tungstenite (pattern: `server/tests/rust_client.rs`):
  handshake auth/Origin/visibility, two clients converge, awareness add/remove, persistence + compaction + reload,
  restart recovery, 4403 on move to someone's Private, 4404 on trash, live replace on restore, limits, epoch mismatch.
- Converter parity: a bun test (happy-dom, real `pageEditorSchema`) writes golden pairs for every Notion golden plus an
  editor-exercise doc: `{ blocks, yUpdate (blocksToYDoc), expected (normalize) }`; Rust asserts
  `fragment_to_blocks(yUpdate) == expected` and `yDocToBlocks(rust blocks_to_update(blocks)) == expected` (the latter
  in bun). Regenerated schema table must match the checked-in one.
- Playwright e2e against the Rust-served build with the production CSP: two contexts type both ways, cursor label
  visible, JSON equal, restore applies live, trash closes the other tab, no CSP/console errors.
- Web unit: DocEditor collab wiring with a fake provider (status labels, terminal close handling).

## 14. Rollout
- `ORBIT__DOCS__COLLAB=off|on` (config.rs pattern), default off for one release; `GET page` returns `collab: bool` +
  `collab_epoch`. Off: route 404 and the web uses today's autosave (content_json is always current thanks to projection).
- No bulk migration: pages initialize lazily on first open. If the flag was off and REST edits changed
  `content_json` after `projected_seq`, the next open resets the collab state (fresh lazy init).
- Order: migration + hub + converter/projection behind the flag → web editor wiring + presence → e2e → flag on.

## 15. Risks and open questions
- Converter coupling to BlockNote/y-prosemirror internals (attr encoding, `undefined` attrs arrive as
  `Any::Undefined`, mark order is not preserved by yrs → irrelevant for JSON). Mitigated by generated schema + parity
  goldens; plan B is the proven client-side init. Tables with rowspans are not fully ported in the spike.
- Content ids: duplicate block ids in stored JSON are re-assigned by BlockNote on load (seen with concatenated
  fixtures) → the first projection changes ids; harmless but visible in history diffs.
- Untrusted Y content bypasses `toEditorContent`; server repair (§8) is required before launch.
- Safari CSP `'self'` for wss; reverse proxies must pass WebSocket upgrades and 20 s pings (realtime already relies on it).
- Open: (a) server-side converter (recommended) vs client init/projection; (b) projection should not bump
  `pages.version` — OK?; (c) audit granularity for collaborative edits (1 per page per 10 min?); (d) flag default and
  whether to show presence in the page tree; (e) collaborator cap per page (50?); (f) offline persistence (IndexedDB)
  later or never.
