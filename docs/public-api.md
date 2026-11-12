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
A service-account token can use tasks only, not docs. Orbit refuses to create a service-account token
with `docs:read`, `docs:write` or `chat:read` (`422`). Its writes show the service account as the author:
as the creator of the tasks that it makes, and as the author of its comments.
Its writes do not send notifications (assignments and mentions), because no
member made them.

The audit log records each task and comment write with the ID of the token
(`api_token_id`). A personal token shows its creator as the actor. A
service-account token shows the service account as the actor.

## Scopes

API tokens and MCP OAuth connections use the same five resource scopes. In
**Settings → API tokens**, set the access for each resource to **No access**,
**Read** or **Write**. API tokens also have an import scope and two export
scopes, which MCP does not use.

| Scope | Access in **Settings → API tokens** | Public API endpoints |
| --- | --- | --- |
| `tasks:read` | Tasks: **Read** | `GET` projects, statuses, labels, members, tasks and comments |
| `tasks:write` | Tasks: **Write** | Create, update and delete tasks, add comments, link tasks, create and change projects, statuses and labels, `POST /integrations/discord/events` |
| `tasks:import` | Tasks: **Write** with **Import mode** | With `tasks:write`: the import fields of new tasks and comments. See [Import from another tool](#import-from-another-tool). |
| `docs:read` | Docs: **Read** | `GET /docs`, `GET /docs/{page_id}`, `GET /docs/{page_id}/versions`, `GET /docs/{page_id}/versions/{version_id}`, `GET /docs/{page_id}/database`, `POST /docs/{page_id}/database/query` |
| `docs:write` | Docs: **Write** | `POST /docs`, `PATCH /docs/{page_id}`, `POST /docs/{page_id}/move`, `DELETE /docs/{page_id}`, `POST /docs/{page_id}/rows`, `PATCH /docs/{page_id}/rows/{row_id}`, `POST /docs/{page_id}/rows/trash` |
| `chat:read` | Chat: **Read** | None. Chat is available through MCP only. A bot that reads and writes one channel uses a [chat webhook with events](chat-webhooks.md#events). |
| `workspace:export` | **Workspace export** | `POST /exports` with `mode: "full"`, `GET /exports/{export_id}`, `GET /exports/{export_id}/download` |
| `workspace:export:personal` | **Personal export** | The same, with `mode: "personal"` |

`tasks:write` includes `tasks:read`, and `docs:write` includes `docs:read`.
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
the project, or 400 when the key is already used.

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
label, or 400 when the project already has a label with this name.

`PATCH /workspaces/{workspace_id}/projects/{project_id}/labels/{label_id}`. Scope:
`tasks:write`. Body: `{expected_version, name?, color?}`. A missing field keeps its value.
Answers 200 with the label, or 409 after a change.

`GET /workspaces/{workspace_id}/projects/{project_id}/milestones`. Scope: `tasks:read`. Every
milestone of the project in its order, on one page (`next_cursor` is `null`). Item:
`{id, project_id, name, description, target_at, position, version, task_count, completed_count,
estimate_total, estimate_completed}`. Use the IDs in `milestone_id`.

`GET /workspaces/{workspace_id}/projects/{project_id}/cycles`. Scope: `tasks:read`. Every cycle of
the project by start date, on one page. Item: `{id, project_id, number, name, starts_at, ends_at,
relative, version, task_count, completed_count, estimate_total, estimate_completed}`. A cycle
covers `starts_at` up to `ends_at`, exclusive. `relative` is `current` (the cycle that runs now),
`next` (the first that starts after now), `previous` (the last that ended before now) or `null`.
Use the IDs in `cycle_id`.

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
`parent_task_id` (only the direct sub-issues
of this task, as a UUID or an identifier such as `ENG-12`; the task must be in an approved project), `q` (text in the title or description, or an
identifier), `sort` (`created_at`, `updated_at`, `priority`, `due_date`,
`title` or `estimate`; default `created_at`; tasks without an estimate come last), `order` (`desc` or `asc`; default `desc`; with
`sort=priority`, `desc` gives `urgent` first and `none` last),
`cursor` and `limit`. Keep the same parameters when you send `cursor`.

```bash
curl -sS -G "$ORBIT_WS/tasks" -H "Authorization: Bearer $ORBIT_TOKEN" \
  --data-urlencode "project_id=$PROJECT_ID" --data-urlencode 'q=login' -d limit=20
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
title, icon, position, version, updated_at}`. `next_cursor` is always `null`.

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
space root) and optional `position`, `teamspace_id` and `private`. The doc and
its sub-docs go under the new parent and take the parent's space. With
`parent_id: null`, `teamspace_id` moves the doc to that teamspace and
`private: true` moves it to the token creator's private space; with neither,
the doc keeps its space. `position` is the index among the new siblings, from 0;
without it, or past the last sibling, the doc goes last. A doc that moves into a
database always goes last. Response: `200` with the doc record, as `GET` shows
it after the move, and `dropped_values`: how many property values the move
deleted because the new database cannot hold them. `expected_version` works as
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
restore them in Orbit. Deleting forever, and restoring, are not part of the API.
Response: `204`. A stale `expected_version` gets `409`. Afterwards `GET` on the
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
| 409 | `export_not_ready` | The export is queued, running or failed, so there is no archive to download yet. |
| 413 | `doc_too_large` | The doc would have, or already has, more than 4,000 blocks. Nothing changed. |
| 422 | `validation_failed`, `parent_cycle`, `parent_invalid` | A value is not valid for the workspace or project, for example an unknown assignee, label, milestone or cycle, or an estimate above 1000. `errors` names the field. |
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
cargo test -p orbit-server --test public_api
ORBIT_TEST_POSTGRES_URL=postgres://orbit:orbit@127.0.0.1:15432/orbit \
  cargo test -p orbit-server --test public_api
```
