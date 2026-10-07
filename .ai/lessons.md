# Lessons

## Migration history
- Do not squash or edit a migration after a local database has applied it. Orbit checks both version and checksum at startup. If an uncommitted migration must be consolidated, compare the old and new schemas and back up the database before reconciling its migration records; do not reset user data to make the dev server start.
- SQLite table rebuilds (to change a CHECK): the runner holds a transaction with `foreign_keys=ON`, so `PRAGMA foreign_keys=OFF` is a no-op and `ALTER TABLE … RENAME` fails on triggers that name the table. Copy rows aside, `DROP`, re-`CREATE` under the same name, re-insert under `PRAGMA defer_foreign_keys=ON`, then recreate the table's own indexes and triggers (see `0021`).
- `scripts/dev-server-watch.sh` reruns `migrate run --seed` against `data/orbit.sqlite` on every source change, so a new migration is applied to the dev DB (and its checksum frozen) the moment it compiles. Get the SQL right in tests (`embedded_through`) before any `cargo build` of the server while the watcher runs (0023 was applied this way on 2026-09-25).
- A new migration also needs: its entry in `crates/platform/src/db/migrate.rs` (else every query on the new table returns 500), `SUPPORTED_SCHEMA_VERSION` in `crates/platform/src/backup.rs` (restore refuses newer snapshots), and the version number in `crates/platform/tests/{database,backup}.rs` and `apps/server/tests/app_smoke.rs`. A new route also needs the path and operation counts in `apps/server/tests/openapi_contract.rs` and `just api`.
- A rebuilt table becomes the *newest* FK child, and SQLite runs parent-delete cascades newest-child first. A `RESTRICT` reference to the rebuilt table (e.g. `tasks.status_id`) can then block a cascade that used to work. Hard deletes must delete the restricting children explicitly first.

## SQLite 3.46.0 (bundled by sqlx 0.8) FTS5 integrity false positive
- `PRAGMA integrity_check` reports "malformed inverted index for FTS5 table …" after a row of an FTS5 table is UPDATEd twice
  (or after `'rebuild'`), although queries are right and FTS5's own `INSERT INTO t(t) VALUES('integrity-check')` passes
  (Python's SQLite 3.46.1 also says ok). It made the weekly integrity job (which runs at App start in tests) stop the
  server once `page_search` rows were updated. `IntegrityService::full` now confirms such findings with FTS5's own check
  (2026-09-25). Check new virtual tables against `integrity_check` with the bundled SQLite, not the system one.

## API retry load
- A failed `invalidateQueries` call can refetch many active queries. Do not retry it on a short fixed UI timer; use bounded backoff so an API error cannot exhaust the shared rate limit.

## Design fidelity
- **Code must read like shadcn/ui** (user, 2026-09-29): inline Tailwind or a small component with `data-slot`/`cva`. Never add a `const fooClass = '…'`, a class helper function, or an app CSS class; style library DOM with arbitrary variants from our wrapper. Rules: `apps/web/README.md`.
- The accent is pink (`--primary`) in both themes; Coolify's purple/yellow accent notes are obsolete since the shadcn migration.
- the chat reference's chat chrome reads clean because of three things: outlined icons at 20px in 32px buttons with 8px gaps (all outline, one reicon family; state uses color, not fill — user, 2026-10-02), a borderless `bg-secondary/40` search field with a 16px right gutter, and neutral member names (white/muted) — role/user colors belong on avatars and message authors only. reicon's default weight is Outline; reicon `Pin` is a map marker, the pushpin is `PinTack`.
- Second sidebars (mail folders, docs tree, tasks projects, chat channels) all use a 48px `PaneHeader` title row (`components/common/Pane.tsx`) and 8px body padding; the app sidebar brand row is also 48px so everything lines up. Never let a list start flush at the top of a pane. Empty states fill the whole pane area (`flex-1`).
- **Coolify v4 is flat-first, not card-first.** The app canvas is one flat surface; columns and second sidebars are separated by 1px borders directly on the canvas. Rounded bordered cards are ONLY for form sections and resource tiles inside content — never wrap whole panes/lists/readers in cards. (Corrected by user on 2026-08-21 after I carded every pane.)
- When the user asks for a "1:1 copy" of a reference UI, port the reference CSS values verbatim (surface ladder, paddings, font sizes, grid columns) into our tokens/utilities instead of approximating. Read `resources/css/app.css` + `utilities.css` + the blade of the exact page first.
- Before restyling to match a reference, restudy the actual screenshot section by section (shell, second nav, content, controls) instead of extrapolating a single pattern to everything.

## Orbit is multi-app
- Features that belong to one app (task views, task presets) nest under that app in the sidebar and headers; never add them as top-level Workspace items next to Tasks/Docs/Mail/Chat (user, 2026-09-25).
- Contextual actions (e.g. "Save view") appear only when they can do something; do not show them on an unchanged page.
- Settings that belong to the person (profile, notifications, sessions, keyboard shortcuts) go in Account settings (`/profile/*`, `AccountLayout`), each as its own page. Workspace Settings (`/settings/*`) has only what belongs to the workspace (user, 2026-10-03).

## Reference discipline
- **When the user names a screen by the reference's name, build that exact screen at the reference's mount point — not a lookalike inside an existing page.** "Server settings" in the chat reference is its own route whose sidebar replaces the channel sidebar, opened from the server dropdown; I first put its tabs into the app-wide Settings page and had to redo it (2026-08-29). Also copy data semantics, not just markup: the member list groups by primary role and names take the highest role's color.
- **Check the reference's component tree, not just its classes.** the chat reference's `ChatArea` renders the member list as `rightPanel` *inside* the chat column under a full-width header (`header` + `flex min-h-0 flex-1` row). I ported the member list as a sibling pane, so the header stopped short of the right edge; the user had to point it out (2026-08-29). When porting a screen, first grep where each panel is mounted (`grep -n "<MemberList\|rightPanel"`) before styling.
- **Always study the assigned reference repo BEFORE building a feature, not after.** Chat was built generically without reading `the chat reference`; the user caught it. The brief maps each feature to a reference (Tasks/Mail→the tasks reference, Docs→the docs reference, Chat→the chat reference) — dispatch an Explore agent on the reference first and implement from its spec.
- Layer/nested two-tone card surfaces are a reference pattern, not Coolify. Coolify cards = one surface + one border.

## Tooling
- The user-run Vite dev server (port 8888) can serve a broken module graph after files are renamed (`.tsx`→`.ts`) mid-session — pages render blank with no console errors. Verify against `aubr build` + `aubx vite preview` before debugging app code, and report the dev server needs a restart instead of touching it.
- Probing dev-mode React modules via `import()` from a bare page fails with "@vitejs/plugin-react can't detect preamble" — that's a probe artifact, not an app error.
- Headless Firefox `--screenshot` can paint stale CSS custom-property values (showed light-theme values inside dark theme). Trust a beacon probe of `getComputedStyle` over screenshot colors before "fixing" color bugs.
- Headless Firefox `--screenshot` does not wait for `loading="lazy"` images inside a scroll container (they show alt text). Data-URL images should load eagerly anyway; for real URLs, accept the artifact or test on a bare page.
- Headless Firefox `--screenshot` captures CSS animations at frame 0: anything with an enter animation (modal fade, `slide-in-right` panels) is invisible/off-screen in the PNG. Verify such UI with a static test page that links the built CSS and sets `*{animation:none!important}`; a 50%-black backdrop is also invisible on the near-black canvas, so do not read its absence as "not rendered".
- Browser-default `text-align: center` on `<button>` inherits into child spans — the base reset must set `text-align: left` on buttons.
- The production CSP is `style-src 'self'`: a library that adds a `<style>` at runtime (Sonner, Mermaid) works on the Vite dev server and loses its styles in production. Put the rules in `index.css` or a constructed style sheet (`document.adoptedStyleSheets`), and test a production build served with the server's CSP header.
- Typecheck the web app with `bunx tsc -b` (what `build` runs). `tsc --noEmit -p .` checks nothing: the root `tsconfig.json` only has references, so it passes on broken code.
- `cargo test --workspace` stops at the first failing test binary; use `--no-fail-fast` to see the whole suite.
- Playwright rewrites the tracked `apps/web/test-results/.last-run.json` on every run — restore it with `git checkout --` and never commit it.
- If Playwright's pinned headless shell is not installed, set `PLAYWRIGHT_CHROMIUM_EXECUTABLE` to an installed one under `~/.cache/ms-playwright/` (`playwright.config.ts` reads it) instead of downloading browsers.
- Under bun + happy-dom, a failing `toBeNull()` on an element or a failing `waitFor` on a full page can exhaust a 4 GB memory cap with no output. Assert negatives by count and keep rendered page tests focused.
- happy-dom has no layout, so a TanStack Virtual list mounts no rows in tests. Put `data-virtual-scroller` on the scroll element: `src/test/dom.ts` gives it a height (and each `data-index` line 1px). Positions in such tests are line indexes in px.
- Tailwind v4 emits variant rules in its own order, not class order: two `data-[…]:` variants that set the same property (e.g. `top`) at equal specificity can resolve the wrong way. Make such variants mutually exclusive instead of relying on override order.

## HTML5 drag and drop: never unmount the drag source on dragstart
- Symptom: kanban cards went invisible while dragging and no drop indicator appeared.
- Cause: `dragstart` set state that filtered the dragged card out of the column; the browser cancels a drag when its source element leaves the DOM (no `dragover`/`drop` fire).
- Rule: keep the source mounted (fade it with a data attribute) and render the placeholder among the *other* items; only unmount after `drop`/`dragend`.

## Never kill processes by name; only kill PIDs you started
- `pkill -f firefox` killed the user's real Firefox during headless screenshot verification.
- Rule: start background helpers with `cmd & echo $!`, keep the PID, and `kill <pid>` that PID only. Never pkill by a generic name (firefox, node, vite).
- For headless Firefox screenshots: the `--screenshot` invocation exits on its own; wrap in `timeout N` instead of killing.

## Package-manager choice
- The current checkout was installed with **aube** (lockfile: `apps/web/aube-lock.yaml`; virtual store at `~/.cache/aube`), so existing automation commonly uses `aube run <script>` / `aube exec <bin>`.
- **Aube is optional.** The project is an ordinary Vite package and can be switched to Bun or pnpm at any time. Use one package manager consistently, generate its lockfile, and remove the obsolete lockfile in the same migration.

## shadcn + Base UI migration (2026-09-19)
- **Render Base UI Popover/menu content only while open.** In our Dropdown wrapper, `{open ? <PopoverContent>…</PopoverContent> : null}`. Closed Base UI popovers still mount their Portal/Positioner and do async work; a list view with hundreds of row-level dropdowns (TaskList: 3 per row × 100+ rows) hangs a render tick if all popover content mounts eagerly. This also matches the old createPortal-on-open behaviour and makes `children(close)` lazy.
- **Base UI popover testing under happy-dom:** `fireEvent.click` OPENS a Base UI trigger (a single click event); `userEvent.click` DOUBLE-toggles it (full pointer sequence → open then close) so the content never appears — use fireEvent to open. For the option/menu item inside, use `userEvent.click(await findByRole(...))` — it waits until the opened popover is actionable (a bare `fireEvent.click` right after opening can no-op because the positioner hasn't settled). Assert open/closed via the trigger's `aria-expanded`, not DOM removal (closed popups linger for an exit animation that never fires in happy-dom). Outside-click dismissal needs `userEvent.click(outside)`, not `fireEvent.pointerDown`.
- **Base UI Popover is modal by default** (inerts the page) — pass `modal={false}` for dropdowns/menus, or the rest of the page becomes inert and content can render oddly.
- **Render the trigger AS the consumer's element** (`<PopoverTrigger render={triggerEl} />`), don't wrap it in a `nativeButton={false}` span — the span adds an extra `role=button` whose accessible name collides with menu items (getByRole ambiguity).
- **Casing collisions:** shadcn primitives are lowercase (`avatar.tsx`); our PascalCase files (`Avatar.tsx`) collide on case-insensitive TS module resolution. Rename our wrapper (e.g. `UserAvatar.tsx`) or the build fails with TS1261.
- **cn is its own package now** (`import { cn } from "cn"`), not `@/lib/utils` internals; `src/lib/utils.ts` just re-exports it. TS6/TS7 deprecate `baseUrl` — use `paths` without it.

## Prefer registry components over hand-maintained wrappers
- After a shadcn migration, converting raw HTML to Tailwind classes is only half the job: also check whether the registry
  already has the component we hand-maintain (`shadcn search @shadcn -l 100`). The user had to point this out for the
  command palette (2026-09-20).
- Concretely replaced: custom `Dropdown` wrapper → `DropdownMenu`/`Popover`, custom `Listbox` → `Select`,
  hand-rolled command palette → `Command`/`CommandDialog`, CSS spinner → `Spinner`, ⌘K spans → `Kbd`,
  icon+input search boxes → `InputGroup`, label+control rows → `Field`.
- Keep custom only where the primitive fights the interaction: 2-D emoji grid, slash menu that must keep the caret in the
  editor, @mention list that must not take focus.

## shadcn layout primitives carry `w-full` — override it when replacing content-sized markup
- The tasks top-bar search stretched across the header after the `InputGroup` migration: `InputGroup`'s base class list contains
  `w-full`, while the old `<label>` wrapper had no width and sized to `min-w-[180px]` (2026-09-20, user reported).
- `Input`, `Textarea` and `SelectTrigger` carry `w-full` too. When porting a flex-row control, diff the old wrapper's width
  classes against the primitive's base and add an explicit `w-auto`/`w-56`/`flex-1` override; `cn()` then drops `w-full`.
- Guard it with a source-level assertion in the layout tests, since the failure is invisible to tsc and jsdom-style tests.

## Verifying layout without a dev server
- Chromium for Playwright is installed at `~/.cache/ms-playwright/chromium_headless_shell-1234/...`, but `playwright-core`
  expects a newer revision, so pass `executablePath` explicitly to `chromium.launch()`.
- Working recipe: `vite build`, then `page.setContent()` with `<style>` = `dist/assets/*.css` and a small markup reproduction,
  then `getBoundingClientRect()` on before/after variants.
- **Build the class string with the real `cn()`** (import it from node_modules). Hand-concatenating base + override classes
  lets both survive, and the stylesheet order decides the winner, so the probe silently disagrees with the app.

## Production CSP blocks injected `<style>` tags
- Production CSP is `style-src 'self'` (`crates/platform/src/http/security.rs`). Libraries that inject a `<style>` tag at runtime (Tiptap/BlockNote `injectCSS`, sonner toasts) break silently in production but work in dev (Vite serves no CSP). Sonner: import `sonner/dist/styles.css` in `components/ui/sonner.tsx`. Verify against the Rust-served build (`include_dir!` embeds `apps/web/dist` at compile time: `bun run build`, then rebuild/run the server) and count `[data-…]` rules in `document.styleSheets`. Turn the injection off (`_tiptapOptions: { injectCSS: false }`) and copy the CSS into our stylesheet. `img-src` is `'self' data: blob: https:` since 2026-09-25: external https images (image blocks, cover URLs) load, plain http ones do not. Tradeoff: the image host sees every viewer's IP and user agent (tracking pixels work); uploaded page files stay same-origin.

## Bun + happy-dom: never let `expect(domNode)` fail inside `waitFor`
- A failing `expect(node).toBeNull()` makes Bun format the whole happy-dom node (it links to the window) for the error. It costs ~40 ms alone and ~750 ms late in the full run, and `waitFor` repeats it on every poll, so tests time out only in the full suite (SessionsPage, 2026-09-25).
- Use `waitForAbsence(() => view.queryBy…)` from `src/test/waitForAbsence.ts`, or throw a plain `Error` in the callback.

## Parallel agents: format only your own files
- `cargo fmt --all` rewrote another agent's in-progress files under `apps/server/src/notion/` (2026-09-25). While someone
  else works in the tree, run `cargo fmt --check` and `rustfmt` on your own files instead of formatting the workspace.

## Base UI Input in tests and dark-mode checkbox states
- `fireEvent.change` and `userEvent.clear` do not reach a *controlled* Base UI `Input` / `InputGroupInput` under happy-dom
  (the DOM value changes, React state does not). Use `userEvent.type(input, text)` and clear with backspaces
  (`'{Backspace}'.repeat(value.length)`) (Notion import pane, 2026-09-25).
- shadcn checkbox fills need a `dark:` twin (`dark:data-checked:bg-primary`) to beat `dark:bg-input/30`; a new state such as
  `data-indeterminate` needs `dark:data-indeterminate:bg-primary` too, or it renders as an outline in dark mode only.


## Realtime refresh pauses while a draft is focused
- `useWorkspaceEvents` skips query invalidation while an input/textarea/contenteditable is focused (protects form drafts).
  Surfaces that are focused almost all the time must merge remote updates themselves and opt out with
  `data-realtime-safe` (Docs `DocEditor`), or they never refresh while someone types — the other user's title change
  never arrived and the next save hit a 409 (2026-09-25). Portalled dialogs/popovers stay outside the marker.

## Print CSS: cascade layers and BlockNote's `.dark`
- Unlayered `!important` loses to layered `!important` (important order reverses layer priority). Tailwind `…!` utilities
  and the base `max-md:text-[16px]!` input rule beat any plain `@media print { … !important }` override; the page title
  printed at 16px because A4 print width is under `md`. Put such overrides in `@layer base` with a more specific
  selector (docs export, 2026-09-25).
- BlockNote puts the scheme class (`dark`) on its own containers, so `.dark { --foreground … }` re-applies there: a
  light-theme override on `<html>` alone left white editor text on white paper. Override `html.x, html.x .dark`.

## Opening detail views without flicker
- Do not show a "Loading…" boundary between a list and a detail view, and do not let detail sections arrive one by one (a late query that toggles read-only/editable remounts the fields). Put the detail queries in shared `queryOptions` factories, prefetch them all on open and navigate when they resolve (short cap, e.g. 300ms); on a direct URL load render a blank canvas until every detail query settles (`features/tasks/api/tasks.ts` `prefetchTaskDetail`, 2026-09-26).

## Brand mark
- The logo is the pink pixel "O" in `apps/web/public/logo.svg` (11x11 grid, pink `#f2458f`, shadow `#742f4d` 1 unit down-right). `favicon.svg`, the PNG app icons and the launch-video `OrbitMark` copy it; change them together. The UI `--primary`/`--sidebar-primary` tokens (both themes) use the same pink, `oklch(0.659 0.216 358.989)`, with white foreground. Show it at multiples of 11px (22, 44…) so pixels stay crisp.

## Permissions
- Every authorization rule lives in `crates/orbit/src/policy.rs`: `Policy::can(role, Permission)` for role rules, methods on `Actor` for rules that depend on a record (author, owner, target member). Repositories get the `Actor` from `repositories/membership.rs` inside their own transaction; they never compare role names or write their own role SQL.
- The web app holds no rules. Workspace abilities come from `workspace.permissions` (`useCan`), installation admin from `useIsInstallationAdmin`, and per-record abilities from `can_*` flags on the record (`can_edit`, `can_delete`, `can_purge`, ...). A new gated action is a new `Permission` or `Actor` method, a flag on the record if the UI needs it, then `just api`.
- Build workspace fixtures in web tests and e2e mocks with `testWorkspace(role)` (`src/test/workspace.ts`); its role table is generated from the server (`api/generated/rolePermissions.ts`). Hand-written literals passed to `setQueryData` or `Response.json` are untyped, so a new required field breaks them only at run time.
- Cookie sessions are read only through `auth_routes::request_session`.
