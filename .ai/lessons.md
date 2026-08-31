# Lessons

## Design fidelity
- the chat reference's chat chrome reads clean because of three things: filled (two-tone) icons at 20px in 32px buttons with 8px gaps, a borderless `bg-secondary/40` search field with a 16px right gutter, and neutral member names (white/muted) — role/user colors belong on avatars and message authors only. reicon has `weight="Filled"` for this.
- Second sidebars (mail folders, docs tree, tasks projects, chat channels) all use a 48px `pane-header` title row and 8px body padding; the app sidebar brand row is also 48px so everything lines up. Never let a list start flush at the top of a pane. Empty states fill the whole pane area (`.empty-state-wrap` + `flex:1`).
- **Coolify v4 is flat-first, not card-first.** The app canvas is one flat surface; columns and second sidebars are separated by 1px borders directly on the canvas. Rounded bordered cards are ONLY for form sections and resource tiles inside content — never wrap whole panes/lists/readers in cards. (Corrected by user on 2026-08-21 after I carded every pane.)
- **Orbit uses the purple accent in dark mode too** (user decision 2026-08-29: "styling inconsistencies, use purple"). Do not reintroduce Coolify's yellow dark accent; count badges are solid purple pills like the chat sidebar.
- Coolify v4 accent is theme-aware (source of truth: `core/DESIGN.md` + `.dark { --color-accent: #fcd452 }` in `app.css`): **purple in light mode, yellow in dark mode**; primary/highlighted buttons stay the purple gradient in both. Selection/active nav states are neutral gray fills (white/6%), not accent-tinted. An earlier note here claimed a purple dark accent — it was wrong; verify against the source, never from memory.
- When the user asks for a "1:1 copy" of a reference UI, port the reference CSS values verbatim (surface ladder, paddings, font sizes, grid columns) into our tokens/utilities instead of approximating. Read `resources/css/app.css` + `utilities.css` + the blade of the exact page first.
- Before restyling to match a reference, restudy the actual screenshot section by section (shell, second nav, content, controls) instead of extrapolating a single pattern to everything.

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
