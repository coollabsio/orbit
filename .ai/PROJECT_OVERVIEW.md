# Project overview

## Product

Orbit is a unified workspace UI inspired by Coolify's visual language and selected UX patterns from the reference projects kept under the ignored `shadow/` directory. It combines:

- Home dashboard
- Project tasks in list and Kanban layouts
- Hierarchical collaborative-style docs
- Three-pane mail client
- Channel chat and direct messages
- Notification inbox
- User profile
- Workspace administration and chat administration

The product is optimized for desktop but has responsive master/detail behavior for mobile browsers.

## What is real and what is mocked

### Working frontend behavior

- Navigation and deep links
- Editing and creating mock records
- Drag and drop for tasks, statuses, docs, channels, categories, and mail folders
- Markdown rendering, mentions, channel mentions, reactions, custom emoji, attachments, embeds, image viewing
- Responsive layouts, light/dark themes, motion, modals, confirmation dialogs, and settings forms

### Mock-only behavior

- All data lives in memory in `apps/web/src/mock/store.ts`.
- Seed data is recreated on a full browser reload.
- Uploads use object/data URLs; there is no durable object storage.
- Sessions, users, notifications, webhooks, mail delivery, typing, and realtime responses are demonstrations.
- The Rust server at `apps/server` currently prints `Hello, world!` and is not connected to the web app.

## Application routes

Routes are declared in `apps/web/src/App.tsx`.

| Route | Surface |
|---|---|
| `/` | Home dashboard |
| `/tasks` | Task list/Kanban |
| `/tasks/:taskId` | Full task detail |
| `/tasks/projects/:projectId/settings` | Project and workflow settings |
| `/docs` | Docs index/empty selection |
| `/docs/:docId` | Document editor |
| `/mail` | Mail folders and thread list |
| `/mail/:threadId` | Mail reader |
| `/chat` | First chat channel |
| `/chat/:channelId` | Channel conversation |
| `/chat/:channelId/thread/:rootId` | Full-screen thread |
| `/chat/settings` | Chat administration: roles, webhooks, emoji |
| `/dm` | Direct-message list |
| `/dm/:dmId` | Direct conversation |
| `/inbox` | Notifications |
| `/profile` | Current-user profile |
| `/settings` | Workspace general settings |
| `/settings/members` | Member administration |
| `/settings/sessions` | Admin session management |

Unknown routes redirect to `/`.

## Top-level repository layout

```text
apps/
  web/                 React/Vite frontend; the active product
  server/              Rust placeholder; no API yet
.ai/                   AI/maintainer documentation and lessons
shadow/                ignored local reference repositories
references/            ignored local design/reference material
```

The old root Docker/Coolify deployment scaffolding was deliberately removed. Reintroduce deployment configuration only after the runtime architecture is decided.

## Product decisions that should be preserved

- Purple is the application accent in both light and dark mode.
- UI is flat-first: panes are separated by hairline borders; do not wrap entire pages in cards.
- Second sidebars use aligned 48px headers and are often resizable.
- The first sidebar is collapsible and persists its state.
- Shared behavior should be reused. Chat, DMs, task comments, emoji, attachments, mentions, and markdown deliberately share components.
- Destructive actions require confirmation when data loss is meaningful.
- Motion is restrained and respects `prefers-reduced-motion`.
