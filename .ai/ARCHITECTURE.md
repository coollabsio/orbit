# Architecture

## Frontend stack

- React 19
- TypeScript 6
- React Router 8
- Vite 8
- React Compiler through `@rolldown/plugin-babel`
- Reicon React for icons
- Oxlint
- Plain CSS with design tokens; no CSS-in-JS and no component framework

Entry points:

- `apps/web/index.html` applies the saved dark/light class before paint.
- `apps/web/src/main.tsx` mounts React in strict mode.
- `apps/web/src/App.tsx` owns providers and routing.
- `apps/web/src/components/shell/AppShell.tsx` owns the global sidebar, mobile drawer/dock, topbar, command palette, and routed content.

## State architecture

The mock store is intentionally tiny:

- `mock/store.ts` holds a module-level `AppState` object.
- `useAppState()` subscribes using `useSyncExternalStore`.
- `updateState(updater)` immutably replaces state and synchronously notifies listeners.
- `mock/actions.ts` is the only mutation API used by feature components.
- `mock/seed.ts` composes seed modules under `mock/seed/`.
- `mock/types.ts` is the canonical domain schema.

There is no persistence layer. Refreshing reconstructs `seedState()`.

### Backend migration seam

When introducing the real backend, keep feature components stable by replacing the mock boundary rather than embedding fetch calls throughout views:

1. Preserve the domain types or introduce API/domain mapping at the boundary.
2. Replace action functions with request/mutation services.
3. Replace `useAppState()` with query/store selectors.
4. Add optimistic updates for existing immediate interactions.
5. Map websocket events into the same state transitions used by local actions.
6. Do authentication and permission checks server-side; UI hiding is not authorization.

## Domain model summary

`AppState` contains:

- `users`, `roles`, `workspace`
- `projects`, per-project `statuses`, `tasks`
- hierarchical `docs`
- `mailFolders`, `mailThreads`
- `chatCategories`, `channels`, `directMessages`, shared `chatMessages`
- `webhooks`, `customEmojis`, `typingUsers`
- `sessions`, `notifications`

Important relationships:

- Tasks reference a project and a per-project status by id.
- Docs form a tree through `parentId`; blocks can link other docs through `refId`.
- Channel and DM messages share `ChatMessage`; `channelId` may be a channel id or DM id.
- Threads are messages: roots use `startsThread`; replies use `threadRootId`.
- Task comments reuse `Attachment` and chat rendering components but live inside each task.
- Mail messages also reuse `Attachment`.

## Frontend folders

```text
src/
  components/
    shell/       application chrome and navigation
    ui/          reusable primitives
    workspace/   shared task/project visual components
  features/
    chat/        channels, DMs, messages, threads, settings
    docs/        document tree and block editor
    home/        dashboard
    inbox/       notifications
    mail/        folders, threads, reader, compose/reply
    profile/     personal profile
    settings/    workspace/admin settings
    shared/      feature-shared card CSS
    tasks/       list, Kanban, task detail, project workflow settings
  lib/           formatting, theme, class helper, navigation bridge
  mock/          domain types, seed, store, actions
  styles/        tokens, reset/base, shared utilities
```

Feature-specific components live one level deeper under `features/*/components`.

## Styling architecture

Import order in `src/index.css`:

1. `styles/tokens.css` — semantic colors, sizes, surface ladder.
2. `styles/base.css` — reset, typography, focus, scrollbars, route motion, reduced-motion override.
3. `styles/utilities.css` — buttons, inputs, navigation rows, panels, modal, popover, shared emoji shell, tables, badges.

Each feature imports its own CSS. Prefer semantic tokens such as `--canvas`, `--base`, `--elevated`, `--line`, `--hairline`, `--text-*`, and `--accent`; do not scatter literal theme colors.

## Shared UI primitives

- `Modal` — focus-trapped dialog, Escape/outside close, scroll lock.
- `Dropdown` and `Listbox` — application menus; avoid native selects for styled controls.
- `EmojiPicker` and `Emoji` — the one shared picker/rendering path, including custom emoji.
- `Avatar` / `AvatarStack`
- `DatePicker`, `InfoTip`, `UnsavedBar`, `EmptyState`
- `Attachments` / `attachmentLib` in chat are reused by tasks and mail.
- `MessageContent`, markdown utilities, mention autocomplete, and message input power channel chat, DMs, and task comments.

## Navigation bridge

Markdown is rendered by plain functions that cannot call React hooks. `lib/navigateBridge.ts` receives React Router's navigate function from `NavigateBridge` in `App.tsx`, allowing internal markdown links and channel mentions to navigate without a full reload. External links still open externally.

## Responsive behavior

The main breakpoint is `899px`:

- Desktop uses the first sidebar plus multi-pane feature layouts.
- Mobile hides the desktop sidebar, shows a bottom dock and optional drawer.
- Master/detail features switch between list and selected-record views based on the route.
- Resizers are hidden on mobile.

Do not add parallel mobile state when the URL can be the source of truth.
