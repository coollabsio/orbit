# Orbit public REST API

The public REST API lets scripts, CI jobs and the Orbit CLI use Orbit with a
workspace API token. The base path is `<public origin>/api/public/v1`.
The OpenAPI document in the repository (`packages/api/src/generated/openapi.json`,
or `orbit openapi --output <file>`) describes this API under the tag `public-api`.

This API is separate from the internal `/api/v1` API of the web and mobile apps.
The internal API can change in each release. Use only the public API in your
scripts.

## Authentication

1. As a workspace owner or administrator, open **Settings → API tokens**.
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

API tokens and MCP OAuth connections use the same five scopes. In
**Settings → API tokens**, set the access for each resource to **No access**,
**Read** or **Write**.

| Scope | Access in **Settings → API tokens** | Public API endpoints |
| --- | --- | --- |
| `tasks:read` | Tasks: **Read** | `GET` projects, statuses, labels, members, tasks and comments |
| `tasks:write` | Tasks: **Write** | Create, update and delete tasks, add comments, `POST /integrations/discord/events` |
| `docs:read` | Docs: **Read** | `GET /docs`, `GET /docs/{page_id}` |
| `docs:write` | Docs: **Write** | `POST /docs`, `PATCH /docs/{page_id}` |
| `chat:read` | Chat: **Read** | None. Chat is available through MCP only. |

`tasks:write` includes `tasks:read`, and `docs:write` includes `docs:read`.
`GET /me` works with each valid token.

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

### Projects, statuses, labels and members

`GET /workspaces/{workspace_id}/projects`. Scope: `tasks:read`. The approved live projects. Item:
`{id, name, key, color, version, created_at, updated_at}`.

```bash
curl -sS "$ORBIT_WS/projects" -H "Authorization: Bearer $ORBIT_TOKEN"
```

`GET /workspaces/{workspace_id}/projects/{project_id}/statuses?cursor&limit`. Scope: `tasks:read`. Item:
`{id, project_id, name, description, color, category, position, version}`.
`category` is `unstarted`, `started`, `completed`, `cancelled` or `duplicate`.

```bash
curl -sS "$ORBIT_WS/projects/$PROJECT_ID/statuses" \
  -H "Authorization: Bearer $ORBIT_TOKEN"
```

`GET /workspaces/{workspace_id}/projects/{project_id}/labels?cursor&limit`.
Scope: `tasks:read`. Item: `{id, project_id, name, color, version}`. Use the
IDs in `label_ids`.

```bash
curl -sS "$ORBIT_WS/projects/$PROJECT_ID/labels" \
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
  "created_at": "…", "updated_at": "…", "version": 4503599627370495,
  "parent_task_id": null,
  "identifier": "ENG-12",
  "url": "https://orbit.example.com/tasks/…?workspace=…"
}
```

`identifier` is the project key and the task number. Use it in `{task}` paths.
`url` is the task's page in the Orbit web app.

`priority` is `none`, `low`, `medium`, `high` or `urgent`.

`GET /workspaces/{workspace_id}/tasks`. Scope: `tasks:read`. Tasks in the approved projects, newest first,
including completed tasks and sub-issues. Optional parameters: `project_id`,
`status_id`, `assignee_id` (a user ID, or `none` for tasks without an
assignee), `label_id`, `priority`, `parent_task_id` (only the direct sub-issues
of this task, as a UUID or an identifier such as `ENG-12`; the task must be in an approved project), `q` (text in the title or description, or an
identifier), `sort` (`created_at`, `updated_at`, `priority`, `due_date` or
`title`; default `created_at`), `order` (`desc` or `asc`; default `desc`; with
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
`due_start_at` and `due_at` (RFC 3339 times; `due_start_at` needs `due_at` and
must not be after it).
The assignees must be members of the workspace. The labels and the status must
be in the task's project. The parent and all the tasks related to it must be in
approved projects. `parent_task_id` takes a task UUID or an identifier such as `ENG-12`.

```bash
curl -sS "$ORBIT_WS/tasks" -H "Authorization: Bearer $ORBIT_TOKEN" \
  -H 'Content-Type: application/json' \
  -d "{\"project_id\":\"$PROJECT_ID\",\"status_id\":\"$STATUS_ID\",\"title\":\"Fix login\",\"priority\":\"high\"}"
```

`PATCH /workspaces/{workspace_id}/tasks/{task}`. Scope: `tasks:write`. Response: `200` with the task. Body:
`expected_version` (required, the `version` that you read), and the optional
`title`, `description`, `status_id`, `priority`, `assignee_ids`, `label_ids`,
`due_start_at`, `due_at` and `parent_task_id`. A list replaces the old list. A missing field keeps
its value, `null` clears a due date, and `parent_task_id: null` detaches the task from its
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

### Comments

`GET /workspaces/{workspace_id}/tasks/{task}/comments?cursor&limit`. Scope:
`tasks:read`. Oldest first. Item:
`{id, task_id, author_id, author_service_account, body, parent_id, version,
created_at, updated_at}`. `author_service_account` is `{id, name}` for a comment
that a service account wrote, and `null` otherwise. Show it as the author when
it is set.
`body` is Markdown.

```bash
curl -sS "$ORBIT_WS/tasks/ENG-12/comments" -H "Authorization: Bearer $ORBIT_TOKEN"
```

`POST /workspaces/{workspace_id}/tasks/{task}/comments`. Scope: `tasks:write`.
Body: `{"body": "Markdown"}` (1 to 100,000 bytes in UTF-8, not only spaces). Response: `201` with
the comment. The author is the token's creator, or for a service-account token
the service account.

```bash
curl -sS "$ORBIT_WS/tasks/ENG-12/comments" -H "Authorization: Bearer $ORBIT_TOKEN" \
  -H 'Content-Type: application/json' -d '{"body":"Deployed in build 512."}'
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
| 403 | `workspace_action_forbidden` | The token's member may not do this, for example delete a task forever (`permanent=true`) without being a workspace owner or admin. |
| 404 | `not_found` | The resource does not exist, is in the trash, or is not in an approved project. |
| 405 | `method_not_allowed` | The path does not take this HTTP method, for example `PUT /tasks`. The `Allow` header lists the methods that it takes (`GET,HEAD,POST`). Orbit sends this before it checks the token. |
| 409 | `conflict` | The resource changed after you read it (for docs and task descriptions, also through live edits). `conflict.current_version` has the current version. |
| 409 | `task_conflict`, `github_content_read_only`, `github_linked_move` | The change conflicts with the task's current state. |
| 413 | `doc_too_large` | The doc would have, or already has, more than 4,000 blocks. Nothing changed. |
| 422 | `validation_failed`, `parent_cycle`, `parent_invalid` | A value is not valid for the workspace or project, for example an unknown assignee or label. `errors` names the field. |
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
- The token's expiry, its revocation and the projects in its grant apply
  immediately. A deleted project leaves the grant.

## Automated checks

```bash
cargo test -p orbit-server --test public_api
ORBIT_TEST_POSTGRES_URL=postgres://orbit:orbit@127.0.0.1:15432/orbit \
  cargo test -p orbit-server --test public_api
```
