# Orbit web

React 19 + TypeScript + Vite, with Tailwind v4 and shadcn/ui on **Base UI** (preset `b1YmtCU1y`, style `base-nova`).
The React Compiler is enabled — see the [React Compiler docs](https://react.dev/learn/react-compiler).

```bash
aube run dev      # dev server
aube run build    # tsc -b && vite build
aube run test     # bun test src
aube run lint     # oxlint
```

## Folder structure

```
src/
  app/                  composition root: App.tsx (routes), Providers.tsx, auth session wiring
    shell/              app chrome: AppShell, Topbar, SidebarNav, CommandPalette, …
  api/                  HTTP client, query keys, problem/pagination helpers
    generated/          openapi-ts output — never edit by hand
  components/
    ui/                 shadcn registry components (lowercase file names)
    common/             app-wide composites built on ui/ (Modal, EmptyState, UserAvatar, …)
      icons/            hand-drawn icons that lucide does not cover
  features/<feature>/
    pages/              route components (one per route, named *Page.tsx)
    components/         UI used only by this feature
    api/ or api.ts      queries, mutations and their models
    <feature>Lib.ts     pure helpers for the feature
  lib/                  framework-agnostic helpers (format, theme, viewport, utils)
  mock/                 in-memory backend used by the mock-backed routes
  test/                 test setup
  index.css             the only stylesheet: Tailwind theme, tokens, base layer
```

## Conventions

- **Styling reads like shadcn/ui.** Styles are Tailwind classes inline in JSX (`cn()` from the `cn` package, like the registry), or inside a small
  component with `data-slot` and `cva` variants (see `components/ui/button.tsx`). No `const fooClass = '…'` variables, no class
  helper functions, no app-owned CSS classes, no CSS-in-JS. A repeated pattern becomes a component (`components/common/Pane.tsx`).
- **Use shadcn parts with their default look** (`DialogFooter`, `DropdownMenuItem`, `PopoverContent`, `Button`, `Input`, …).
  Pass layout classes only (`w-auto p-0`); do not re-skin them with borders, radii, shadows or colours.
- **Hooks are `data-*` attributes, not classes**: `data-slot` for parts, `data-active`/`data-kind` for state; style them with
  `data-[…]:`, `in-data-[…]:`, `group-data-[…]:` variants. Tests query by role, label or data attribute.
- **Library DOM we do not render** (BlockNote `.bn-*`) is styled from our wrapper with arbitrary variants, the way shadcn's
  `chart.tsx` styles Recharts: see `features/docs/editor/ThemedBlockNoteView.tsx` and `PageEditor.tsx`.
- **`index.css` is the only stylesheet** and holds tokens (`:root`, `.dark`, `@theme inline`), the base layer, `@theme`
  animations, and two blocks that cannot be Tailwind: the verbatim Tiptap base stylesheet (CSP) and print mode.
- **Reach for the registry before writing a component**: `node_modules/.bin/shadcn search @shadcn -l 100`. `components/common/`
  is for app-specific composition over registry parts, not for reimplementing them.
- **Tooltips**: a control gets `<Tip label="…">` (`components/common/Tip.tsx`), not a DOM `title`. Passive text (timestamps,
  truncated names) keeps the native `title`.
- **Inline `style={{}}` is for runtime-dynamic values only** (user colours, pointer coordinates, measured sizes).
- **Imports**: `@/…` across directories, `./x` inside the same directory.
- **Layering**: `app/` → `features/` → `components/`, `lib/`, `api/`. Nothing in `components/`, `lib/` or `api/` may import a
  feature; only `app/` (the composition root, which includes the shell) may wire features together.
- **Cross-feature imports**: `auth`, `workspaces` and `realtime` are platform features — the session, the current workspace and
  its members — and any feature may use them. Every other feature pair should stay independent; if two features need the same
  thing, move it down into `components/common/` or `lib/`.
- **Aggregator pages are the one exception**: a route component under `pages/` may read another feature's public surface
  (its `api/` hooks and presentational components) — the home dashboard and the API-token scopes do. Feature *components*
  must not.
- **Tests sit next to the code** (`Thing.tsx` → `Thing.test.tsx`); run with `bun test`.
