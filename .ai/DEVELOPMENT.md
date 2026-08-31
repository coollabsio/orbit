# Development and delivery

## Requirements

- A modern JavaScript runtime/package manager
- Node-compatible environment for Vite tooling
- Rust only if working on `apps/server`

## Package manager: aube is optional

The current checkout was installed with **aube**, and `apps/web/aube-lock.yaml` records that installation. Aube is convenient but not an architectural dependency. `package.json` contains standard scripts and dependencies.

### Current aube workflow

```bash
cd apps/web
aube run dev       # Vite on http://localhost:8888
aube run build     # tsc -b && vite build
aube run lint      # oxlint
aube run preview   # Vite preview
```

### Switching to Bun

```bash
cd apps/web
bun install
bun run dev
bun run build
bun run lint
```

Commit `bun.lock` and remove `aube-lock.yaml` as part of the migration so contributors do not have competing lockfiles.

### Switching to pnpm

```bash
cd apps/web
pnpm install
pnpm dev
pnpm build
pnpm lint
```

Commit `pnpm-lock.yaml` and remove `aube-lock.yaml` in the same change. Do not mix package managers within one install.

## Development server

Vite is configured for port **8888** in `vite.config.ts`. Use `http://localhost:8888`; do not guess or start duplicate servers if one is already running.

## Required verification before committing

```bash
cd apps/web
aube run build
aube run lint
```

Equivalent Bun/pnpm commands are valid after migration. A build is necessary because it includes TypeScript project compilation.

For visual or interaction work, also smoke-test the exact route in a browser:

- Check console errors/warnings.
- Test the changed interaction, not only initial rendering.
- Check clipping, stacking, focus rings, overflow, and responsive behavior.
- Check both expanded and collapsed sidebars when shell layout changes.
- Check light and dark themes for token-based styling changes.
- Honor reduced-motion settings.

## Git conventions

- Keep commits small and scoped: `feat(web-chat): ...`, `fix(web-mail): ...`, `style(web-ui): ...`.
- Do not commit `.DS_Store`; root `.gitignore` excludes it recursively.
- `shadow/` and `references/` are local ignored reference material, not shipping source.
- Do not manually create git worktrees unless explicitly requested.
- Preserve unrelated user changes. If asked to commit all pending work, review the status first and describe notable deletions.

## Adding a feature

1. Locate the nearest feature and shared components before creating new primitives.
2. Update `mock/types.ts` when the domain changes.
3. Add representative seed data under `mock/seed/`.
4. Add mutations to `mock/actions.ts`; components should not mutate the store directly.
5. Add routes in `App.tsx` and breadcrumbs in `Topbar.tsx`.
6. Add desktop and mobile navigation if the feature is top-level.
7. Use feature CSS and existing semantic tokens.
8. Build, lint, and manually exercise the interaction.
9. Update these `.ai` docs when architecture or behavior changes.

## Attachments

Use `features/chat/attachmentLib.ts` to convert clipboard/files and `features/chat/components/Attachments.tsx` to render them. Tasks and mail already reuse these. Avoid creating a fourth attachment renderer.

## Emoji and mentions

- Always reuse `components/ui/EmojiPicker.tsx`; chat, docs, modals, and reactions must not diverge.
- Render custom values through `components/ui/Emoji.tsx` or markdown custom-emoji handling.
- Reuse `useMentionAutocomplete` and `MentionPopover` for `@` users and `#` channels.

## Drag and drop

HTML5 drag sources must remain mounted through `dragstart`; hide/fade via attributes instead. Render an explicit placeholder/drop line. See `lessons.md`.

## Backend phase

`apps/server` has no useful implementation yet. Before adding endpoints, decide:

- Authentication/session model
- Workspace/tenant boundaries
- Authorization rules
- Database and migrations
- Attachment storage
- Mail provider integration
- Websocket/event protocol
- Optimistic mutation and conflict strategy

The frontend currently assumes synchronous success. Preserve perceived speed with optimistic UI when networking is introduced.
