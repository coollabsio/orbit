# Orbit public REST API

The public REST API lets scripts, CI jobs and the Orbit CLI use Orbit with a
workspace API token. The base path is `<public origin>/api/public/v1`.
The OpenAPI document in the repository (`packages/api/src/generated/openapi.json`,
or `orbit openapi --output <file>`) describes this API under the tag `public-api`.

This API is separate from the internal `/api/v1` API of the web and mobile apps.
The internal API can change in each release. Use only the public API in your
scripts.

## Authentication

1. As a workspace owner or administrator, open **Settings → API tokens**. Other
   members (not guests) can create only a personal export token there (see
   [Exports](#exports)).
2. Select the projects, the permissions and an expiry. Then create the token.
   `project_ids` is required, with at least one project, when `scopes` includes
   `tasks:read` or `tasks:write`. Alternatively, set `all_projects: true` (the
   **All projects, including future ones** box): the token then covers every
   live project of the workspace, also projects created later. Without a task
   scope, projects have no effect: the token stores none (`project_ids: []`).
3. Copy the token (`orb_…`). Orbit shows it one time only.
4. Send the token in the `Authorization` header of each request:

```bash
export ORBIT_URL='https://orbit.example.com'
read -rs -p 'Orbit API token: ' ORBIT_TOKEN; echo
export ORBIT_TOKEN

curl -sS "$ORBIT_URL/api/public/v1/me" -H "Authorization: Bearer $ORBIT_TOKEN"
```

Do not put the token in a URL, in a repository, or in a shared shell history.

Each token belongs to one workspace and to the projects that you selected, or to
all live projects of the workspace when you selected all projects.
Task and doc paths contain the workspace ID:
`/api/public/v1/workspaces/{workspace_id}/…`. Get the ID from `GET /me`. A
path with another workspace ID returns `404`. The API does not use cookies, sessions or CSRF tokens. Requests
without an `Origin` header are accepted. If a request has an `Origin` header, the
origin must be the Orbit public origin. The `X-Orbit-Contract` header is not
necessary.

A personal token acts as the person who created it. Orbit checks on each request
that this person is still a member of the workspace and that their account is
not suspended. If not, the token stops working.

A service-account token does not depend on the person who created it. It keeps
working when that person leaves the workspace, changes role or is suspended,
until an owner or administrator revokes it. When you revoke a token and the
service account has no other token that is not revoked and not expired, Orbit
disables the service account; its past work keeps its
name. A new token with the same service-account name enables it again. The
GitHub integration's service account (named `GitHub`) is never disabled this way.
A service-account token can use tasks only, not docs, chat or mail. Orbit refuses to create a
service-account token with `docs:read`, `docs:write`, `chat:read`, `mail:read` or `mail:write` (`422`). Its writes show the service account as the author:
as the creator of the tasks that it makes, and as the author of its comments.
Its writes do not send notifications (assignments and mentions), because no
member made them.

The audit log records each task and comment write with the ID of the token
(`api_token_id`). A personal token shows its creator as the actor. A
service-account token shows the service account as the actor.

## Scopes

API tokens and MCP OAuth connections use the same seven resource scopes. In
**Settings → API tokens**, set the access for each resource to **No access**,
**Read** or **Write**. API tokens also have an import scope and two export
scopes, which MCP does not use.

| Scope | Access in **Settings → API tokens** | Public API endpoints |
| --- | --- | --- |
| `tasks:read` | Tasks: **Read** | `GET` projects, statuses, labels, milestones, cycles, members, tasks, comments, relations, task activity and the task trash |
| `tasks:write` | Tasks: **Write** | Create, update and delete tasks, restore them from the trash, add, edit and delete comments, link tasks, create and change projects, create, change and delete statuses, labels, milestones and cycles, `POST /integrations/discord/events` |
| `tasks:import` | Tasks: **Write** with **Import mode** | With `tasks:write`: the import fields of new tasks and comments. See [Import from another tool](#import-from-another-tool). |
| `docs:read` | Docs: **Read** | `GET /docs`, `GET /docs/trash`, `GET /teamspaces`, `GET /docs/{page_id}`, `GET /docs/{page_id}/comments`, `GET /docs/{page_id}/versions`, `GET /docs/{page_id}/versions/{version_id}`, `GET /docs/{page_id}/database`, `POST /docs/{page_id}/database/query`, `GET /docs/{page_id}/database/views` and `…/views/{view_id}` |
| `docs:write` | Docs: **Write** | `POST /docs`, `PATCH /docs/{page_id}`, `POST /docs/{page_id}/move`, `DELETE /docs/{page_id}`, `POST /docs/{page_id}/restore`, `POST /docs/{page_id}/comments`, `POST /docs/{page_id}/rows`, `PATCH /docs/{page_id}/rows/{row_id}`, `POST /docs/{page_id}/rows/trash`, `POST /databases`, the schema, view and linked view endpoints under `/docs/{page_id}/database` |
| `chat:read` | Chat: **Read** | `GET /chat/conversations`, `GET /chat/conversations/{conversation_id}/messages`, `GET /chat/threads/{message_id}`, `GET /chat/search`. Chat has no write endpoint: a bot that reads and writes one channel uses a [chat webhook with events](chat-webhooks.md#events). |
| `mail:read` | Mail: **Read** | `GET /mail/mailboxes`, `GET /mail/threads`, `GET /mail/threads/{thread_id}`, `GET /mail/search`, `GET /mail/counts`, `GET /mail/attachments/{attachment_id}`, `GET /mail/drafts`, `GET /mail/drafts/{draft_id}`, `POST /mail/drafts/{draft_id}/check`, `GET /mail/outbound`, `GET /mail/labels`, `GET /mail/contacts`, `GET /mail/from-addresses`, `GET /mail/signatures/{address_id}`, `GET /mail/snippets`, `GET /mail/settings`, `GET /mail/mailboxes/{mailbox_id}/vacation` |
| `mail:write` | Mail: **Write** | Change threads (`PATCH /mail/threads/{thread_id}`, `POST /mail/threads/bulk`), create, change, delete and **send** drafts, draft files, cancel a send, change labels, signatures, snippets, settings and vacation replies. See [Mail](#mail). |
| `workspace:export` | **Workspace export** | `POST /exports` with `mode: "full"`, `GET /exports/{export_id}`, `GET /exports/{export_id}/download` |
| `workspace:export:personal` | **Personal export** | The same, with `mode: "personal"` |

`tasks:write` includes `tasks:read`, `docs:write` includes `docs:read`, and `mail:write` includes
`mail:read`.
`GET /me` works with each valid token.

An export scope must be the only scope of a personal token: Orbit refuses a
service-account token with an export scope, or an export scope together with
another scope (`422 invalid_api_token`). `workspace:export` needs a workspace
owner or administrator, when you create the token and on each request.
`workspace:export:personal` is open to each member except guests.

`tasks:import` needs `tasks:write` (`422 invalid_api_token` otherwise). Its
creator must be a workspace owner or administrator on each write: when the
creator loses that role, each write of the token gets `403
workspace_action_forbidden`.

## Conventions

- Field names are the same as in the internal Orbit records, so clients can
  share models.
- IDs are UUID strings. Times are RFC 3339 strings in UTC with milliseconds, for
  example `2026-11-04T09:30:00.000Z`.
- A `{task}` path segment is a task UUID or a task identifier such as `ENG-12`
  (the letter case is not important).
- A `{task}` value that is neither a UUID nor an identifier gives `400`
  (`invalid_request`) on every method. An identifier of no visible task gives
  `404`.
- A task's or doc's `version` is an opaque number below 2^53. It changes with
  every change, including live edits in the editor. Send it back unchanged as
  `expected_version`, and compare it only for equality; it does not count up.
- Lists return `{"items": [...], "next_cursor": string | null}`. To get the next
  page, send `next_cursor` as `cursor` with the same
  other parameters. `next_cursor` is `null` on the last page and on lists that
  have no pages.
- `limit` is 1 to 100. The default is 50.
- A task contains no embedded parent, duplicate or ancestor records.
  `parent_task_id` is `null` when the parent is not in an approved project.

- `POST /tasks` and `POST /tasks/{task}/comments` take an optional
  `Idempotency-Key` header (1 to 255 visible ASCII characters, for example a
  UUID), so a client can retry after a timeout without a second task or comment.
  Orbit keeps the first successful response (`200` or `201`) for 24 hours for
  each token and key. A repeat with the same path and JSON body (the order of
  fields does not matter) gets that response again, with the header
  `Idempotent-Replayed: true`. The same key with another path or body gets `422`
  (`idempotency_key_reused`). If the first request still runs, the repeat gets
  `409` (`idempotency_request_running`) with `Retry-After: 1`; retry it. Orbit
  keeps no response of an error, so a retry after an error runs again. Use a
  new key for each new task or comment.

## Endpoints

### Token and workspace

`GET /me`. Scope: none.

```bash
curl -sS "$ORBIT_URL/api/public/v1/me" -H "Authorization: Bearer $ORBIT_TOKEN"
```

```json
{
  "token": {"id": "…", "name": "ci", "scopes": ["tasks:read", "tasks:write"],
            "project_ids": ["…"], "expires_at": "2026-12-01T00:00:00.000Z"},
  "workspace": {"id": "…", "name": "Acme"},
  "workspaces": [{"id": "…", "name": "Acme", "scopes": ["tasks:read", "tasks:write"],
                  "project_ids": ["…"]}],
  "user": {"id": "…", "email": "dev@example.com", "display_name": "Dev"},
  "service_account": null
}
```

For a service-account token, `user` is `null` and `service_account` is
`{"id", "name"}`.

`workspaces` lists each workspace that the token can use, with the scopes and
projects there. Today a token has exactly one workspace, so the list has one
item, the same as `workspace`. Read `workspaces` in new clients: a later version
can give a token more than one workspace.

In the examples below, set the workspace first:

```bash
export ORBIT_WS="$ORBIT_URL/api/public/v1/workspaces/$WORKSPACE_ID"
```

### Projects, statuses, labels, milestones, cycles and members

`GET /workspaces/{workspace_id}/projects`. Scope: `tasks:read`. The approved live projects. Item:
`{id, name, key, color, version, created_at, updated_at}`.

```bash
curl -sS "$ORBIT_WS/projects" -H "Authorization: Bearer $ORBIT_TOKEN"
```

`POST /workspaces/{workspace_id}/projects`. Scope: `tasks:write`, and the token must cover
**all projects**; a token for chosen projects gets 403 `all_projects_required`, because it could
not use the new project. Body: `{name, key, color?}`. `key` is the prefix of task identifiers:
1 to 20 letters, digits or underscores, unique in the workspace, stored in upper case. `color` is
`#rrggbb` (default `#6366f1`). The project gets the default workflow statuses. Answers 201 with
the project, or 409 when the key is already used.

```bash
curl -sS -X POST "$ORBIT_WS/projects" -H "Authorization: Bearer $ORBIT_TOKEN" \
  -H 'Content-Type: application/json' --data '{"name":"Video series","key":"VID"}'
```

`PATCH /workspaces/{workspace_id}/projects/{project_id}`. Scope: `tasks:write`. Body:
`{expected_version, name?, color?}`. A missing field keeps its value. Answers 200 with the
project, or 409 when the project changed after you read it. The key does not change.

`GET /workspaces/{workspace_id}/projects/{project_id}/statuses?cursor&limit`. Scope: `tasks:read`. Item:
`{id, project_id, name, description, color, category, position, version}`.
`category` is `unstarted`, `started`, `completed`, `cancelled` or `duplicate`.

```bash
curl -sS "$ORBIT_WS/projects/$PROJECT_ID/statuses" \
  -H "Authorization: Bearer $ORBIT_TOKEN"
```

`POST /workspaces/{workspace_id}/projects/{project_id}/statuses`. Scope: `tasks:write`. Body:
`{name, category, color?, description?, position?}`. `category` is `unstarted`, `started`,
`completed` or `cancelled`: tasks in a `completed` or `cancelled` status are done. Orbit makes
the `duplicate` status itself. Without `color`, the status gets the color of its category.
Without `position`, it goes after the last status. Answers 201 with the status.

`PATCH /workspaces/{workspace_id}/projects/{project_id}/statuses/{status_id}`. Scope:
`tasks:write`. Body: `{expected_version, name?, category?, color?, description?, position?}`. A
missing field keeps its value. The Duplicate status cannot change its category, and no other
status can become `duplicate` (400). Answers 200 with the status, or 409 after a change.

`DELETE /workspaces/{workspace_id}/projects/{project_id}/statuses/{status_id}?expected_version=…`.
Scope: `tasks:write`. Answers 204. No task may use the status, trashed tasks included (409), and
the Duplicate status cannot be deleted (400). The token's creator must be allowed to manage the
project (403 otherwise).

```bash
curl -sS -X POST "$ORBIT_WS/projects/$PROJECT_ID/statuses" \
  -H "Authorization: Bearer $ORBIT_TOKEN" -H 'Content-Type: application/json' \
  -d '{"name":"In review","category":"started"}'
```

`GET /workspaces/{workspace_id}/projects/{project_id}/labels?cursor&limit`.
Scope: `tasks:read`. Item: `{id, project_id, name, color, version}`. Use the
IDs in `label_ids`.

```bash
curl -sS "$ORBIT_WS/projects/$PROJECT_ID/labels" \
  -H "Authorization: Bearer $ORBIT_TOKEN"
```

`POST /workspaces/{workspace_id}/projects/{project_id}/labels`. Scope: `tasks:write`. Body:
`{name, color?}`: 1 to 100 characters, and `#rrggbb` (gray when unset). Answers 201 with the
label, or 409 when the project already has a label with this name.

`PATCH /workspaces/{workspace_id}/projects/{project_id}/labels/{label_id}`. Scope:
`tasks:write`. Body: `{expected_version, name?, color?}`. A missing field keeps its value.
Answers 200 with the label, or 409 after a change.

`DELETE /workspaces/{workspace_id}/projects/{project_id}/labels/{label_id}?expected_version=…`.
Scope: `tasks:write`. Answers 204 and takes the label off its tasks. The token's creator must be
allowed to manage the project (403 otherwise).

`GET /workspaces/{workspace_id}/projects/{project_id}/milestones`. Scope: `tasks:read`. Every
milestone of the project in its order, on one page (`next_cursor` is `null`). Item:
`{id, project_id, name, description, target_at, position, version, task_count, completed_count,
estimate_total, estimate_completed}`. Use the IDs in `milestone_id`.

`POST /workspaces/{workspace_id}/projects/{project_id}/milestones`. Scope: `tasks:write`. Body:
`{name, description?, target_at?}`: a name of 1 to 200 characters, unique in the project (409
otherwise), and a description of up to 20,000 characters. `target_at` is `YYYY-MM-DD`, stored at
noon UTC as the web app does, or an RFC 3339 time. The milestone goes after the others. Answers 201
with the milestone, as the list shows it.

`PATCH /workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}`. Scope:
`tasks:write`. Body: `{expected_version, name?, description?, target_at?, position?}`. A missing
field keeps its value; `null` clears `description` or `target_at`. Answers 200, or 409 after a
change.

`DELETE /workspaces/{workspace_id}/projects/{project_id}/milestones/{milestone_id}?expected_version=…`.
Scope: `tasks:write`. Answers 204. Its tasks stay, without a milestone.

`GET /workspaces/{workspace_id}/projects/{project_id}/cycles`. Scope: `tasks:read`. Every cycle of
the project by start date, on one page. Item: `{id, project_id, number, name, starts_at, ends_at,
relative, version, task_count, completed_count, estimate_total, estimate_completed,
carried_over_count}`. `carried_over_count` counts the live tasks that "Move unfinished tasks to
next cycle" took out of the cycle and that are not back in it; they are not in `task_count`. A cycle
covers `starts_at` up to `ends_at`, exclusive. `relative` is `current` (the cycle that runs now),
`next` (the first that starts after now), `previous` (the last that ended before now) or `null`.
Use the IDs in `cycle_id`.

`POST /workspaces/{workspace_id}/projects/{project_id}/cycles`. Scope: `tasks:write`. Body:
`{name?, starts_at?, ends_at?}`. A date is an RFC 3339 time, or `YYYY-MM-DD` for midnight UTC;
`ends_at` is exclusive. Without `starts_at`, the cycle starts when the last cycle ends (today's
UTC midnight for the first one). Without `ends_at`, it lasts as long as the last cycle (14 days for
the first one). Cycles of a project cannot overlap (409); they may touch. Answers 201 with the next
numbered cycle, as the list shows it.

`PATCH /workspaces/{workspace_id}/projects/{project_id}/cycles/{cycle_id}`. Scope: `tasks:write`.
Body: `{expected_version, name?, starts_at?, ends_at?}`. `null` or a blank `name` clears it, and the
cycle shows as its number. Answers 200, or 409 after a change.

`DELETE /workspaces/{workspace_id}/projects/{project_id}/cycles/{cycle_id}?expected_version=…`.
Scope: `tasks:write`. Answers 204. Its tasks stay, without a cycle.

```bash
curl -sS -X POST "$ORBIT_WS/projects/$PROJECT_ID/cycles" \
  -H "Authorization: Bearer $ORBIT_TOKEN" -H 'Content-Type: application/json' \
  -d '{"starts_at":"2026-11-23","ends_at":"2026-12-07"}'
```

The counts cover the live tasks, without cancelled and duplicate tasks. `completed_count` and
`estimate_completed` cover the tasks in a `completed` status.

```bash
curl -sS "$ORBIT_WS/projects/$PROJECT_ID/cycles" \
  -H "Authorization: Bearer $ORBIT_TOKEN"
```

`GET /workspaces/{workspace_id}/members?cursor&limit`. Scope: `tasks:read`. The
workspace members, to find assignee IDs. Item: `{user_id, display_name}`. The
API does not show email addresses or roles.

```bash
curl -sS "$ORBIT_WS/members" -H "Authorization: Bearer $ORBIT_TOKEN"
```

### Project trash, members and order

`DELETE /workspaces/{workspace_id}/projects/{project_id}?expected_version=…`. Scope: `tasks:write`.
Moves the project with its tasks to the trash for 30 days. The token's creator must be allowed to
manage the project (403 otherwise). Answers 204, or 409 after a change.

A trashed project is in no token's approved projects. So the project trash needs a token for
**all projects** (403 `workspace_action_forbidden` otherwise):

- `GET /workspaces/{workspace_id}/projects/trash?cursor&limit`. Scope: `tasks:read`. Item: the
  project with `deleted_at`, last trashed first.
- `GET /workspaces/{workspace_id}/projects/trash/{project_id}`. Scope: `tasks:read`. The project
  with its `statuses` and `deleted_by` (`null` for a service account).
- `POST /workspaces/{workspace_id}/projects/{project_id}/restore`. Scope: `tasks:write`. Body:
  `{expected_version}`. Answers 200 with the project, or 409 after a change or when a live project
  took its key.
- `DELETE /workspaces/{workspace_id}/projects/{project_id}/permanent`. Scope: `tasks:write`. Deletes
  a trashed project forever with all its tasks (409 for a live project). Needs a token of a
  workspace owner or admin, or a service account.

`GET /workspaces/{workspace_id}/projects/{project_id}/members`. Scope: `tasks:read`. The members of
the project: `{user_id, created_at}`. Only they (and workspace owners and admins, who are not
listed) see a private project. `POST …/members` with `{user_id}` adds a workspace member (201;
again changes nothing), and `DELETE …/members/{user_id}` removes one (204). Both need a creator who
may manage the project (403 otherwise).

`POST /workspaces/{workspace_id}/projects/{project_id}/statuses/reorder` and
`POST …/milestones/reorder`. Scope: `tasks:write`. Body: `{items: [{id, expected_version,
position}]}`, 1 to 100 items. All moves are written, or none (409 after a change). Answers 200 with
the project's statuses or milestones in their new order. Reordering statuses needs a creator who
may manage the project.

`POST /workspaces/{workspace_id}/projects/{project_id}/cycles/{cycle_id}/move-unfinished`. Scope:
`tasks:write`. Moves the cycle's unfinished tasks to the next cycle, as the web app's "Move
unfinished tasks to next cycle". Answers 200 with `{moved, to_cycle}`, or 409 when no cycle comes
after it.

### Tasks

A task:

```json
{
  "id": "…", "project_id": "…", "number": 12, "status_id": "…",
  "title": "Fix login", "description": "Markdown text", "priority": "high",
  "assignee_ids": ["…"], "label_ids": ["…"],
  "due_start_at": null, "due_at": null,
  "milestone_id": null, "cycle_id": null, "estimate": 3,
  "created_at": "…", "updated_at": "…", "completed_at": null,
  "creator_id": "…", "creator_service_account": null, "external_id": null,
  "version": 4503599627370495,
  "parent_task_id": null,
  "identifier": "ENG-12",
  "url": "https://orbit.example.com/tasks/…?workspace=…"
}
```

`identifier` is the project key and the task number. Use it in `{task}` paths.
`url` is the task's page in the Orbit web app.
`completed_at` is when the task entered a `completed` or `cancelled` status, and `null` while it
is open. `creator_id` is the member who created the task. For a task that a service account
created, `creator_id` is `null` and `creator_service_account` is `{id, name}`. `external_id` is the
source tool's ID of an imported task (see [Import from another tool](#import-from-another-tool)),
and `null` otherwise.

`priority` is `none`, `low`, `medium`, `high` or `urgent`. `estimate` is a number of points from
0 to 1000, or `null`.

`GET /workspaces/{workspace_id}/tasks`. Scope: `tasks:read`. Tasks in the approved projects, newest first,
including completed tasks and sub-issues. Optional parameters: `project_id`,
`status_id`, `assignee_id` (a user ID, or `none` for tasks without an
assignee), `label_id`, `priority`, `milestone_id` (a milestone ID, or `none`), `cycle_id` (a
cycle ID; `current`, `next` or `previous` for that cycle of each task's project; or `none`),
`due_before` and `due_after` (a date such as
`2026-11-12`, which means 00:00 UTC, or a UTC time such as `2026-11-12T15:00:00Z`; both bounds are
exclusive, and tasks without a due date never match), `has_due_date` (`true` or `false`),
`parent_task_id` (only the direct sub-issues
of this task, as a UUID or an identifier such as `ENG-12`; the task must be in an approved project), `q` (text in the title or description, or an
identifier), `sort` (`created_at`, `updated_at`, `priority`, `due_date`,
`title` or `estimate`; default `created_at`; tasks without an estimate come last), `order` (`desc` or `asc`; default `desc`; with
`sort=priority`, `desc` gives `urgent` first and `none` last),
`view_id` (only the tasks of a saved view from `GET /views`, with its filter, its "show completed" option
and, without `sort` and `order`, its order; personal tokens only), `cursor` and `limit`. Keep the same parameters when you send `cursor`.

```bash
curl -sS -G "$ORBIT_WS/tasks" -H "Authorization: Bearer $ORBIT_TOKEN" \
  --data-urlencode "project_id=$PROJECT_ID" --data-urlencode 'q=login' -d limit=20
```

Overdue tasks, oldest first:

```bash
curl -sS -G "$ORBIT_WS/tasks" -H "Authorization: Bearer $ORBIT_TOKEN" \
  --data-urlencode "due_before=$(date -u +%Y-%m-%dT%H:%M:%SZ)" -d sort=due_date -d order=asc
```

`GET /workspaces/{workspace_id}/tasks/{task}`. Scope: `tasks:read`.

```bash
curl -sS "$ORBIT_WS/tasks/ENG-12" -H "Authorization: Bearer $ORBIT_TOKEN"
```

`POST /workspaces/{workspace_id}/tasks`. Scope: `tasks:write`. Response: `201` with the task. Body:
`project_id` and `status_id` (required), `title` (required, 1 to 500
characters), and the optional `description` (Markdown, up to 50,000 bytes),
`priority` (default `none`), `assignee_ids`, `label_ids`, `parent_task_id`,
`milestone_id`, `cycle_id`, `estimate`, `due_start_at` and `due_at` (RFC 3339 times; `due_start_at` needs `due_at` and
must not be after it).
The assignees must be members of the workspace. The labels, the status, the
milestone and the cycle must be in the task's project. The parent and all the tasks related to it must be in
approved projects. `parent_task_id` takes a task UUID or an identifier such as `ENG-12`.

```bash
curl -sS "$ORBIT_WS/tasks" -H "Authorization: Bearer $ORBIT_TOKEN" \
  -H 'Content-Type: application/json' \
  -d "{\"project_id\":\"$PROJECT_ID\",\"status_id\":\"$STATUS_ID\",\"title\":\"Fix login\",\"priority\":\"high\"}"
```

`PATCH /workspaces/{workspace_id}/tasks/{task}`. Scope: `tasks:write`. Response: `200` with the task. Body:
`expected_version` (required, the `version` that you read), and the optional
`title`, `description`, `status_id`, `priority`, `assignee_ids`, `label_ids`,
`due_start_at`, `due_at`, `milestone_id`, `cycle_id`, `estimate`, `parent_task_id` and
`duplicate_of_id`. A list replaces the old list. `duplicate_of_id` (a task UUID or identifier in
an approved project) marks the task as a duplicate of that task and moves it to the Duplicate
status; `null` unmarks it. A missing field keeps its value, `null` clears a due date, the
milestone, the cycle or the estimate, and `parent_task_id: null` detaches the task from its
parent (a new parent is a task UUID or an identifier such as `ENG-12`). A new parent and all the tasks related to it must be in approved projects. If the task changed after you read it, the
response is `409`. This includes edits that people make to the description in
the editor. Read the task again, then try again. All the tasks in the
task's parent and sub-issue tree must be in approved projects.

```bash
curl -sS -X PATCH "$ORBIT_WS/tasks/ENG-12" \
  -H "Authorization: Bearer $ORBIT_TOKEN" -H 'Content-Type: application/json' \
  -d '{"expected_version":4503599627370495,"status_id":"…","assignee_ids":[]}'
```

`DELETE /workspaces/{workspace_id}/tasks/{task}?expected_version={version}`. Scope: `tasks:write`. Moves the task and
its sub-issues to the trash. Response: `204` with no body. The tasks stay in the Orbit
trash for 30 days, where members can restore them in Orbit. Then Orbit deletes them
forever. After the delete, `GET` answers `404` for these tasks.

```bash
curl -sS -X DELETE "$ORBIT_WS/tasks/ENG-12?expected_version=4503599627370495" \
  -H "Authorization: Bearer $ORBIT_TOKEN"
```

Add `permanent=true` to delete the task forever. You cannot undo this. It removes
the task and its sub-issues with their comments, attachments (and the stored
files), relations and description, as the end of the 30 days in the trash does.
Stored files are deleted at the next daily maintenance run. Response: `204`.

- A task that is not in the trash needs its current `expected_version` (`409`
  otherwise). Its sub-issues that are not in the trash go with it.
- It also works for a task that is already in the trash, for example one that
  you moved there earlier. The sub-issues that went to the trash with it go
  too. A task in the trash cannot change, and `GET` cannot read it, so Orbit
  does not check `expected_version` (send the last version that you read, or any
  number).
- Like deleting teamspace docs forever, it needs a token of a workspace owner or
  admin, or a service account token. Other tokens get `403`
  (`workspace_action_forbidden`), and the task does not change.

```bash
curl -sS -X DELETE "$ORBIT_WS/tasks/ENG-12?expected_version=4503599627370495&permanent=true" \
  -H "Authorization: Bearer $ORBIT_TOKEN"
```

### Task trash and activity

`GET /workspaces/{workspace_id}/tasks/trash?cursor&limit`. Scope: `tasks:read`. The tasks in the
trash of the approved projects, last trashed first. An item is the task as `GET /tasks/{task}`
shows it, with `deleted_at` and `trashed_descendant_count` (the sub-issues that a restore brings
back too). A page can hold fewer than `limit` tasks before the last page: follow `next_cursor`.

`GET /workspaces/{workspace_id}/tasks/trash/{task}`. Scope: `tasks:read`. A task in the trash, with
its `status`, `labels` and `deleted_by` (the user who trashed it; `null` for a service account).

`POST /workspaces/{workspace_id}/tasks/{task}/restore`. Scope: `tasks:write`. Body:
`{expected_version}`, the `version` from the trash. Restores the task with the sub-issues that went
to the trash with it. A task whose parent is still in the trash comes back top-level. Answers 200
with the task, or 409 after a change.

```bash
curl -sS -X POST "$ORBIT_WS/tasks/ENG-12/restore" \
  -H "Authorization: Bearer $ORBIT_TOKEN" -H 'Content-Type: application/json' \
  -d '{"expected_version":4503599627370495}'
```

`GET /workspaces/{workspace_id}/tasks/{task}/activity?cursor&limit`. Scope: `tasks:read`. The
task's history, newest first. Item: `{id, action, actor_id, actor_name, metadata, occurred_at}`.
`actor_id` is `null` for system events and service accounts. `metadata` depends on `action`: a task
or project reference outside the approved projects is `null`, with `<field>_hidden: true`.

`POST /workspaces/{workspace_id}/tasks/trash/empty`. Scope: `tasks:write`. Deletes forever every
task and project in the workspace's trash. It covers the whole trash, so it needs a token for all
projects of a workspace owner or admin, or a service account (403 otherwise). Answers 200 with the
counts.

### Bulk updates, order and GitHub links

`POST /workspaces/{workspace_id}/tasks/bulk`. Scope: `tasks:write`. Body: `{updates: [{task,
expected_version, …}]}`: 1 to 100 distinct tasks (UUID or identifier), each with the fields of
`PATCH /tasks/{task}` except `project_id`. All updates are written in one transaction, or none: one
stale `expected_version` answers 409 for the whole request. Answers 200 with `{items,
auto_closed}`: the tasks, and the parents that the sub-issue automation closed.

`POST /workspaces/{workspace_id}/tasks/reorder`. Scope: `tasks:write`. Body: `{items: [{id,
expected_version, position}]}`: 1 to 100 tasks of approved projects (`id` is a UUID or identifier).
Sets their board positions, all or none. Answers 200 with the tasks.

`GET /workspaces/{workspace_id}/tasks/{task}/github-links`. Scope: `tasks:read`. The GitHub issues
and pull requests linked to the task: `{kind, title, url, state, source}`.

### Relations

`GET /workspaces/{workspace_id}/tasks/{task}/relations`. Scope: `tasks:read`. Every relation of
the task, as seen from it: `{id, type, direction, task, created_at}`. `type` is `blocks`,
`related` or `duplicate`. `direction` is `outgoing` when this task is the blocker or the
duplicate. `task` is the other task: `{id, project_id, number, title, status_id, version}`.
Relations to tasks outside the approved projects are left out. `next_cursor` is `null`.

`POST /workspaces/{workspace_id}/tasks/{task}/relations`. Scope: `tasks:write`. Body:
`{type, task_id}`. `type` is `blocks` (this task blocks `task_id`), `blocked_by` or `related`.
`task_id` is a task UUID or identifier in an approved project (422 otherwise). A relation that
already links the two tasks is replaced, and the same relation again is returned unchanged. A
duplicate relation is not replaced (409). Answers 201 with the relation.

`DELETE /workspaces/{workspace_id}/tasks/{task}/relations/{relation_id}`. Scope: `tasks:write`.
Answers 204. Removing a duplicate relation also moves the duplicate out of the Duplicate status.
Make duplicates with `duplicate_of_id` on `PATCH /tasks/{task}`.

```bash
curl -sS -X POST "$ORBIT_WS/tasks/ENG-12/relations" \
  -H "Authorization: Bearer $ORBIT_TOKEN" -H 'Content-Type: application/json' \
  -d '{"type":"blocks","task_id":"ENG-14"}'
```

### Comments

`GET /workspaces/{workspace_id}/tasks/{task}/comments?cursor&limit`. Scope:
`tasks:read`. Oldest first. Item:
`{id, task_id, author_id, author_service_account, body, parent_id, external_id, version,
created_at, updated_at}`. `author_service_account` is `{id, name}` for a comment
that a service account wrote, and `null` otherwise. Show it as the author when
it is set.
`body` is Markdown.

```bash
curl -sS "$ORBIT_WS/tasks/ENG-12/comments" -H "Authorization: Bearer $ORBIT_TOKEN"
```

`POST /workspaces/{workspace_id}/tasks/{task}/comments`. Scope: `tasks:write`.
Body: `{"body": "Markdown", "parent_id"?: "…"}` (1 to 100,000 bytes in UTF-8, not only
spaces). `parent_id` is a comment of the same task: the new comment is its reply. Response:
`201` with the comment. The author is the token's creator, or for a service-account token
the service account.

```bash
curl -sS "$ORBIT_WS/tasks/ENG-12/comments" -H "Authorization: Bearer $ORBIT_TOKEN" \
  -H 'Content-Type: application/json' -d '{"body":"Deployed in build 512."}'
```

`PATCH /workspaces/{workspace_id}/tasks/{task}/comments/{comment_id}`. Scope: `tasks:write`. Body:
`{expected_version, body}`. Only the comment's author can edit it (400 otherwise). Answers 200 with
the comment, or 409 after a change.

`DELETE /workspaces/{workspace_id}/tasks/{task}/comments/{comment_id}?expected_version=…`. Scope:
`tasks:write`. Deletes your own comment, or any comment if you may moderate comments in the
project (400 otherwise). You cannot undo this. Answers 204.

Comment edits and deletes act as the token's creator, so a service-account token gets 404.

### Import from another tool

A token with `tasks:write` and `tasks:import` (**Import mode** in **Settings → API tokens**)
copies tasks from another tool, such as Plane, Jira or a tool of your own, as the built-in
importers do:

- Its writes notify nobody (assignees, mentions) and run no sub-issue automation, also its
  updates. The source tool already decided the state of each task.
- New tasks and comments take the import fields below. A token without `tasks:import` that
  sends one of them gets `403 insufficient_scope`.

| Field | On | Effect |
| --- | --- | --- |
| `external_id` | tasks, comments | The source tool's ID, 1 to 200 characters, unique in the workspace. A second create with the same ID answers `200` with the item that the first create made, and changes nothing. A restarted import thus makes no duplicates. An ID whose task is in the trash, or a comment ID on another task, answers `409`. |
| `number` | tasks | The task number, so `ABC-166` stays `ABC-166`. It must be free in the project, also among tasks in the trash (`409`). Later tasks get numbers after the highest one. |
| `created_at`, `updated_at` | tasks, comments | The original times (RFC 3339). `updated_at` needs `created_at` and must not be before it; it is `created_at` when unset. |
| `completed_at` | tasks | When the task was done. Kept when the status is `completed` or `cancelled`. |
| `creator_id` | tasks | The member shown as the creator. |
| `author_id` | comments | The member shown as the author. |
| `author_name` | comments | An author who is not a member, shown in bold on the first line of the comment. Not with `author_id`. |

`creator_id` and `author_id` must be active members who are not guests (`422`). The audit
log still names the token.

A typical import:

1. Create the projects (`POST /projects`, with a token for all projects), then their statuses
   and labels.
2. Create the tasks, parents before their sub-issues, with `external_id`, `number` and the
   original times.
3. Add the comments, oldest first, with `external_id`, the author and the original times.
4. Link the tasks with `POST /tasks/{task}/relations`, and mark duplicates with
   `duplicate_of_id`.

If the script stops, run it again: each create with an `external_id` that Orbit already has
returns the existing item.

```bash
curl -sS "$ORBIT_WS/tasks" -H "Authorization: Bearer $ORBIT_TOKEN" \
  -H 'Content-Type: application/json' \
  -d "{\"project_id\":\"$PROJECT_ID\",\"status_id\":\"$STATUS_ID\",\"title\":\"Fix login\",
       \"external_id\":\"plane:6f1c…\",\"number\":166,\"created_at\":\"2024-03-01T09:30:00.000Z\"}"
```

### Docs

Docs show only the documents that the token's creator can see, including their
private docs. The `docs:read` and `docs:write` scopes are off by default. Doc
writes act as the token's creator: their name shows as the last editor, and
mentions in the Markdown notify the mentioned members.

`GET /workspaces/{workspace_id}/docs`. Scope: `docs:read`. Item: `{id, parent_id, teamspace_id, private,
title, icon, position, version, updated_at}`. `next_cursor` is always `null`. With the query
`project_id` (an approved project), the list leaves out the docs of teamspaces that hold other
projects' docs: you get the global docs, the docs of that project's teamspace and private docs.

```bash
curl -sS "$ORBIT_WS/docs" -H "Authorization: Bearer $ORBIT_TOKEN"
```

`GET /workspaces/{workspace_id}/docs/{page_id}`. Scope: `docs:read`. The document record, as the MCP
`get_doc` tool returns it. `content` is the editor's BlockNote JSON, and
`markdown` is the same content as GitHub-flavored Markdown, the format that doc
writes take. `POST` and `PATCH` return the same record. Links to other docs show
as `[Title](/docs/{page_id})`. A doc that the token's creator cannot see shows as
`[Untitled](/docs/{page_id})`.

```bash
curl -sS "$ORBIT_WS/docs/$PAGE_ID" -H "Authorization: Bearer $ORBIT_TOKEN"
```

`POST /workspaces/{workspace_id}/docs`. Scope: `docs:write`. Body: `title`
(optional, up to 500 characters), `markdown` (optional, up to 200,000 bytes),
`icon` (optional, up to 64 characters) and the place of the doc: `parent_id`
(the doc goes under this doc, in its space), or for a root doc `teamspace_id` or
`"private": true`. Without a place, the doc goes to the root of the default
teamspace. The doc and its content are created in one step. Response: `201`
with the document record. A doc can have at most 4,000 blocks (paragraphs,
headings, list items and other blocks, nested ones included). Markdown with
more blocks gets `413` (`doc_too_large`), and no doc is created.

```bash
curl -sS "$ORBIT_WS/docs" -H "Authorization: Bearer $ORBIT_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"title":"Release notes","markdown":"## Build 512\n\n- Faster sync"}'
```

`PATCH /workspaces/{workspace_id}/docs/{page_id}`. Scope: `docs:write`. Body:
`expected_version` (required) and optional `title`, `icon` (`null` clears it)
and `markdown`. `markdown` is the doc's whole new content. Send the `markdown`
you read, with your edits. Orbit compares it with the current content and
changes only what you changed, also for people who have the doc open. Blocks you
leave as read stay exactly as they are, with their colors, comments and other
settings. An edited block keeps its settings and its kind: a toggle stays a
toggle, a callout stays a callout. Text you did not touch keeps its colors and
comments. Markdown cannot create colors, alignment, toggles, callouts, comments,
file or sub-page blocks, image sizes or table cell styling: new blocks get only
what Markdown can show. A write that changes nothing (the same title, icon and
content) changes no data: the response has the same `version`, so other clients
that hold it can still write. Orbit checks `expected_version` first, so a stale
version gets `409` also for such a write. If the doc changed after you read it (title, icon or content,
including live edits in the editor), the response is `409`. Read the doc again,
then try again. The `version` in the doc list is the same value. Response: `200`
with the document record. A locked doc answers `423` (`page_locked`).
`markdown` that gives the doc more than 4,000 blocks, or any `markdown` for a
doc that already has more than 4,000 blocks (for example, from the editor), gets
`413` (`doc_too_large`), and nothing changes. Such a doc keeps its title and
icon writes; edit its content in Orbit, or split it into several docs.

```bash
curl -sS -X PATCH "$ORBIT_WS/docs/$PAGE_ID" -H "Authorization: Bearer $ORBIT_TOKEN" \
  -H 'Content-Type: application/json' -d '{"expected_version":4503599627370495,"markdown":"Updated."}'
```

`POST /workspaces/{workspace_id}/docs/{page_id}/move`. Scope: `docs:write`. Body:
`expected_version` (required), `parent_id` (required: a doc ID, or `null` for a
space root) and optional `position`, `teamspace_id`, `private` and `dry_run`. The doc and
its sub-docs go under the new parent and take the parent's space. With
`parent_id: null`, `teamspace_id` moves the doc to that teamspace and
`private: true` moves it to the token creator's private space; with neither,
the doc keeps its space. `position` is the index among the new siblings, from 0;
without it, or past the last sibling, the doc goes last. A doc that moves into a
database always goes last. Response: `200` with the doc record, as `GET` shows
it after the move, and `dropped_values`: how many property values the move
deleted because the new database cannot hold them. With `dry_run: true`, Orbit
does all checks and returns the doc as it would be after the move, with the
`dropped_values` that the move would delete, but changes nothing; `version`
stays the current one, so you can send the same body without `dry_run` to do the
move. Use it before you move a database row to another database or out of
its database. `expected_version` works as
in `PATCH`: a stale value gets `409`. A move under the doc itself or under one
of its sub-docs gets `422`; moving a database under a database gets `409`
(`database_in_database`); moving teamspace docs that others created into a
private space needs a workspace owner or admin (`403`,
`workspace_action_forbidden`).

```bash
curl -sS "$ORBIT_WS/docs/$PAGE_ID/move" -H "Authorization: Bearer $ORBIT_TOKEN" \
  -H 'Content-Type: application/json' \
  -d "{\"expected_version\":4503599627370495,\"parent_id\":\"$PARENT_ID\",\"position\":0}"
```

`DELETE /workspaces/{workspace_id}/docs/{page_id}?expected_version=…`. Scope:
`docs:write`. Moves the doc and its live sub-docs to the trash, where people can
restore them in Orbit or with `POST /docs/{page_id}/restore`. Deleting forever is
not part of the API. Response: `204`. A stale `expected_version` gets `409`. Afterwards `GET` on the
doc answers `404`.

`GET /workspaces/{workspace_id}/docs/{page_id}/versions`. Scope: `docs:read`.
The history of a doc, newest first, without content: `id`, `kind` (`auto`,
`restore` or `import`), `title`, `icon`, `created_at` and `created_by`. Query:
`cursor` and `limit` (1 to 100, default 50), as in other lists. Orbit keeps a
version when someone edits the title or content, at most one per 10 minutes.

`GET /workspaces/{workspace_id}/docs/{page_id}/versions/{version_id}`. Scope:
`docs:read`. One version with its `content` and its `markdown`, the format that
doc writes take. Restoring a version is not part of the API: send the version's
`markdown` in a `PATCH`.

`GET /workspaces/{workspace_id}/docs/trash`. Scope: `docs:read`. The docs in the trash that the
token's creator can see, last trashed first, on one page. Item: `{id, parent_id, teamspace_id,
private, can_purge, title, icon, kind, position, version, updated_at, deleted_at, …}`. Docs trashed
with a parent doc are not listed: restoring the parent brings them back. Docs stay in the trash for
30 days.

`POST /workspaces/{workspace_id}/docs/{page_id}/restore`. Scope: `docs:write`. Body:
`{expected_version}`, the `version` from the trash. Restores the doc with the sub-docs trashed with
it. A row of a database that is in the trash cannot be restored before the database. Answers 200
with the doc, as `GET /docs/{page_id}` shows it, and `restored_descendant_ids`: the IDs of the
sub-docs (and database rows) that came back with it. It answers 409 after a change.

`GET /workspaces/{workspace_id}/docs/{page_id}/comments`. Scope: `docs:read`. The comment threads of
the doc, oldest first, on one page. Item: `{id, quote, resolved, resolved_by, created_by,
created_at, comments}`. A comment is `{id, thread_id, author, body, mentioned_user_ids,
created_at, edited_at, deleted?}`; `author`, `created_by` and `resolved_by` are `{id, name}`, and
`body` is Markdown (empty once deleted).

`POST /workspaces/{workspace_id}/docs/{page_id}/comments`. Scope: `docs:write`. Body:
`{body, thread_id?, quote?}`, with exactly one of `thread_id` (a reply in that thread) and `quote`
(a new thread on 1 to 1,000 characters of the doc's text; 400 otherwise). `body` is Markdown:
paragraphs, bold, italic, code, links and `@Name` mentions; headings and list items become plain
paragraphs. A new thread is not anchored in the doc's text: the doc lists it with its quote.
Answers 201 with the comment.

```bash
curl -sS "$ORBIT_WS/docs/$PAGE_ID/comments" -H "Authorization: Bearer $ORBIT_TOKEN" \
  -H 'Content-Type: application/json' -d '{"body":"Is this final?","quote":"launch date"}'
```

`GET /workspaces/{workspace_id}/teamspaces`. Scope: `docs:read`. The teamspaces that the token's
creator sees, the default one first. Item: `{id, name, icon, is_default, project_id, global}`. Send
an `id` as `teamspace_id` to create or move a doc. `project_id` is the linked project, `null` for
global docs (`global: true`) or a project that is not approved. Each item has a `version` for the
writes below.

#### Teamspace writes

Scope: `docs:write`. As in the web app, any member may add, rename and move teamspaces; deleting
one needs the teamspace delete permission, which owners and admins have (403 otherwise).

- `POST /workspaces/{workspace_id}/teamspaces`. Body: `{name, icon?}`: 1 to 100 characters after
  trimming, and an emoji. The teamspace goes last. Answers 201.
- `PATCH /workspaces/{workspace_id}/teamspaces/{teamspace_id}`. Body: `{expected_version, name?,
  icon?, project_id?}`. `null` clears `icon`, and `project_id: null` makes the teamspace hold
  global docs. A linked project must be approved for the token (400 otherwise), and a project has at
  most one teamspace (409).
- `DELETE /workspaces/{workspace_id}/teamspaces/{teamspace_id}?expected_version=…`. Only an empty
  teamspace, never the last one (409). Answers 204.
- `POST /workspaces/{workspace_id}/teamspaces/{teamspace_id}/move`. Body: `{expected_version,
  position}`. Position 0 makes it the default teamspace. Answers 200 with the list.

#### Search, favorites, copies and locks

- `GET /workspaces/{workspace_id}/docs/search?q=…`. Scope: `docs:read`. The docs that the token's
  creator can see where every word matches the title, the text or a row's values (the last word as
  a prefix), best first.
- `GET /workspaces/{workspace_id}/docs/recent?limit=`. Scope: `docs:read`. The docs that the
  creator opened last.
- `GET /workspaces/{workspace_id}/docs/favorites`. Scope: `docs:read`. The creator's favorites, in
  order. `PUT /docs/{page_id}/favorite` adds one last (a favorite stays where it is), `DELETE
  /docs/{page_id}/favorite` removes it (204), and `POST /docs/favorites/{page_id}/move` with
  `{position}` moves it. Scope: `docs:write`.
- `GET /workspaces/{workspace_id}/docs/{page_id}/descendants`. Scope: `docs:read`. Every live
  sub-doc at any depth, by `parent_id` and `position`.
- `POST /workspaces/{workspace_id}/docs/{page_id}/duplicate`. Scope: `docs:write`. Body:
  `{include_children}`. The copy, titled "<title> (copy)", goes right after the doc. Answers 201
  with it.
- `POST /workspaces/{workspace_id}/docs/{page_id}/lock`. Scope: `docs:write`. Body: `{locked}`. A
  locked doc's title, icon and content cannot change (423). Answers 200 with the doc.
- `POST /workspaces/{workspace_id}/docs/{page_id}/versions/{version_id}/restore`. Scope:
  `docs:write`. Body: `{expected_version}`, the doc's `version`. The content becomes the version's;
  the current content is saved as a version first. Answers 200 with the doc, or 409 after a change.
- `DELETE /workspaces/{workspace_id}/docs/{page_id}/permanent?expected_version=…`. Scope:
  `docs:write`. Deletes a trashed doc forever with its sub-docs (409 when it is not in the trash).
  Teamspace docs need a workspace owner or admin (403); members delete their own private docs.
- `POST /workspaces/{workspace_id}/docs/trash/empty`. Scope: `docs:write`. Deletes forever every
  trashed doc that the creator may delete, as above; the others stay.

`GET /docs/{page_id}` already gives a doc as Markdown, so there is no separate export of one doc.

#### Doc comment threads

Scope: `docs:write`. Reply with `POST /docs/{page_id}/comments` and `thread_id`.

- `PATCH /workspaces/{workspace_id}/docs/{page_id}/threads/{thread_id}/comments/{comment_id}`.
  Body: `{body}` (Markdown, as for a new comment). Only the comment's author may edit it (403).
  Answers 200 with the comment.
- `DELETE …/threads/{thread_id}/comments/{comment_id}`. Only the author (403). The comment keeps
  its place, empty. Answers 204.
- `POST …/threads/{thread_id}/resolve` and `POST …/threads/{thread_id}/reopen`. Answers 200 with
  the thread.
- `DELETE …/threads/{thread_id}`. Deletes the thread with its comments. Its creator may delete it
  while nobody else commented in it (409 otherwise); a member who may moderate comments may delete
  any thread (403 otherwise). Answers 204.

### Databases

A database is a doc with `"kind": "database"` in the doc list. Its rows are docs
too: `GET /docs/{row_id}` returns a row's body, its `database_id` and its
`values`. The database endpoints use the docs scopes and show only the
databases that the token's creator can see. Writes act as the token's creator.

Values use the same keys and names that people see:

- **Keys** are property names or property IDs. A name matches in any letter case
  when no property has the exact name. A name that two properties share gets
  `422`: use the ID.
- **Select and status** take an option name or ID. **Multi-select** takes a list
  of them. A value never adds an option: an unknown option gets `422`.
- **Person** takes a list of user IDs or member emails, or `"me"`.
- **Date** takes `YYYY-MM-DD`, a UTC time (`2026-11-09T14:00:00Z`), the
  interval `start/end`, or `{"start": …, "end": …}`. A time can also have a UTC
  offset (`2026-11-10T14:00:00+01:00`): Orbit stores it as the UTC time
  (`2026-11-10T13:00:00Z`). A date without a time stays a date. Date filters
  take the same forms.
- **Relation** takes a list of page IDs of rows of the related database. Titles
  are not accepted.
- **Text, URL and number** take their value; a percent number stores `0.5`
  for 50%. **Checkbox** takes `true` or `false`. `null` clears a value.
- **Results** show option names, people as `{id, name}`, dates as
  `{start, end}`, related pages as `{id, title}` and unique IDs as
  `"PREFIX-12"` (`"12"` without a prefix), and every property except the title
  and the created and edited ones (the row has these as its own fields). Two
  properties with the same name show under their IDs. Rollups and formulas show
  as computed when the database is read; a formula's people and pages show as
  `{id, name}` and `{id, title}` too. A relation shows only the pages that the
  token's creator can read; the row's `hidden_links` counts the others per
  relation key (absent when there are none).
- **Unique ID filters** take `"PREFIX-12"` (the prefix in any letter case),
  `"12"` or `12`.

`GET /workspaces/{workspace_id}/docs/{page_id}/database`. Scope: `docs:read`.
The database's `title`, `url`, `locked`, `row_count`, `properties` (`id`, `name`,
`type`, `options` with names, and the rest of the `config`), `views` (`id`,
`name`, `type`) and `templates` (`id`, `name`).

`POST /workspaces/{workspace_id}/docs/{page_id}/database/query`. Scope:
`docs:read`. Body (all optional):

- `filter`: an AND/OR group, as a view's filter: up to 3 levels, 10 children
  per group and 20 rules. A rule is `{property_id, operator, value}`, with a
  property name or ID. A rule without a value matches every row.
- `sorts`: up to 10 `{property_id, direction}`, `direction` `asc` (default) or
  `desc`. Empty values sort last.
- `view_id`: starts from this view's filter and sorts. `filter` must then match
  too, and `sorts` replace the view's.
- `limit` (1 to 100, default 50), `cursor` (the `next_cursor` of the previous
  page), and `tz` (an IANA zone such as `Europe/Budapest` for relative dates
  and computed values; default `UTC`).

Response: `{items, next_cursor, total}`. Each item is a row: `id`, `title`,
`icon`, `url`, `created_at`, `created_by`, `updated_at`, `updated_by` and
`values`. Orbit filters and sorts on the server with the same rules as the web
app. A cursor works only with the same filter, sorts, view and zone; another
query with it gets `422` (`cursor`). A `422` names the invalid field in
`errors`, with a message that says what the field needs, such as `limit`:
"Send a limit from 1 to 100."

```bash
curl -sS "$ORBIT_WS/docs/$DATABASE_ID/database/query" -H "Authorization: Bearer $ORBIT_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"filter":{"op":"and","children":[{"property_id":"Status","operator":"is","value":["In progress"]},{"property_id":"Due","operator":"on_or_before","value":"today"}]},"sorts":[{"property_id":"Due","direction":"asc"}],"tz":"Europe/Budapest"}'
```

`POST /workspaces/{workspace_id}/docs/{page_id}/rows`. Scope: `docs:write`.
Body (all optional): `title` (up to 500 characters), `values` (up to 50),
`markdown` (the row's body) and `template_id` (a template of the database, which
fills the title, values and body; `title` and `values` in the body win, and
`markdown` replaces the template's body). The row goes last. Response: `201`
with the row. A database holds at most 5,000 rows (`409`
`database_row_limit`).

`PATCH /workspaces/{workspace_id}/docs/{page_id}/rows/{row_id}`. Scope:
`docs:write`. Body: `values` (up to 50), `title`, or both. The last write wins,
as in the web app: there is no `expected_version`. A locked database still takes
value writes; the lock covers its properties and views. Response: `200` with the
row.

`POST /workspaces/{workspace_id}/docs/{page_id}/rows/trash`. Scope:
`docs:write`. Body: `row_ids` (1 to 100). Each row goes to the trash for 30
days with its sub-pages. IDs that are not live rows of the database are
skipped. Response: `{"trashed": n}`.

```bash
curl -sS -X PATCH "$ORBIT_WS/docs/$DATABASE_ID/rows/$ROW_ID" -H "Authorization: Bearer $ORBIT_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"values":{"Status":"Done","Owner":["ada@example.com"],"Due":"2026-11-20"}}'
```

#### Database schema

The schema endpoints add, change and delete a database's properties (its
columns). They need `docs:write` and act as the token's creator, who must be
able to edit the database. A locked database refuses them (`423`
`page_locked`). `{property}` is a property name or ID, with the same matching
as value keys.

`POST /workspaces/{workspace_id}/docs/{page_id}/database/properties`. Body:
`name` (1 to 200 characters), `type` (any type but `title`: `text`, `number`,
`select`, `multi_select`, `status`, `date`, `person`, `checkbox`, `url`,
`email`, `phone`, `files`, `unique_id`, `relation`, `rollup`, `created_time`,
`created_by`, `last_edited_time`, `last_edited_by`, `button` or `formula`), and
optional `config` and `position` (index among the properties; omitted appends
it). Select, multi-select and status options in `config` can be names or
`{name, color, group}` objects. Colors: `default`, `gray`, `brown`, `orange`,
`yellow`, `green`, `blue`, `purple`, `pink`, `red`. `group` (`todo`,
`in_progress` or `done`) is only for a status. Without `config`, a type gets its
default (a status gets Not started, In progress and Done). Every view shows the
new property. Response: `201` with the property (`id`, `name`, `type`, `config`,
`position`). A database holds at most 100 properties (`409`
`database_property_limit`).

`PATCH /workspaces/{workspace_id}/docs/{page_id}/database/properties/{property}`.
Body: one or more of `name`, `type`, `config`, `add_options` and `position`,
and optional `dry_run`.

- `type` converts the values (it clears the values that cannot convert) and
  gives the type's default config. To set the config too, send it in a later
  request.
- `config` replaces the whole config. Options left out are removed from every
  row. An option sent by name only keeps the ID, color and group of the option
  with that name, so its values stay.
- `add_options` adds options after the current ones and skips names the
  property already has (in any letter case). Send it without `config`.
- `dry_run: true` returns the result without saving it.

Response: `200` with `{property, converted, cleared}`: the counts of values
that a type change converted, and of values that a type change or a removed
option cleared. The title property cannot change its type (`409`
`database_title_property`).

`DELETE /workspaces/{workspace_id}/docs/{page_id}/database/properties/{property}`.
Deletes the property and its values in every row. You cannot undo this. The
title property cannot be deleted (`409` `database_title_property`). Response:
`204`.

```bash
curl -sS -X PATCH "$ORBIT_WS/docs/$DATABASE_ID/database/properties/Status" \
  -H "Authorization: Bearer $ORBIT_TOKEN" -H 'Content-Type: application/json' \
  -d '{"add_options":["Blocked",{"name":"Shipped","color":"green"}]}'
```

#### Create a database

`POST /workspaces/{workspace_id}/databases`. Scope: `docs:write`. Body:

- `title` (up to 500 characters), `icon`, and where it goes, as for `POST /docs`: `parent_id`, or
  `teamspace_id`, or `private: true`. A database cannot go directly under another database
  (`409` `database_in_database`).
- `properties` (optional): the properties after the title property "Name", each
  `{name, type, config}` as in `POST …/database/properties`. Types: `text`, `number`, `select`,
  `multi_select`, `status`, `date`, `person`, `checkbox`, `url`, `email`, `phone`, `files`,
  `created_time`, `created_by`, `last_edited_time`, `last_edited_by`. Add relations, rollups,
  formulas, buttons and unique IDs afterwards. Omitted: a "Tags" multi-select.
- `views` (optional, 1 to 50): each as in [Views](#views). Their filters, sorts, columns and group
  name the properties above and their options. Omitted: one "Table" view.

Response: `201` with the database, as `GET …/database` shows it. The whole database is written
in one step: if a property or view is invalid, nothing is created (`422`).

#### Views

A database has views (table, board, list, calendar, gallery, timeline, form) with a filter, sorts
and visible columns. These endpoints read and change them with property and option **names**, as
`query_database` does. They need `docs:write` (read: `docs:read`) and an editable, unlocked
database (`423` `page_locked`). Add `?link_id=` to work on the views of a [linked view
block](#linked-views) instead of the database's own views.

- `GET /workspaces/{workspace_id}/docs/{page_id}/database/views` lists the views, and `GET
  …/views/{view_id}` returns one.
- `POST …/views` adds a view. Body: `name` (1 to 200 characters), `type` (default `table`),
  `filter` (an AND/OR group as in `query_database`, `property_id` is a property name or ID and
  `value` takes option names), `sorts` (up to 10 `{property_id, direction}`), `columns` (the
  visible properties in order; the others are hidden; the title always shows) and `group_by` (a
  board's select, status, multi-select or person property). Omitted fields give every row, no
  sorts and every property. Response: `201`. A database holds at most 50 views and a linked view
  block at most 20 (`409` `database_view_limit`, `database_link_view_limit`).
- `PATCH …/views/{view_id}` changes `name`, `position`, `filter`, `sorts`, `columns` or
  `group_by`. A field you send replaces what the view has; other fields stay. Send
  `{"op": "and", "children": []}` to clear the filter and `[]` to clear the sorts. The view's
  type cannot change: add a view of the new type. Last write wins: there is no `expected_version`.
- `DELETE …/views/{view_id}` returns `204`. The last view of a database or of a linked view block
  stays (`409` `database_last_view`).

A view returned by the API has `id`, `name`, `type`, `position`, `filter`, `sorts`, `columns` and,
for a board, `group_by`. A property that shares its name with another is shown by ID.

```bash
curl -sS -X POST "$ORBIT_WS/docs/$DATABASE_ID/database/views" \
  -H "Authorization: Bearer $ORBIT_TOKEN" -H 'Content-Type: application/json' \
  -d '{"name":"Dessert","filter":{"property_id":"Type","operator":"is","value":"Dessert"}}'
```

#### Linked views

A linked view block shows an existing database inside a doc, with views of its own. The rows are
not copied, and its filters and columns do not change the database's own views.

`POST /workspaces/{workspace_id}/docs/{page_id}/database/links`, where `{page_id}` is the
database. Scope: `docs:write`. Body: `host_page_id` (the doc that shows the database; you must be
able to edit it) and optional `from_view_id` (an own view whose type and config the block's first
view copies; default: the first view). Orbit adds the block at the end of the host doc, so the
request fails with `409 conflict` if someone changed the doc at the same time (send it again), and
with `423 page_locked` for a locked doc. Response: `201` with `link_id`, `database_id`,
`host_page_id` and the block's `views`. Use `link_id` as `?link_id=` in the view endpoints above.
`GET /docs/{host_page_id}` shows the block as a `database` block in `content` with the same
`linkId`.

`DELETE …/database/links/{link_id}` removes the block from its doc and deletes its views. The
database and its rows stay. Response: `204`.

#### Row order, templates and formula previews

`POST /workspaces/{workspace_id}/docs/{page_id}/rows/{row_id}/move`. Scope: `docs:write`. Body:
`{after_row_id, values?}`: the row goes right after `after_row_id`, or first for `null`. The order is
the one that views without a sort show. `values` (by property name or id, as in `PATCH
…/rows/{row_id}`) are written with the move, as a board drop sets the group value. Answers 200 with
the row.

`POST /workspaces/{workspace_id}/docs/{page_id}/database/templates`. Scope: `docs:write`. Body:
`{name, from_row_id?}`: 1 to 200 characters, and a row or template whose title, icon, values and
body the template copies (else it is empty). Answers 201 with `{id, name}`, as `templates` in `GET
…/database` lists it. A template is a page: change its body with `PATCH /docs/{id}` and its values
with `PATCH …/rows/{id}`. `POST …/rows` with `template_id` fills a new row from it. `PATCH
…/database/templates/{template_id}` with `{name?, position?}` renames or moves it, and `DELETE`
deletes it (204; rows made from it stay). Templates have no version, as in the web app.

`POST /workspaces/{workspace_id}/docs/{page_id}/database/formula/preview`. Scope: `docs:read`. Body:
`{expression, property_id?, tz?}`. Tries the expression without saving it: answers 200 with its
`result_type`, or `error` with `message`, `start` and `end`, and `samples` (`{row_id, value}`) on
the first rows. `property_id` is the formula property being edited, so it cannot read itself.

Not in the API: pressing a button property (its actions are row writes that the row endpoints
already do, and the web app limits presses per person), and submitting a form view (a form is a
web page for people; `POST …/rows` writes every property).

### Chat

Chat is read-only and shows only the conversations that the token's creator can see. A bot that
writes uses a [chat webhook](chat-webhooks.md).

`GET /workspaces/{workspace_id}/chat/conversations`. Scope: `chat:read`. Every visible
conversation, on one page. Item: `{id, kind, name, topic, category_id, position, member_ids,
is_default, self_dm, archived, created_by, created_at, …}`.

`GET /workspaces/{workspace_id}/chat/conversations/{conversation_id}/messages?before&limit`. Scope:
`chat:read`. A page of messages, oldest first: `{items, before, after}`. Send `before` from the
response to read older messages; it is `null` when there are none. `limit` is 1 to 100.

`GET /workspaces/{workspace_id}/chat/threads/{message_id}?before&limit`. Scope: `chat:read`. A
thread: `{root, items, before, after, state}`, where `items` are the replies to the root message.

`GET /workspaces/{workspace_id}/chat/search?q&conversation_id&author_id&has_file&cursor`.
Scope: `chat:read`. Messages in the visible conversations, newest first, 20 per page:
`{items: [{message, conversation_id, ranges}], cursor}`. Every word of `query` must match, the last
one as a prefix (up to 500 characters). Without `query`, send a filter: the result is then the
newest messages that pass it. Send `cursor` from the response for the next page.

```bash
curl -sS "$ORBIT_WS/chat/search?q=deploy" -H "Authorization: Bearer $ORBIT_TOKEN"
```

### Mail

Mail acts as the token's creator, with the web app's rules: you see only the mailboxes that
you are a member of, and their threads, and only your own drafts, snippets, signatures and
settings. A mailbox, thread, message, attachment or draft that you cannot see is `404`, like a
missing one. Mail must be on for the Orbit server (`403 mail_disabled` otherwise). Refusals keep
the web app's stable codes in `code`, such as `send_as_denied`, `sending_paused`,
`send_not_ready`, `no_recipients`, `draft_conflict` and `mail_forbidden`. Mail administration
(domains, send routes, providers, suppressions, mailbox members and addresses) is not in the
public API.

Paths are under `/workspaces/{workspace_id}/mail`:

| Method and path | Scope | Result |
| --- | --- | --- |
| `GET /mailboxes` | `mail:read` | Your mailboxes: `{id, name, kind, status, primary_address, unread}`. |
| `GET /threads?mailbox_id&folder&label_id&starred&unread&sent&cursor&limit` | `mail:read` | Threads newest first, and `next_cursor`. `folder` is `inbox`, `archive`, `spam` or `trash`. |
| `GET /threads/{thread_id}` | `mail:read` | `{thread, messages}`: each message with `body_text` (also for HTML mail), `has_html`, `attachments`, `auth` and, for mail you sent, `delivery`. |
| `PATCH /threads/{thread_id}` | `mail:write` | Move (`folder`, `restore`), mark (`read`), star (`starred`), or label (`add_labels`, `remove_labels`) a thread. |
| `POST /threads/bulk` | `mail:write` | The same change for up to 100 `ids`. |
| `GET /search?q&cursor&limit` | `mail:read` | Threads that match `q`, with the operators `from:`, `to:`, `subject:`, `label:`, `in:`, `is:unread`, `is:starred`, `has:attachment`, `before:` and `after:`. |
| `GET /counts` | `mail:read` | Unread threads in each folder of each mailbox. |
| `GET /attachments/{attachment_id}` | `mail:read` | The file, always as a download. |
| `GET /drafts?mailbox_id`, `GET /drafts/{draft_id}` | `mail:read` | Your drafts. |
| `POST /drafts` | `mail:write` | A new draft (`mode: "new"`, `from_address_id`), or a `reply`, `reply_all`, `forward` or `forward_attached` of a `thread_id`; with `to`, `cc`, `bcc`, `subject` and `body_html`. Your draft of a thread already there is `200`. |
| `PATCH /drafts/{draft_id}`, `DELETE /drafts/{draft_id}` | `mail:write` | Change a draft (send its `version`) or discard it. |
| `POST /drafts/{draft_id}/files?inline`, `PATCH`/`DELETE /drafts/{draft_id}/files/{file_id}` | `mail:write` | Attach a file (multipart, one `file` field), send it as a link (`as_link`), or remove it. |
| `POST /drafts/{draft_id}/check` | `mail:read` | The recipients a send would skip (suppression list). |
| `POST /drafts/{draft_id}/send` | `mail:write` | Sends the draft: `{outbound, suppressed}`. |
| `GET /outbound?view&mailbox_id` | `mail:read` | Your `scheduled` messages or your `outbox`. |
| `POST /outbound/{outbound_id}/cancel` | `mail:write` | Undoes a send before it goes; returns the draft. |
| `GET /labels`; `POST /labels`, `PATCH`/`DELETE /labels/{label_id}` | `mail:read`; `mail:write` | Workspace mail labels. Only owners and admins change them (`403 mail_forbidden`). |
| `GET /contacts?q` | `mail:read` | Up to 8 address suggestions. |
| `GET /from-addresses?thread_id` | `mail:read` | The addresses you can send as, and whether each is `ready`. |
| `GET`, `PUT`, `DELETE /signatures/{address_id}` | `mail:read`; `mail:write` | Your signature for an address. |
| `GET /snippets`; `POST /snippets`, `PATCH`/`DELETE /snippets/{snippet_id}` | `mail:read`; `mail:write` | Your snippets. |
| `GET /settings`, `PUT /settings` | `mail:read`; `mail:write` | `undo_send_seconds`: 0, 5, 10, 20 or 30. |
| `GET`, `PUT /mailboxes/{mailbox_id}/vacation` | `mail:read`; `mail:write` | The vacation reply. The owner of a personal mailbox, or an owner or admin for a shared one, sets it. |

Sending goes through the same path as the web app. `POST /drafts/{draft_id}/send` first runs the
check, then holds the message for your undo-send time (or until `scheduled_at`, an RFC 3339
time). The workspace's send policy, rate limits and recipient limits apply. Until the message
goes, `POST /outbound/{outbound_id}/cancel` returns it to a draft. A second send of the same draft
returns the same message. The HTML of a message is shown only in the web app's sandboxed viewer:
the API gives `body_text`.

```bash
DRAFT=$(curl -sS -X POST "$ORBIT_WS/mail/drafts" -H "Authorization: Bearer $ORBIT_TOKEN" \
  -H 'Content-Type: application/json' \
  -d "{\"mode\":\"new\",\"from_address_id\":\"$FROM\",\"to\":[\"ana@example.com\"],\"subject\":\"Hi\",\"body_html\":\"<p>Hello</p>\"}" | jq -r .id)
curl -sS -X POST "$ORBIT_WS/mail/drafts/$DRAFT/send" -H "Authorization: Bearer $ORBIT_TOKEN"
```

### Notifications, saved views and files

These act as the token's creator, so a service-account token gets `403
workspace_action_forbidden` for notifications and views, and for file uploads and deletes (it can
list and download task files).

**Notifications** (the creator's inbox). Reading needs `tasks:read`, marking read `tasks:write`.
A notification shows only when the token may read what it points to: a task notification needs
the task's project in the grant, a doc mention needs `docs:read`, a chat mention or report needs
`chat:read`, and a GitHub notice needs its repository connected to an approved project. Other
kinds are hidden. "Mark all read" marks only the notifications that the token can list, so a
token for one project does not clear the creator's other unread items.

| Method and path | Scope | Result |
| --- | --- | --- |
| `GET /notifications?unread&cursor&limit` | `tasks:read` | Newest first: `{id, kind, actor_user_id, task_id, comment_id, page_id, chat_conversation_id, …, read_at, created_at}` and `next_cursor`. A page can hold fewer than `limit` items before the last one. |
| `POST /notifications/{notification_id}/read` | `tasks:write` | The notification; an already read one keeps its `read_at`. |
| `POST /notifications/read-all` | `tasks:write` | `{updated}`. At most 5,000 per call. |

**Saved views** (task filters with display options). Reading needs `tasks:read`, writing
`tasks:write`. A view whose filter has a `project` condition naming a project outside the grant
is hidden (`404`), and a create or update cannot name one (`400`). The web app's rules apply:
personal views are their owner's; a workspace view can be changed by its owner or a workspace
owner or admin; only the owner changes `visibility`. View preferences and the sidebar project
order are not in the API: they are the web app's layout state.

| Method and path | Scope | Result |
| --- | --- | --- |
| `GET /views` | `tasks:read` | Every view the creator sees, by name, with `is_favorite` and `favorite_position`. |
| `POST /views`, `GET`/`PATCH`/`DELETE /views/{view_id}` | `tasks:write`, `tasks:read` | `{name, description, icon, color, visibility, state}`; `PATCH` takes `expected_version` (`409` when stale). |
| `PUT`/`DELETE /views/{view_id}/favorite` | `tasks:write` | `204`. Idempotent; a new favorite goes last. |
| `PUT /view-favorites/order` | `tasks:write` | `{view_ids}`: every favorite of `GET /views`, each once. Favorites that the token cannot see keep their places. |
| `GET /tasks?view_id={view_id}` | `tasks:read` | The view's tasks: Orbit runs its filter inside the token's approved projects, with its "show completed" option and, without `sort` and `order`, its order. Other `GET /tasks` parameters narrow the list. |

**Files.** Uploads are `multipart/form-data` with one `file` field, as in the web app. The server
detects the type from the bytes. The server's upload limit (25 MiB per file by default) and the
workspace's storage quota apply (`413 upload_too_large`, `413 storage_quota_exceeded`). The bytes
go to the file store before the database write. Downloads stream the bytes with the stored
`Content-Type`, `Content-Disposition` (with the file name) and `X-Content-Type-Options: nosniff`.
A file that an admin deleted answers `410` with a "Deleted file" image. Each file in a result has
its `download_path`.

| Method and path | Scope | Result |
| --- | --- | --- |
| `GET /tasks/{task}/attachments?cursor&limit` | `tasks:read` | The task's own files, oldest first (not its comments' files, not files placed in the description). |
| `POST /tasks/{task}/attachments` | `tasks:write` | `201` with the file. |
| `DELETE /tasks/{task}/attachments/{attachment_id}` | `tasks:write` | `204`. |
| `GET /tasks/{task}/attachments/{attachment_id}/download` | `tasks:read` | The bytes. |
| `GET`, `POST /tasks/{task}/comments/{comment_id}/attachments`; `DELETE …/{attachment_id}`; `GET …/{attachment_id}/download` | as above | The files of a comment. Adding or deleting one follows the comment's edit rule: its author, or a workspace owner or admin (`403`). |
| `POST /docs/{page_id}/files` | `docs:write` | `201` with `{id, page_id, url, file_name, mime_type, size_bytes, download_path}`. Put `url` in the doc's Markdown (`![name](url)` for an image) with `PATCH /docs/{page_id}`. |
| `GET /docs/{page_id}/files/{file_id}` | `docs:read` | The bytes of a doc's file: `file_id` is the last segment of its `url`. |

To attach a file to a new comment, create the comment, then upload to it. The web app's "comment
with only a file" upload is not in the API.

```bash
curl -sS "$ORBIT_WS/tasks/ENG-12/attachments" -H "Authorization: Bearer $ORBIT_TOKEN" \
  -F "file=@report.pdf"
curl -sS -OJ "$ORBIT_WS/tasks/ENG-12/attachments/$FILE/download" -H "Authorization: Bearer $ORBIT_TOKEN"
```

### Exports

An export is a ZIP archive that another Orbit instance can import. The export
paths have no workspace ID: an export always belongs to the token's workspace
and to the token's creator. The archive's `README.txt` and `manifest.json`
describe its layout.

| Mode | Scope | The archive holds |
| --- | --- | --- |
| `full` | `workspace:export` | The workspace as the token's creator sees it: tasks, projects, teamspaces, docs (also their own private docs), public channels, and the private channels and DMs that they are in. |
| `personal` | `workspace:export:personal` | Only the creator's private docs (with their files and comments), the private channels and DMs that they are in, and the members who appear in them. |

`POST /exports` starts an export. The `mode` must match the token's scope
(else `403 insufficient_scope`). If the token's creator already has an export
of this mode that is queued or running, Orbit returns it and does not start
another one. Thus you can send the request again after an error. A ready
export is not reused: each `POST` after it finished starts a fresh one.

```bash
curl -sS -X POST "$ORBIT_URL/api/public/v1/exports" \
  -H "Authorization: Bearer $ORBIT_TOKEN" -H 'Content-Type: application/json' \
  -d '{"mode": "full"}'
```

```json
{"id": "…", "status": "queued", "mode": "full", "requested_by_name": "Dev",
 "size_bytes": null, "created_at": "…", "completed_at": null, "expires_at": "…"}
```

The status is `202` while the export is `queued` or `running`, and `200` when
it is `ready` or `failed`. `GET /exports/{export_id}` returns the same record;
poll it until the status is `ready` or `failed`. A token sees only the exports
of its creator in its own mode (`404` for others). An export expires seven days
after it is ready.

`GET /exports/{export_id}/download` streams the ZIP
(`Content-Type: application/zip`). It returns `409 export_not_ready` until the
export is `ready`.

### Discord integration

`POST /integrations/discord/events`. Scope: `tasks:write`. A Discord bot sends a
message here to make a task from it. Body: `event_id` (required, 1 to 200
characters, the same ID for a retry), `message` (required), `message_url`
(required, a `https://discord.com/channels/…` link) and `project_id` (optional
when the token has one project only). Response: `201` with
`{"task": {...}, "duplicate": false}`, or `200` with `"duplicate": true` when
Orbit already has a task for this `event_id`. This endpoint was at
`/api/v1/integrations/discord/events` before. Update the bot configuration to
the new path.

This endpoint has its own error codes, not the codes in [Errors](#errors):

| Status | `code` | Cause |
| --- | --- | --- |
| 400 | `invalid_request` | The body is not valid JSON or has unknown fields. |
| 401 | `api_token_required` | No `Authorization: Bearer` token. |
| 401 | `invalid_api_token` | The token is not correct, expired or revoked. |
| 402 | `workspace_read_only`, `trial_limit_reached` | Orbit Cloud: the workspace has no active subscription, or the trial limit is reached. |
| 403 | `api_token_scope_forbidden` | The token does not have `tasks:write`. |
| 409 | `integration_event_conflict` | The `event_id` was used before with a different message or `message_url`. |
| 422 | `validation_failed` | A field is not valid, or `project_id` is not a project of the token. `errors` names the field. |
| 422 | `integration_project_unavailable` | The project or its first status is not available. |
| 500 | `internal_error` | Orbit could not complete the request. Try again later. |

```bash
curl -sS "$ORBIT_URL/api/public/v1/integrations/discord/events" \
  -H "Authorization: Bearer $ORBIT_TOKEN" -H 'Content-Type: application/json' \
  -d '{"event_id":"1234","message":"Login fails","message_url":"https://discord.com/channels/1/2/3"}'
```

## Errors

Errors are RFC 9457 problem documents (`Content-Type: application/problem+json`):

```json
{
  "type": "https://docs.orbit.dev/problems/not_found",
  "title": "Not found", "status": 404, "code": "not_found",
  "detail": "The resource does not exist or this token cannot use it.",
  "instance": "/api/public/v1/workspaces/…/tasks/PRV-3",
  "request_id": "…"
}
```

| Status | `code` | Cause |
| --- | --- | --- |
| 400 | `invalid_request`, `invalid_cursor` | A parameter or the body is not correct. Orbit checks the token and its scope first, so a token without the scope gets `403`, also with an incorrect body. |
| 401 | `unauthorized` | No token, or the token is not correct, expired or revoked. The response has `WWW-Authenticate: Bearer realm="orbit"`. |
| 402 | `workspace_read_only`, `trial_limit_reached` | Orbit Cloud: the workspace has no active subscription, or the trial limit is reached. |
| 403 | `insufficient_scope` | The token does not have the scope of the endpoint. |
| 403 | `workspace_action_forbidden` | The token's member may not do this, for example delete a task forever (`permanent=true`) or run a full export without being a workspace owner or admin. |
| 404 | `not_found` | The resource does not exist, is in the trash, or is not in an approved project. |
| 405 | `method_not_allowed` | The path does not take this HTTP method, for example `PUT /tasks`. The `Allow` header lists the methods that it takes (`GET,HEAD,POST`). Orbit sends this before it checks the token. |
| 409 | `conflict` | The resource changed after you read it (for docs and task descriptions, also through live edits). `conflict.current_version` has the current version. |
| 409 | `task_conflict`, `github_content_read_only`, `github_linked_move` | The change conflicts with the task's current state. |
| 409 | `idempotency_request_running` | The first request with this `Idempotency-Key` has not finished. Retry after the `Retry-After` time. |
| 409 | `export_not_ready` | The export is queued, running or failed, so there is no archive to download yet. |
| 413 | `doc_too_large` | The doc would have, or already has, more than 4,000 blocks. Nothing changed. |
| 422 | `validation_failed`, `parent_cycle`, `parent_invalid` | A value is not valid for the workspace or project, for example an unknown assignee, label, milestone or cycle, or an estimate above 1000. `errors` names the field. |
| 422 | `idempotency_key_reused` | The `Idempotency-Key` was used for another path or body. |
| 423 | `page_locked` | The doc is locked. Unlock it in Orbit to change its title, icon or content. |
| 429 | `rate_limit_exceeded` | Too many requests. Wait as the `Retry-After` header tells (RFC 9110). Orbit sends a number of seconds. |
| 500 | `internal_error` | Orbit could not complete the request, for example because the database is unavailable. Try again later. |
| 503 | `task_busy`, `page_busy`, `service_unavailable` | The task's description or the doc's content is busy on the server, or a service is unavailable. Try again later. |

Orbit returns `404`, not `403`, for a resource in a project that the token
cannot use. Thus a token cannot find out which other resources exist.

## Limits

- Rate limit: the public API has its own budget, apart from the web app's, so
  busy tokens do not slow down the members who use Orbit in the browser. Every
  `/api/public/` endpoint and MCP (`/mcp`) share it. Its limit is the general
  limit (`ORBIT_RATE_LIMIT_GENERAL_PER_MINUTE`) for each client IP address and,
  separately, for each token. All the tokens of one workspace also share a
  workspace budget (`ORBIT_RATE_LIMIT_WORKSPACE_MULTIPLIER` times the general
  limit) on the `/workspaces/{workspace_id}/…` paths. Only successful responses
  (status below 400) count against the workspace budget, so requests with an
  unknown token or a wrong method cannot use it up. Each server counts on its
  own: with N servers, the limits are N times the setting.
- The usual request body limit and request timeout of the Orbit server apply.
- Databases: 100 rows per query page, 100 rows per trash request, and 50 values
  per row write.
- The token's expiry, its revocation and the projects in its grant apply
  immediately. A deleted project leaves the grant.

## Automated checks

```bash
cargo test -p orbit-server --test it public_api::
ORBIT_TEST_POSTGRES_URL=postgres://orbit:orbit@127.0.0.1:15432/orbit \
  cargo test -p orbit-server --test it public_api::
```
