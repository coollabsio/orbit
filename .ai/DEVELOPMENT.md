# Development and delivery

## Requirements

- Rust 1.97.1 (installed automatically by `rust-toolchain.toml`)
- Bun 1.3.14 or later
- Just

## Tooling

Orbit uses Bun as its sole frontend package manager. `apps/web/bun.lock` is committed and installs must use frozen-lockfile mode.

The root `justfile` is the standard command surface:

```bash
just setup
just dev
just test
just check
just build
```

`just dev` starts the future Rust server on port **8080** and Vite on port **8888**, stopping both processes when the recipe exits. The initial `orbit` CLI only supplies help; its serving and maintenance subcommands arrive with later backend tasks.

## Direct commands

```bash
cd apps/web
bun install --frozen-lockfile
bun run dev
bun run build
bun run lint
bun run test
```

Backend commands remain usable directly, for example `cargo test --workspace` and `cargo run -p orbit-server -- --help`.

## Development server

Vite is configured for port **8888** in `vite.config.ts`. Use `http://localhost:8888`; do not guess or start duplicate servers if one is already running.

## Required verification before committing

```bash
cd apps/web
bun run build
bun run lint
bun run test
```

A build is necessary because it includes TypeScript project compilation.

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

The backend starts as a Rust workspace: `orbit-platform` contains reusable platform interfaces, `orbit-domain` holds Orbit domain logic, and `orbit-server` is the `orbit` binary. Later tasks add HTTP, persistence, and operational commands. The frontend currently assumes synchronous success; preserve perceived speed with optimistic UI when networking is introduced.
