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

## shadcn + Base UI migration (2026-09-19)
- **Render Base UI Popover/menu content only while open.** In our Dropdown wrapper, `{open ? <PopoverContent>…</PopoverContent> : null}`. Closed Base UI popovers still mount their Portal/Positioner and do async work; a list view with hundreds of row-level dropdowns (TaskList: 3 per row × 100+ rows) hangs a render tick if all popover content mounts eagerly. This also matches the old createPortal-on-open behaviour and makes `children(close)` lazy.
- **Base UI popover testing under happy-dom:** `fireEvent.click` OPENS a Base UI trigger (a single click event); `userEvent.click` DOUBLE-toggles it (full pointer sequence → open then close) so the content never appears — use fireEvent to open. For the option/menu item inside, use `userEvent.click(await findByRole(...))` — it waits until the opened popover is actionable (a bare `fireEvent.click` right after opening can no-op because the positioner hasn't settled). Assert open/closed via the trigger's `aria-expanded`, not DOM removal (closed popups linger for an exit animation that never fires in happy-dom). Outside-click dismissal needs `userEvent.click(outside)`, not `fireEvent.pointerDown`.
- **Base UI Popover is modal by default** (inerts the page) — pass `modal={false}` for dropdowns/menus, or the rest of the page becomes inert and content can render oddly.
- **Render the trigger AS the consumer's element** (`<PopoverTrigger render={triggerEl} />`), don't wrap it in a `nativeButton={false}` span — the span adds an extra `role=button` whose accessible name collides with menu items (getByRole ambiguity).
- **Casing collisions:** shadcn primitives are lowercase (`avatar.tsx`); our PascalCase files (`Avatar.tsx`) collide on case-insensitive TS module resolution. Rename our wrapper (e.g. `UserAvatar.tsx`) or the build fails with TS1261.
- **cn is its own package now** (`import { cn } from "cn"`), not `@/lib/utils` internals; `src/lib/utils.ts` just re-exports it. TS6/TS7 deprecate `baseUrl` — use `paths` without it.

## SUPERSEDED (2026-09-19): purple accent
- The earlier notes in this file that insist on the Coolify PURPLE accent (light purple / dark purple, "use purple", brand-purple gradients) are SUPERSEDED. The app migrated to shadcn/ui (Base UI) + Tailwind v4 with preset `b1YmtCU1y` (style base-nova, baseColor neutral, theme PINK, Noto Sans). Per the user's explicit decision ("preset wins entirely"), the accent is now PINK (`--primary`), Coolify purple is retired, and all hand-written CSS/design tokens are gone (only src/index.css remains). Do NOT reintroduce purple. Theme lives in src/index.css :root/.dark; components use Tailwind theme utilities (bg-primary, text-foreground, bg-sidebar-accent, etc.). Branch: shadcn-tailwind.

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
