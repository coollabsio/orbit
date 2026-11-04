# Orbit public REST API

The public REST API lets scripts, CI jobs and the Orbit CLI use Orbit with a
workspace API token. The base path is `<public origin>/api/public/v1`.

This API is separate from the internal `/api/v1` API of the web and mobile apps.
The internal API can change in each release. Use only the public API in your
scripts.

## Authentication

1. As a workspace owner or administrator, open **Settings → API tokens**.
2. Select the projects, the permissions and an expiry. Then create the token.
3. Copy the token (`orb_…`). Orbit shows it one time only.
4. Send the token in the `Authorization` header of each request:

```bash
export ORBIT_URL='https://orbit.example.com'
read -rs -p 'Orbit API token: ' ORBIT_TOKEN; echo
export ORBIT_TOKEN

curl -sS "$ORBIT_URL/api/public/v1/me" -H "Authorization: Bearer $ORBIT_TOKEN"
```

Do not put the token in a URL, in a repository, or in a shared shell history.

The token sets the workspace and the projects. Requests do not contain a
workspace ID. The API does not use cookies, sessions or CSRF tokens. Requests
without an `Origin` header are accepted. If a request has an `Origin` header, the
origin must be the Orbit public origin. The `X-Orbit-Contract` header is not
necessary.

A personal token acts as the person who created it. Orbit checks on each request
that this person is still a member of the workspace and that their account is
not suspended. Task writes and comments show this person as the author. A
service-account token also needs its service account to be active. It can use
tasks only, not docs or chat.

## Scopes

| Scope | Name in **Settings → API tokens** | Endpoints |
| --- | --- | --- |
| `read` | Read | Projects, statuses, labels, members, tasks and comments (read) |
| `write` | Write | Create, update and delete tasks, and add comments |
| `docs:read` | Docs read | `GET /docs`, `GET /docs/{page_id}` |
| `chat:read` | Chat read | `GET /conversations`, `GET /conversations/{id}/messages` |

`GET /me` works with each valid token. The `write` scope does not include `read`.

## Conventions

- Field names are the same as in the internal Orbit records, so clients can
  share models.
- IDs are UUID strings. Times are RFC 3339 strings in UTC with milliseconds, for
  example `2026-11-04T09:30:00.000Z`.
- A `{task}` path segment is a task UUID or a task identifier such as `ENG-12`
  (the letter case is not important).
- Lists return `{"items": [...], "next_cursor": string | null}`. To get the next
  page, send `next_cursor` as `cursor` (for messages, as `before`) with the same
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
  "token": {"id": "…", "name": "ci", "scopes": ["read", "write"],
            "project_ids": ["…"], "expires_at": "2026-12-01T00:00:00.000Z"},
  "workspace": {"id": "…", "name": "Acme"},
  "user": {"id": "…", "email": "dev@example.com", "display_name": "Dev"},
  "service_account": null
}
```

For a service-account token, `user` is `null` and `service_account` is
`{"id", "name"}`.

### Projects, statuses, labels and members

`GET /projects`. Scope: `read`. The approved live projects. Item:
`{id, name, key, color, version, created_at, updated_at}`.

```bash
curl -sS "$ORBIT_URL/api/public/v1/projects" -H "Authorization: Bearer $ORBIT_TOKEN"
```

`GET /projects/{project_id}/statuses?cursor&limit`. Scope: `read`. Item:
`{id, project_id, name, description, color, category, position, version}`.
`category` is `unstarted`, `started`, `completed`, `cancelled` or `duplicate`.

```bash
curl -sS "$ORBIT_URL/api/public/v1/projects/$PROJECT_ID/statuses" \
  -H "Authorization: Bearer $ORBIT_TOKEN"
```

`GET /projects/{project_id}/labels?cursor&limit`. Scope: `read`. Item:
`{id, project_id, name, color, version}`.

```bash
curl -sS "$ORBIT_URL/api/public/v1/projects/$PROJECT_ID/labels" \
  -H "Authorization: Bearer $ORBIT_TOKEN"
```

`GET /members?cursor&limit`. Scope: `read`. The workspace members, to find
assignee IDs. Item: `{user_id, display_name, email, role}`. `role` is `owner`,
`admin` or `member`.

```bash
curl -sS "$ORBIT_URL/api/public/v1/members" -H "Authorization: Bearer $ORBIT_TOKEN"
```

### Tasks

A task:

```json
{
  "id": "…", "project_id": "…", "number": 12, "status_id": "…",
  "title": "Fix login", "description": "Markdown text", "priority": "high",
  "assignee_ids": ["…"], "label_ids": ["…"],
  "due_start_at": null, "due_at": null,
  "created_at": "…", "updated_at": "…", "version": 3,
  "parent_task_id": null
}
```

`priority` is `none`, `low`, `medium`, `high` or `urgent`.

`GET /tasks`. Scope: `read`. Tasks in the approved projects, newest first,
including completed tasks and sub-issues. Optional parameters: `project_id`,
`status_id`, `assignee_id`, `label_id`, `priority`, `q` (text in the title or
description, or an identifier), `cursor` and `limit`.

```bash
curl -sS -G "$ORBIT_URL/api/public/v1/tasks" -H "Authorization: Bearer $ORBIT_TOKEN" \
  --data-urlencode "project_id=$PROJECT_ID" --data-urlencode 'q=login' -d limit=20
```

`GET /tasks/{task}`. Scope: `read`.

```bash
curl -sS "$ORBIT_URL/api/public/v1/tasks/ENG-12" -H "Authorization: Bearer $ORBIT_TOKEN"
```

`POST /tasks`. Scope: `write`. Response: `201` with the task. Body:
`project_id` and `status_id` (required), `title` (required, 1 to 500
characters), and the optional `description` (Markdown, up to 50,000 bytes),
`priority` (default `none`), `assignee_ids`, `label_ids` and `parent_task_id`.
The assignees must be members of the workspace. The labels and the status must
be in the task's project. The parent and all the tasks related to it must be in
approved projects.

```bash
curl -sS "$ORBIT_URL/api/public/v1/tasks" -H "Authorization: Bearer $ORBIT_TOKEN" \
  -H 'Content-Type: application/json' \
  -d "{\"project_id\":\"$PROJECT_ID\",\"status_id\":\"$STATUS_ID\",\"title\":\"Fix login\",\"priority\":\"high\"}"
```

`PATCH /tasks/{task}`. Scope: `write`. Response: `200` with the task. Body:
`expected_version` (required, the `version` that you read), and the optional
`title`, `description`, `status_id`, `priority`, `assignee_ids` and `label_ids`.
A list replaces the old list. If the task changed after you read it, the
response is `409`. Read the task again, then try again. All the tasks in the
task's parent and sub-issue tree must be in approved projects.

```bash
curl -sS -X PATCH "$ORBIT_URL/api/public/v1/tasks/ENG-12" \
  -H "Authorization: Bearer $ORBIT_TOKEN" -H 'Content-Type: application/json' \
  -d '{"expected_version":3,"status_id":"…","assignee_ids":[]}'
```

`DELETE /tasks/{task}?expected_version=N`. Scope: `write`. Moves the task and
its sub-issues to the trash. Response: `204` with no body. The tasks stay in the Orbit
trash for 30 days.

```bash
curl -sS -X DELETE "$ORBIT_URL/api/public/v1/tasks/ENG-12?expected_version=4" \
  -H "Authorization: Bearer $ORBIT_TOKEN"
```

### Comments

`GET /tasks/{task}/comments?cursor&limit`. Scope: `read`. Oldest first. Item:
`{id, task_id, author_id, body, parent_id, version, created_at, updated_at}`.
`body` is Markdown.

```bash
curl -sS "$ORBIT_URL/api/public/v1/tasks/ENG-12/comments" \
  -H "Authorization: Bearer $ORBIT_TOKEN"
```

`POST /tasks/{task}/comments`. Scope: `write`. Body: `{"body": "Markdown"}`
(1 to 100,000 characters). Response: `201` with the comment.

```bash
curl -sS "$ORBIT_URL/api/public/v1/tasks/ENG-12/comments" \
  -H "Authorization: Bearer $ORBIT_TOKEN" -H 'Content-Type: application/json' \
  -d '{"body":"Deployed in build 512."}'
```

### Docs

Docs and chat show only the content that the token's creator can see, including
their private docs, private channels and direct messages. The scopes are off by
default.

`GET /docs`. Scope: `docs:read`. Item: `{id, parent_id, teamspace_id, private,
title, icon, position, version, updated_at}`. `next_cursor` is always `null`.

```bash
curl -sS "$ORBIT_URL/api/public/v1/docs" -H "Authorization: Bearer $ORBIT_TOKEN"
```

`GET /docs/{page_id}`. Scope: `docs:read`. The document record, as the MCP
`get_doc` tool returns it.

```bash
curl -sS "$ORBIT_URL/api/public/v1/docs/$PAGE_ID" -H "Authorization: Bearer $ORBIT_TOKEN"
```

### Chat

`GET /conversations`. Scope: `chat:read`. The conversations, as the MCP
`list_conversations` tool returns them. `next_cursor` is always `null`.

```bash
curl -sS "$ORBIT_URL/api/public/v1/conversations" -H "Authorization: Bearer $ORBIT_TOKEN"
```

`GET /conversations/{conversation_id}/messages?before&limit`. Scope:
`chat:read`. Messages in time order, oldest first. To get older messages, send
`next_cursor` as `before`.

```bash
curl -sS "$ORBIT_URL/api/public/v1/conversations/$CONVERSATION_ID/messages?limit=20" \
  -H "Authorization: Bearer $ORBIT_TOKEN"
```

### Discord integration

`POST /integrations/discord/events`. Scope: `write`. A Discord bot sends a
message here to make a task from it. Body: `event_id` (required, 1 to 200
characters, the same ID for a retry), `message` (required), `message_url`
(required, a `https://discord.com/channels/…` link) and `project_id` (optional
when the token has one project only). Response: `201` with
`{"task": {...}, "duplicate": false}`, or `200` with `"duplicate": true` when
Orbit already has a task for this `event_id`. This endpoint was at
`/api/v1/integrations/discord/events` before. Update the bot configuration to
the new path.

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
  "instance": "/api/public/v1/tasks/PRV-3",
  "request_id": "…"
}
```

| Status | `code` | Cause |
| --- | --- | --- |
| 400 | `invalid_request`, `invalid_cursor` | A parameter or the body is not correct. |
| 401 | `unauthorized` | No token, or the token is not correct, expired or revoked. The response has `WWW-Authenticate: Bearer realm="orbit"`. |
| 402 | `workspace_read_only`, `trial_limit_reached` | Orbit Cloud: the workspace has no active subscription, or the trial limit is reached. |
| 403 | `insufficient_scope` | The token does not have the scope of the endpoint. |
| 404 | `not_found` | The resource does not exist, is in the trash, or is not in an approved project. |
| 409 | `conflict` | `expected_version` is old. `conflict.current_version` has the current version. |
| 409 | `task_conflict`, `github_content_read_only`, `github_linked_move` | The change conflicts with the task's current state. |
| 422 | `validation_failed`, `parent_cycle`, `parent_invalid` | A value is not valid for the workspace or project, for example an unknown assignee or label. `errors` names the field. |
| 429 | `rate_limit_exceeded` | Too many requests. Wait for the number of seconds in `Retry-After`. |
| 503 | `task_busy`, `service_unavailable` | Try again later. |

Orbit returns `404`, not `403`, for a resource in a project that the token
cannot use. Thus a token cannot find out which other resources exist.

## Limits

- Rate limit: the general API limit (`ORBIT_RATE_LIMIT_GENERAL_PER_MINUTE`)
  for each client IP address and, separately, for each token.
- The usual request body limit and request timeout of the Orbit server apply.
- The token's expiry, its revocation and the projects in its grant apply
  immediately. A deleted project leaves the grant.

## Automated checks

```bash
cargo test -p orbit-server --test public_api
ORBIT_TEST_POSTGRES_URL=postgres://orbit:orbit@127.0.0.1:15432/orbit \
  cargo test -p orbit-server --test public_api
```
