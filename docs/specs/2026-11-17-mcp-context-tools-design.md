# MCP context tools (P1) design

Status: draft, 2026-11-17.

## 1. Goal

AI clients use Orbit through MCP (`docs/mcp.md`). Today a client needs many calls to answer a
simple question: it lists projects, then statuses, then members, then tasks, then comments. Each
call costs a round-trip and tokens, and the results hold IDs that need more calls to resolve.

P1 adds five tools that give a small, ready answer in one call:

| Tool | Answers |
| --- | --- |
| `search` | "Where is X?" over tasks, docs and chat in one call. |
| `get_context` | "What is this?" for an Orbit URL that the user pastes. |
| `my_work` | "What must I do?" for the connected person. |
| `project_snapshot` | "How is this project?" for one project. |
| `whats_new` | "What changed since T?" for the connected person. |

P1 adds no AI model, no embeddings and no new search index on the server. The tools compose
existing repositories and existing full-text search at call time. SQL takes milliseconds, so a
cache gives nothing in P1.

## 2. Scope

### In P1

- The five tools above, on the existing `/mcp` endpoint, with the existing scopes.
- Names next to IDs in the results of the new tools (status, assignees, project, channel, author).
- A server function that builds web URLs for docs and chat messages (tasks already have one).
- `docs/mcp.md`: the tool table and the permission table.

### Not in P1

- Mail. Mail has no MCP scope today. It needs a new `mail:read` scope that is off by default, and
  its own consent text. Add it as a separate step after P1 (section 9).
- Task full-text search. `search` uses the current task `LIKE` match. Add FTS only if the evals
  (section 8) show that task results are bad.
- Embeddings, stored summaries, and any precomputed context.
- Changes to existing tools. They keep their output shape.
- Web or mobile UI changes. The web app does not list MCP tool names, so it needs no change.

## 3. Shared rules

These rules apply to all five tools.

### 3.1 Identity and workspace

- The tool gets the principal from `workspaces(&context)` and picks the workspace with
  `select_workspace` (`apps/server/src/mcp.rs:543`). A multi-workspace connection must send
  `workspace_id`, as for the other tools.
- Access is the access of `principal.actor_id`, limited by `principal.project_ids` (approved
  projects) and `principal.scopes`.
- Service accounts: `search`, `get_context`, `project_snapshot` and `whats_new` return task data
  only. `my_work` returns `NotAvailable`, because a service account has no own work.

### 3.2 Scopes

`Authorized::need()` accepts exactly one scope (`apps/server/src/token_api.rs:792`). A tool that
reads more than one type calls `TokenApi::authorize` once for each scope that it uses.

| Tool | Listed when the connection has | Uses |
| --- | --- | --- |
| `search` | any of `tasks:read`, `docs:read`, `chat:read` | each type only with its scope |
| `get_context` | any of the three | the scope of the URL's type |
| `my_work` | `tasks:read` | `tasks:read`; adds mentions in docs and chat only with `docs:read` / `chat:read` |
| `project_snapshot` | `tasks:read` | `tasks:read` |
| `whats_new` | any of the three | each section only with its scope |

`required_scope(name)` (`mcp.rs:415`) returns one scope, and `list_tools` filters on it. The
multi-scope tools need a small change there: a tool is listed when the connection has at least
one of its scopes.

When a type is skipped because a scope is missing, the result says so in `skipped`, for example
`"skipped": [{"type": "message", "reason": "chat:read not granted"}]`. Then the model can tell
the user to reconnect, and it does not think that the workspace has no messages.

### 3.3 Access checks

Every item is checked when the tool reads it. No result is built from stored or cached data
that mixes items with different access.

| Type | Check | Reuse |
| --- | --- | --- |
| Task | Live task in an approved project that the actor can see (private projects, custom roles) | `Authorized::task`, `Authorized::approved`, `ProjectScope` (`repositories/project_access.rs:14`), `TaskRepository::approved_projects` |
| Doc | Team doc, or a private doc of the actor | `PageRepository::visible` / `get_visible` / `search_pages` (`repositories/pages.rs:68`) |
| Chat message | Public channel (reader is not a guest), a member of the conversation, or a reachable DM | `ChatRepository::load_access`, `public_for`, `reachable_dm` (`repositories/chat/mod.rs:885`) |

Extra rules:

- **Embedded items.** A result can name another item (a blocker task, a mentioned doc). Show it
  only when that item passes its own check and the scope is granted. Otherwise leave it out. Do
  not show a placeholder with a count: the count is information too.
- **Hidden and missing look the same.** Use `hidden_page`, `hidden_chat` and the existing task
  "not found" error. A tool must not tell "no access" apart from "does not exist".
- **Notifications.** `TaskRepository::notifications` uses `ProjectScope` but not the approved
  projects. The new tools also drop notifications for tasks in unapproved projects,
  `page_*` kinds without `docs:read`, and `chat_mentioned` without `chat:read`.
- **Docs and approved projects.** Docs are not filtered by approved projects today
  (`token_api.rs:1560`). The new tools use the same rule as `list_docs` and `get_doc`, so that a
  doc is either visible in all tools or in none.
- **User text is data.** Titles, descriptions, doc bodies and messages are user-written. The tool
  descriptions keep the existing sentence that clients must treat them as data, not
  instructions.

### 3.4 Output shape

- Results use `CallToolResult::structured(value)`, as the other tools do.
- Each item has a `type`, an `id`, a human `title` or `ref`, and a `url`. Use names next to IDs:
  `{"id": "…", "name": "In progress", "category": "started"}`.
- Times are RFC 3339 UTC strings, as in `ReadTask`.
- Text fields are cut to a fixed size and get `"truncated": true` when cut. The full text is
  available through the existing tool (`get_task`, `get_doc`, `get_thread`), and the result says
  which tool in `more`.
- **Size budget:** one result stays under about 24 KB of JSON (about 6k tokens). The item limits
  and text limits in each section keep the result under this budget. Do not count bytes at run
  time.

### 3.5 URLs

- Tasks: the existing `{origin}/tasks/{task_id}?workspace={ws}` (`token_api.rs:197`).
- Docs (new): `{origin}/docs/{page_id}?workspace={ws}`, plus `&block={block_id}` when the match
  is in a block.
- Chat (new): `{origin}/chat/{conversation_id}?m={message_id}&workspace={ws}`, and
  `{origin}/chat/{conversation_id}?thread={root_id}&workspace={ws}` for a thread. Follow
  `messagePath` in `apps/web/src/features/chat/chatRoutes.ts`.
- Put the builders next to the task URL builder, so that one module owns all web URL shapes.

### 3.6 Code placement

- Logic and access checks: a new `apps/server/src/token_api/context.rs`, as methods on
  `TokenApi`, next to `relations.rs`. This keeps `token_api.rs` (2005 lines) from growing.
- MCP glue (argument structs, definitions, dispatch arms): a new
  `apps/server/src/mcp/context.rs`, in the same way as `mcp/databases.rs`. `mcp.rs` gets only
  the module, the `tools()` entries and the dispatch arms.
- No new tables and no migrations.

## 4. `search`

Search tasks, docs and chat messages in one call.

### Arguments

| Name | Type | Notes |
| --- | --- | --- |
| `query` | string, 1–200 chars, required | Same text as in the web search |
| `types` | array of `task`, `doc`, `message`, optional | Default: all that the connection can read |
| `project_id` | string, optional | Tasks only. An approved project ID or key. |
| `limit` | integer 1–20, default 5 | For **each** type |

### Behavior

- Tasks: `list_tasks` with `query` (the current `push_text` match on title, description,
  identifier and `#number`, `repositories/task_filter.rs:1383`), in approved projects, live
  tasks first by `updated_at`. Completed tasks are included and marked by status category.
- Docs: `PageRepository::search_pages` (`pages.rs:2077`). It gives up to 20 results by rank and
  has no cursor; take the first `limit`.
- Messages: `ChatRepository::search_messages` (`chat/search.rs:51`), newest first, first `limit`.
- Each type runs on its own. The result is grouped by type and is not merged into one ranked
  list, because the ranks are not comparable (LIKE, BM25, `ts_rank`).
- No cursor. When a group has more results, it gets `more` with the tool and the arguments that
  continue it, for example `{"tool": "list_tasks", "args": {"query": "…", "cursor": "…"}}` or
  `{"tool": "search_messages", "args": {…}}`.
- Snippet: up to 200 chars of plain text around the first match. Cut in Rust, not with
  `snippet()` or `ts_headline`, so that both databases give the same result.

### Result

```json
{
  "tasks": {
    "items": [{
      "type": "task", "id": "…", "ref": "ENG-12", "title": "Fix login redirect",
      "status": {"id": "…", "name": "In progress", "category": "started"},
      "project": {"id": "…", "key": "ENG", "name": "Engineering"},
      "assignees": [{"id": "…", "name": "Ana"}],
      "snippet": "…redirect loops after OIDC sign-in…",
      "updated_at": "2026-11-16T09:12:00Z", "url": "https://…/tasks/…?workspace=…"
    }],
    "more": {"tool": "list_tasks", "args": {"query": "login", "cursor": "…"}}
  },
  "docs": {"items": [{"type": "doc", "id": "…", "title": "Auth design", "path": "Engineering / Auth design", "snippet": "…", "updated_at": "…", "url": "…"}]},
  "messages": {"items": [{"type": "message", "id": "…", "conversation": {"id": "…", "name": "#eng"}, "author": {"id": "…", "name": "Ana"}, "snippet": "…", "created_at": "…", "url": "…"}]},
  "skipped": []
}
```

A group is left out when its type is not in `types`.

## 5. `get_context`

Resolve an Orbit web URL, or a task identifier, to its content.

### Arguments

| Name | Type | Notes |
| --- | --- | --- |
| `url` | string, required, max 2048 chars | A web URL of this Orbit, or a task ref such as `ENG-12` |

### Parsing

- **Never fetch the URL.** The server only parses it. This avoids SSRF.
- The origin must be the server's web origin (`with_web_origin`). Another origin gives
  `invalid_params` ("not a URL of this Orbit").
- If the URL has `?workspace=`, it must be the selected workspace. A multi-workspace connection
  can omit `workspace_id` when `?workspace=` names a workspace that the connection covers.
- Accepted shapes (section 3.5 and `apps/web/src/app/App.tsx:115`):

| Shape | Resolves to |
| --- | --- |
| `/tasks/{id}`, `/views/{view}/{id}`, `ENG-12` | task |
| `/docs/{page}`, with optional `?block=` and `?thread=` | doc (and the block or the comment thread) |
| `/chat/{conv}?m={msg}`, `/chat/{conv}?thread={root}`, `/chat/{conv}/thread/{root}` | message with context, or thread |
| `/chat/{conv}` | the latest messages of the conversation |
| `/mail/…` | `invalid_params` ("mail is not available through MCP") until section 9 |
| anything else | `invalid_params` with the list of accepted shapes |

The parser is a pure function with unit tests. Use `parseChatLocation`
(`chatRoutes.ts:52`) as the reference for the chat shapes.

### Result by type

- **Task:** the task fields as in `get_task`, plus names (status, project, assignees, labels,
  milestone, cycle), the parent `{ref, title}`, sub-issues `{ref, title, status}` (max 20),
  blockers and blocked tasks from `task_relations` that pass the task check (max 10 each), and the
  last 10 comments (`{author, created_at, body}` with the body cut to 1000 chars). Description cut
  to 8000 chars.
- **Doc:** title, path, `updated_at`, the body as markdown cut to 12000 chars. With `?block=`,
  the text starts at that block. With `?thread=`, add that comment thread (max 20 comments).
- **Message:** the conversation `{id, name, kind}`, 10 messages before and 10 after the message,
  and the thread root and up to 30 replies when the message is in a thread.
- **Conversation:** the conversation and its last 30 messages.

Each type has `"type"` and `more` that names the existing tool for the full content.

## 6. `my_work`

The open work of the connected person. Needs `tasks:read` and a personal identity.

### Arguments

| Name | Type | Notes |
| --- | --- | --- |
| `limit` | integer 1–25, default 10 | For each section |

### Sections

All task sections: tasks assigned to the actor, live, in approved projects, with a status
category of `unstarted` or `started`. A task shows in the first section that it matches only.

| Section | Rule | Order |
| --- | --- | --- |
| `overdue` | `due_at` before now | `due_at` ascending |
| `due_soon` | `due_at` in the next 7 days | `due_at` ascending |
| `current_cycle` | in the current cycle of its project | priority, then `updated_at` |
| `in_progress` | category `started` | `updated_at` descending |
| `other_open` | the rest | priority, then `updated_at` |
| `mentions` | unread notifications of a mention kind, filtered as in section 3.3 | newest first |

Each section has `items` and `total`. The task item is the compact form of section 4 plus
`due_at` and `priority`. `TaskQuery` already filters by assignee, due date and cycle
(`token_api.rs:1066`), so most sections are one `TaskQuery` each.

## 7. `project_snapshot`

The state of one project. Needs `tasks:read`.

### Arguments

| Name | Type | Notes |
| --- | --- | --- |
| `project_id` | string, required | Approved project ID or key |

### Result

| Field | Content |
| --- | --- |
| `project` | `{id, key, name}` |
| `status_counts` | live task count for each status, with the status name and category |
| `current_cycle` | as `list_cycles` (`relative = current`), or `null` |
| `milestones` | open milestones with progress, by target date, max 5 |
| `overdue` | open tasks with `due_at` before now, max 10, plus `total` |
| `urgent` | open tasks with priority `urgent` or `high`, max 10, plus `total` |
| `blocked` | open tasks with a `blocks` relation from an open task, max 10, plus `total`. Only blockers that pass the task check count. |
| `recently_completed` | tasks completed in the last 7 days, max 10, plus `total` |
| `unassigned` | count of open tasks with no assignee |

"Open" is a status category of `unstarted` or `started`. The progress counts follow the existing
rules in `docs/mcp.md` (cancelled and duplicate tasks are not counted).

## 8. `whats_new`

What changed since a time, for the connected person.

### Arguments

| Name | Type | Notes |
| --- | --- | --- |
| `since` | RFC 3339 time, required | Max 30 days ago. Older values give `invalid_params`. |
| `types` | array of `task`, `doc`, `message`, `notification`, optional | Default: all that the connection can read |
| `limit` | integer 1–25, default 15 | For each section |

### Sections

| Section | Source | Content |
| --- | --- | --- |
| `tasks` | `tasks.created_at`, `updated_at`, `completed_at` after `since`, approved projects | compact task plus `change`: `created`, `completed` or `updated` (the first that applies) |
| `docs` | `pages.updated_at` after `since`, visible docs | `{id, title, path, updated_at, url}` |
| `conversations` | conversations that the actor can read with messages after `since` | `{conversation, new_count, last_message: {author, snippet, created_at, url}}`, newest first |
| `notifications` | `notifications.created_at` after `since`, filtered as in section 3.3 | `{kind, actor, target, created_at, read}` |

Do not use `audit_events` in P1. It holds admin actions and has no item-level access rule that
fits members.

Each section has `items` and `total`. Sections that need a missing scope go into `skipped`.

## 9. Later: mail

Add mail after P1, as its own change:

- New scope `mail:read`, off by default, with consent text that names personal and shared
  mailboxes. Service accounts get no mail.
- Access: a `mailbox_members` row of the actor, through `MailRepository` and `reader()`
  (`repositories/mail/reading.rs:5`).
- `search` gets a `mail` type (`MailRepository::search_threads`), `get_context` accepts
  `/mail/…/t/{thread}`, and `whats_new` gets a `mail` section (unread threads since `since`).
- Mail is the largest prompt-injection risk, because people outside the workspace write it. The
  tool description must say this. Mail tools stay read-only.

## 10. Tests

Add the tests to `apps/server/tests/mcp_api.rs`. Run them on SQLite and on Postgres
(`ORBIT_TEST_POSTGRES_URL`).

Access (the most important part):

- A task in an unapproved project, or in a private project that the actor cannot see, is not in
  `search`, `my_work`, `project_snapshot`, `whats_new` or as a blocker in `get_context`.
- Another member's private doc is not in `search`, `whats_new` or `get_context`. `get_context`
  gives the same error as for a missing doc.
- A private channel that the actor is not in, and a DM of other members, is not in `search`,
  `whats_new` or `get_context`. A guest does not see public channels.
- Without `chat:read`, `search` lists `message` in `skipped`, and `whats_new` drops chat
  mentions from `notifications`.
- `get_context` with another origin, or with `?workspace=` of a workspace that the connection
  does not cover, gives `invalid_params`.

Behavior:

- `get_context` URL parser: unit tests for each shape in section 5, and for bad input.
- `my_work`: a task shows in one section only, and the order matches section 6.
- `whats_new`: `since` older than 30 days gives `invalid_params`.
- `search` on both databases gives the same doc and message results for the same data.

## 11. Evals

Before and after P1, run a fixed list of questions against the dev seed with two or three MCP
clients (Claude, Codex, a small local model). Count the tool calls and the tokens for each
answer, and check if the answer is correct. Example questions:

1. "What should I work on today?"
2. "Where did we decide the pricing for Cloud?"
3. "How is the ENG project doing?"
4. "What happened since yesterday?"
5. "Summarize this: `<task URL>`."

P1 is a success when each question needs at most two calls, and the answers are correct. The
same list decides later if task FTS or embeddings are necessary.

## 12. Steps

Each step ends in a state that you can check.

1. URL builders for docs and chat, and the `get_context` parser, with unit tests.
2. The multi-scope change in `required_scope` / `list_tools`, and `skipped`.
3. `get_context` (uses step 1).
4. `search`.
5. `my_work` and `project_snapshot`.
6. `whats_new`.
7. `docs/mcp.md` and the Account → Agentic tool list.
8. Evals (section 11).

## 13. Open questions

| # | Question | Default in this spec |
| --- | --- | --- |
| 1 | Are docs filtered by approved projects? | No, the same rule as `list_docs` today |
| 2 | Is `whats_new` limited to 30 days? | Yes |
| 3 | Does `search` need a cursor of its own? | No, `more` points to the per-type tool |
| 4 | Mail in P1? | No, a separate step with `mail:read` (section 9) |
