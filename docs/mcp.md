# Orbit MCP

Orbit serves an MCP endpoint at `<public origin>/mcp`. It uses
Streamable HTTP in the existing server. No separate process is needed.

In Orbit, open **Account settings → Agentic** to copy the endpoint and the setup
command for each client below.

## Client setup

### Claude

Start Orbit with `just dev`, then add the server:

```bash
claude mcp add \
  --scope user --transport http orbit http://127.0.0.1:8888/mcp
```

Install the CLI for your selected backend and make it available on your `PATH`.

1. Start the Claude CLI and enter `/mcp`.
2. Select **orbit** and authenticate. Sign in to Orbit, select a workspace
   and projects, then choose optional permissions and allow access.
3. In Jean, open **Settings → MCP Servers** and enable **orbit** under
   **Claude**. Start a new Claude session.
4. Ask: **“Use orbit to list my projects and tasks.”**

### Codex

With Orbit running locally:

```bash
codex mcp add orbit --url http://127.0.0.1:8888/mcp
codex mcp login orbit
```

Complete Orbit sign-in and project approval in your browser. Use `/mcp` in the
Codex terminal to inspect the connection. In Jean, enable **orbit** under
**Settings → MCP Servers → Codex**, then start a new Codex session.

For configuration details, see the [official Codex MCP documentation](https://developers.openai.com/codex/mcp/).

### Grok

With Orbit running locally:

```bash
grok mcp add --scope user --transport http orbit http://127.0.0.1:8888/mcp
grok mcp doctor orbit
```

Use `/mcps` in the Grok terminal to inspect the server and complete authentication
when requested. In Jean, enable **orbit** under
**Settings → MCP Servers → Grok**, then start a new Grok session.

If your Grok version cannot complete OAuth, use a read-only Orbit API token with
the `--header "Authorization: Bearer <token>"` option when adding the server.
Use user scope, not a shared project configuration, and never commit the token.
See [API-token setup](#api-token-alternative).

Each backend has its own server configuration and credentials. Adding Orbit to
Claude does not configure Codex or Grok.

### Remote server through Tailscale

The public origin must match the HTTPS address used by the MCP client. For this
repository's development server, configure Jean's Run command as follows:

```bash
ORBIT_PUBLIC_ORIGIN=https://devbox.example.ts.net:18888 \
ORBIT_DEV_API_PORT=18080 ORBIT_DEV_WEB_PORT=18888 just dev
```

Replace the hostname with your server's Tailscale hostname. Stop the old Run
environment before starting this command. A Tailscale HTTPS proxy must already
forward port 18888 to `http://127.0.0.1:18888`; setting the public origin does
not enable HTTPS. Use the HTTPS address for Orbit sign-in too.

Add the HTTPS endpoint to your selected backend:

```bash
claude mcp add --scope user --transport http orbit https://devbox.example.ts.net:18888/mcp

codex mcp add orbit --url https://devbox.example.ts.net:18888/mcp
codex mcp login orbit

grok mcp add --scope user --transport http orbit https://devbox.example.ts.net:18888/mcp
grok mcp doctor orbit
```

No fixed callback port is required. Let the client choose its callback port.
If the CLI runs on a remote server and your browser runs on your computer,
start sign-in with manual callback entry:

```bash
claude mcp login orbit --no-browser
# or
codex mcp login orbit --no-browser
```

Open the printed authorization link in your browser. On Orbit's consent page,
select **Client runs on a remote server**, choose access, and approve.
Click **Copy callback URL** and paste the full URL into the waiting terminal.
The code expires after one minute; restart sign-in if it expires. If clipboard
access fails, select the URL field and copy it manually. Denying access also
shows a callback URL so the terminal can finish with the denial.

Clients without manual callback entry still need SSH forwarding of their
loopback callback port, or the API-token alternative. Tailscale access to Orbit
alone does not forward a client's loopback callback.

### Disconnect and troubleshooting

- To revoke access, open **Account settings → Agentic → Connected apps** in Orbit and
  disconnect the client.
- If the protected-resource address does not match, correct
  `ORBIT_PUBLIC_ORIGIN`, restart Orbit, then authenticate again.
- If tool discovery fails after a server update, reconnect the server in
  Claude's `/mcp` menu or start a new Claude session.

## OAuth connection

Orbit includes its own authorization server. No external identity provider is
needed. In your MCP client, choose **Streamable HTTP** and **OAuth**, then enter
`<public origin>/mcp`.

1. The client discovers Orbit's authorization server and registers as a public
   client with a callback URI.
2. Orbit opens its sign-in page if you are not signed in.
3. Select one or more workspaces, and for each the projects that the client may read.
4. Click **Allow read access**. Orbit returns an authorization code to the client.
5. The client exchanges the code using its S256 PKCE verifier and connects.

One connection can cover several workspaces. Each has its own projects, and the
permissions you choose apply to all of them. When a connection covers more than one
workspace, every tool takes a `workspace_id` argument (the IDs are in `get_me`),
and a call without it fails with a message that says so. With one workspace the argument
is not needed. API tokens belong to one workspace.

The only scope is `tasks:read`. Project selection is stored in the grant, not in
client-supplied tool arguments. Access tokens last 15 minutes. Refresh tokens
last 30 days and rotate on every successful refresh. Reuse of a consumed refresh
token revokes the entire connection. Authorization codes last one minute, can
be used once, and require the exact client and registered callback URI.

Open **Account settings → Agentic → Connected apps** to disconnect a client. Each
workspace of a connection is one row: disconnecting a row removes that workspace, and
the tokens stop when the last workspace is gone. Leaving a workspace removes it from
your connections in the same way. Current workspace membership, account
suspension, project availability, token expiry, audience, and revocation are
checked on every MCP request. Removing a project does not grant access to any
other project. Existing connections survive server restarts.

Only public clients (`token_endpoint_auth_method: none`) are supported. Register
HTTPS callbacks or HTTP loopback callbacks (`127.0.0.1`, `localhost`, `::1`);
fragments and embedded credentials are not permitted. Dynamic client
registration is supported. Remote client ID metadata documents are not fetched,
and custom-scheme callbacks and client-credentials grants are not supported.
Client names are supplied by clients and are not verified identities.

Discovery endpoints:

- `/.well-known/oauth-authorization-server`
- `/.well-known/oauth-protected-resource/mcp` (also available at the root)

Protocol endpoints: `/oauth/register`, `/oauth/authorize`, `/oauth/token`, and
`/oauth/revoke`. Token and revocation requests use form URL encoding. Tokens and
codes are stored only as SHA-256 hashes. Browser consent uses the signed-in
session, a user-bound nonce, and normal same-origin checks. Public protocol
endpoints use no cookies and support CORS without credentials.

## Development

Jean's configured command is:

```bash
ORBIT_DEV_API_PORT=18080 ORBIT_DEV_WEB_PORT=18888 just dev
```

Use **http://127.0.0.1:18888/mcp** for the Jean environment, with issuer
**http://127.0.0.1:18888**. The API listens on port 18080. The development watcher
sets the public origin to the web port, and Vite proxies discovery and OAuth
protocol routes without proxying the `/oauth/consent` React page.

With plain `just dev`, use **http://127.0.0.1:8888/mcp** instead. Use the same host
throughout the connection; `localhost` and `127.0.0.1` have different cookies and
origins. Restart an already-running dev command after changing its port or origin.

A local client can connect to these URLs. A cloud-hosted client cannot reach your
loopback server. It needs a public HTTPS endpoint and a matching
`ORBIT_PUBLIC_ORIGIN` value. Do not expose a development server to the
internet without separate access controls. Production still requires HTTPS.

## API-token alternative

API tokens remain supported for scripts or clients with custom HTTP headers.
As a workspace owner or administrator, open **Settings → API tokens**, set
Tasks to **Read**, select projects, and set an expiry. Configure
`Authorization: Bearer <token>` on the MCP client. Never put the token in a URL
or a repository. A personal token stops working when its creator leaves the
workspace. A service-account token does not depend on its creator: it works until
it is revoked. When you revoke a token and the service account has no other
token that is not revoked and not expired, Orbit disables the service account; its past work keeps its name. A new token with
the same service-account name enables it again.

The same tokens also work with the public REST API at `/api/public/v1`, for
scripts, CI jobs and the Orbit CLI. That API serves tasks and docs, not chat. See
[Orbit public REST API](public-api.md).

## Tools

| Tool | Arguments | Result |
| --- | --- | --- |
| `list_projects` | None | Approved live projects only. |
| `list_tasks` | Optional `project_id`, `query`, `parent_task_id`, `milestone_id`, `cycle_id`, `cursor`, `limit` | Tasks and `next_cursor`. |
| `get_task` | Required `task_id` | One task in an approved project. |
| `create_project` | Required `name`, `key`; optional `color` | The new project, with the default statuses. Needs task write and an all-projects connection. |
| `list_task_statuses`, `list_labels` | Required `project_id`; optional `cursor`, `limit` | Statuses or labels (names, colors) of an approved project. |
| `list_milestones` | Required `project_id` | Every milestone of an approved project, in order: `name`, `description`, `target_at` and the progress counts. |
| `list_cycles` | Required `project_id` | Every cycle of an approved project, by start date: `number`, `name`, `starts_at`, `ends_at` (exclusive), the progress counts, and `relative`. |
| `list_members` | Optional `cursor`, `limit` | Workspace members: `user_id` and `display_name`. |
| `list_comments` | Required `task_id`; optional `cursor`, `limit` | Comments on the task, oldest first, and `next_cursor`. |
| `update_project` | Required `project_id`, `expected_version`; optional `name`, `color` | The project after the change. |
| `create_task_status`, `update_task_status` | Required `project_id` and, to create, `name` and `category`; to update, `status_id` and `expected_version`. Optional `name`, `category`, `color`, `description`, `position` | The status. |
| `create_label`, `update_label` | Required `project_id` and, to create, `name`; to update, `label_id` and `expected_version`. Optional `name`, `color` | The label. |
| `list_task_activity` | Required `task_id`; optional `cursor`, `limit` | The task's history, newest first: `action`, `actor_id`, `actor_name`, `metadata`, `occurred_at`, and `next_cursor`. |
| `update_comment` | Required `task_id`, `comment_id`, `expected_version`, `body` (Markdown) | The comment, with its new `version`. |
| `delete_comment` | Required `task_id`, `comment_id`, `expected_version` | `{"deleted": true}`. |
| `list_task_trash` | Optional `cursor`, `limit` | Tasks in the trash of approved projects, last trashed first: the `get_task` view plus `deleted_at` and `trashed_descendant_count`, and `next_cursor`. |
| `get_trashed_task` | Required `task_id` | One trashed task: the `get_task` view plus `deleted_at`, `status`, `labels` and `deleted_by`. |
| `restore_task` | Required `task_id`, `expected_version` | The restored task, as `get_task` returns it. |
| `create_milestone` | Required `project_id`, `name`; optional `description`, `target_at` | The milestone, as `list_milestones` returns it. |
| `update_milestone` | Required `project_id`, `milestone_id`, `expected_version`; optional `name`, `description`, `target_at` (`null` clears each), `position` | The milestone. |
| `create_cycle` | Required `project_id`; optional `name`, `starts_at`, `ends_at` | The cycle with `relative`, as `list_cycles` returns it. |
| `update_cycle` | Required `project_id`, `cycle_id`, `expected_version`; optional `name` (`null` clears), `starts_at`, `ends_at` | The cycle. |
| `delete_milestone`, `delete_cycle` | Required `project_id`, `milestone_id` or `cycle_id`, `expected_version` | `{"deleted": true}`. Its tasks stay, without a milestone or cycle. |
| `delete_task_status`, `delete_label` | Required `project_id`, `status_id` or `label_id`, `expected_version` | `{"deleted": true}`. A deleted label comes off its tasks. |
| `list_task_relations` | Required `task_id` | The task's relations (`blocks`, `related`, `duplicate`): `id`, `type`, `direction` and the other `task`. Relations to tasks outside the approved projects are left out. |
| `get_me` | Optional `workspace_id` | The connection: its user (`null` for a service account), workspace, scopes and approved project IDs, and `workspaces`: every workspace it covers with its own scopes and project IDs. Needs no permission. |

`list_tasks` searches titles and descriptions. `parent_task_id` (a task ID or an identifier
such as `ENG-12`) lists only the direct sub-issues of that task. The default limit is 50; the valid
range is 1–100. Results include completed tasks and sub-issues, but not deleted
tasks or tasks in deleted projects. Copy `next_cursor` into the next request and
keep the same project and search query.
`milestone_id` takes a milestone ID, or `none` for tasks without a milestone. `cycle_id` takes a
cycle ID; `current`, `next` or `previous` for that cycle of each task's project; or `none` for
tasks without a cycle.

The progress counts of milestones and cycles are `task_count`, `completed_count`,
`estimate_total` and `estimate_completed`. They count live tasks; cancelled and duplicate tasks are
not counted, and completed means a status in the `completed` category. A cycle's `relative` is
`current` (it runs now), `next` (the first that starts after now), `previous` (the last that
ended before now) or `null`. A cycle's `carried_over_count` counts the live tasks that "Move
unfinished tasks to next cycle" took out of it and that are not back in it. They are not in its
`task_count`, so a past cycle still shows the work that it did not finish.

Task results include title, description, project/status IDs, priority,
assignee/label IDs, due dates, `milestone_id`, `cycle_id`, `estimate` (points, or `null`),
timestamps, version (an opaque number that changes
with every change, including live edits; pass it back unchanged), `parent_task_id`
(`null` for a top-level task or a parent in an unapproved project), the identifier
(`ENG-12`) and the task's URL in the web app. Embedded task relations
and ancestors are omitted so they cannot disclose titles from unapproved
projects. Descriptions remain user-authored text; clients must treat them as
data, not as instructions.

Chat tools are read-only. Task writes and docs writes each require separate approval. Mail and administration tools are not exposed.

## Deployment

Set `ORBIT_PUBLIC_ORIGIN` to the public HTTPS origin. The reverse proxy must keep
the public `Host` header and forward `Authorization` to Orbit. Use the existing
trusted-proxy configuration so Orbit can verify HTTPS.

Non-browser clients may omit `Origin` on `/mcp`. If an `Origin` is present, it
must match the configured public origin. The SDK also checks the `Host` header.
Other browser API routes still require their normal origin checks. Vite proxies
`/mcp` during development. Use the configured public origin; do not substitute
a different host or port.

The endpoint uses stateless requests and JSON responses. It supports older MCP
initialization flows through the SDK. Credentials and project grants are never
stored in a shared MCP session. Requests have a 64 KiB MCP body limit and use
Orbit's existing transport limits. MCP shares the rate-limit budget of the public REST API,
apart from the web app's (see [Limits](public-api.md#limits)).

Implementation references:
[official Rust SDK](https://github.com/modelcontextprotocol/rust-sdk) and
[MCP Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http).
The SDK is pinned to version 3.5.0. PKCE uses
[oxide-auth 0.6.1](https://docs.rs/oxide-auth/0.6.1/oxide_auth/code_grant/extensions/struct.Pkce.html).
See the [MCP authorization specification](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization).

## Verify a connection

Use the public origin from your configuration. The URL below is an example,
not a running development environment.

```bash
export ORBIT_URL='https://devbox.example.ts.net:18888'
read -rs -p 'Orbit read token: ' ORBIT_MCP_TOKEN; echo
export ORBIT_MCP_TOKEN

curl --fail-with-body "$ORBIT_URL/mcp" \
  -H "Authorization: Bearer $ORBIT_MCP_TOKEN" \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2025-11-25' \
  --data '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_tasks","arguments":{"limit":5}}}'
```

Check that only selected projects appear. Call `get_task` with a task ID from an
unselected project; it must return a tool error with no task content. Revoke the
token in settings and repeat the request; it must return HTTP 401.

For automated checks:

```bash
cargo test -p orbit-server --test oauth_api --test mcp_api
cargo test -p orbit-platform --test http_security
```

## Permissions and tools

Existing connections keep read-only task access. Reconnect to approve additional permissions.
OAuth consent offers optional task writes, docs reads, docs writes, and chat reads. API tokens
use the same scopes: `tasks:read`, `tasks:write` (task create, update and delete; includes
`tasks:read`), `docs:read`, `docs:write` (doc create and update; includes `docs:read`), and
`chat:read`. The API token scopes `workspace:export` and `workspace:export:personal` are for
the public REST API only (`docs/public-api.md`); a token with only an export scope gets no MCP
tools.
Docs and chat permissions default to off. They apply to content visible to the connected
person in the selected workspace, including their private docs, private channels and DMs.
Service-account tokens support tasks only; docs and chat require a personal identity.

| Permission | Tools |
| --- | --- |
| Task read | `list_projects`, `list_tasks`, `get_task`, `list_task_statuses`, `list_labels`, `list_milestones`, `list_cycles`, `list_members`, `list_comments`, `list_task_relations`, `list_task_activity`, `list_task_trash`, `get_trashed_task` |
| Task write | `create_task`, `update_task`, `delete_task`, `restore_task`, `create_comment`, `update_comment`, `delete_comment`, `create_project`, `update_project`, `create_task_status`, `update_task_status`, `delete_task_status`, `create_label`, `update_label`, `delete_label`, `create_milestone`, `update_milestone`, `delete_milestone`, `create_cycle`, `update_cycle`, `delete_cycle`, `create_task_relation`, `delete_task_relation` |
| None | `get_me` |
| Docs read | `list_docs`, `get_doc`, `list_doc_versions`, `get_doc_version`, `list_doc_trash`, `list_doc_comments`, `list_teamspaces`, `get_database`, `query_database`, `list_database_views` |
| Docs write | `create_doc`, `update_doc`, `delete_doc`, `move_doc`, `restore_doc`, `create_doc_comment`, `create_database_row`, `update_database_row`, `trash_database_rows`, `create_database_property`, `update_database_property`, `delete_database_property`, `create_database`, `create_database_view`, `update_database_view`, `delete_database_view`, `create_database_link`, `delete_database_link` |
| Chat read | `list_conversations`, `list_messages`, `get_thread`, `search_messages` |

Task create takes `project_id`, `status_id`, `title`, optional markdown `description`,
`priority`, `parent_task_id` (creates a sub-issue), `assignee_ids`, `label_ids`, `milestone_id`,
`cycle_id` and `estimate` (0 to 1000 points). The milestone and the cycle must be in the task's
project. Task update takes `task_id`, `expected_version`, and optional title, description, status,
priority, `assignee_ids`, `label_ids`, `milestone_id`, `cycle_id`, `estimate`, `duplicate_of_id` or `parent_task_id`:
a task ID or identifier moves the task under that task, and `null` detaches it. `null` also clears
the milestone, the cycle or the estimate. `duplicate_of_id` (a task in an approved project) marks the
task as its duplicate and moves it to the Duplicate status; `null` unmarks it.
`assignee_ids` replaces the whole list with member user IDs from `list_members` or `get_me`, and
`[]` unassigns everyone; to claim a task, send the current list plus your own `get_me` user ID.
`label_ids` replaces the whole list with label IDs of the task's project from `list_labels`, and
`[]` removes all labels. A label of another project, or the same label twice, is refused.
`create_project` works only for a connection approved for all projects (choose all projects at
consent, or when you create the token): a connection for chosen projects could not use the new
project. `key` is the identifier prefix (1 to 20 letters, digits or underscores, unique in the
workspace).
`create_comment` takes `task_id` and a Markdown `body` and writes as the connection's user (or
its service account).
`create_task_relation` takes `task_id`, `type` and `other_task_id` (both tasks in approved
projects). `type` is `blocks` (`task_id` blocks `other_task_id`), `blocked_by` or `related`. A
relation that already links the two tasks is replaced. A duplicate relation, or a `blocks`
relation in the other direction, is refused: delete it first. `delete_task_relation` takes
`task_id` and a `relation_id` from `list_task_relations`; removing a duplicate relation also moves
the duplicate out of the Duplicate status. In a relation, `direction` is `outgoing` when the task
is the blocker or the duplicate. A task
cannot become a sub-issue of itself or of its own sub-issues. Delete takes `task_id` and `expected_version` and moves the task subtree
to the trash for 30 days. With `permanent: true` it deletes the subtree forever instead, with
its comments and attachments; this cannot be undone. It also works for a task that is
already in the trash (then the version is not checked), and needs a connection of a workspace
owner or admin. Read the latest task version before updating or deleting. The tools that take
`expected_version` fail with "version changed" if the item changed after your read, including edits in the editor. Read it again and retry. A malformed task
identifier is an invalid-params error. Task writes require
approval for every project in the related parent/sub-issue tree, since task automations can
change related tasks. Task tools return a projection that excludes references outside grants.
Doc create takes optional `title`, Markdown `markdown`, and a place: `parent_id`, or for a
root doc `teamspace_id` or `private: true` (omitted: the default teamspace). Doc update takes
`page_id`, `expected_version`, and optional `title` or `markdown`. `markdown` is the whole new content: send the Markdown that `get_doc` returned, with your
edits. Only what you change changes, also in open editors. An update that changes nothing
keeps the doc's version. Unchanged blocks keep their colors,
comments and other settings, and edited blocks keep their settings and kind. New blocks get only
what Markdown can express (no colors, toggles, callouts, comments or file blocks). `get_doc`, `create_doc` and `update_doc` return the doc with
`markdown` (its content as Markdown) next to the BlockNote `content`. Sub-page links use the
page's title, or "Untitled" for pages you cannot see. Doc writes act as the connected person
and fail on a locked doc. A doc can have at most 4,000 blocks (nested ones included):
`create_doc` with more, `update_doc` with `markdown` that gives more, or `update_doc` with
`markdown` for a doc that already has more, fail with "A doc can have at most 4,000 blocks"
and change nothing.
`delete_doc` takes `page_id` and `expected_version` and moves the doc and its sub-docs to the
trash, where people can restore them; deleting forever is not available. `move_doc` takes
`page_id`, `expected_version` and a place: `parent_id`, or for a root move `teamspace_id` or
`private: true` (omitted: the doc's own space), and an optional `position` among the new
siblings (default: last). Its result has `dropped_values`: the database property values that
the move deleted. `list_doc_versions` takes `page_id` and optional `cursor`, `limit`, and lists
the stored versions newest first, without content. `get_doc_version` takes `page_id` and
`version_id` and returns that version with `markdown`; to restore it, send that Markdown to
`update_doc`.
`update_comment` and `delete_comment` need a personal connection (a service account is
refused). Only the author can edit a task comment; the author, or a member who may moderate
comments in the project, can delete it.
`list_task_activity` leaves out references outside the grant: the ID of a task or project in a
project that is not approved becomes `null` with `<field>_hidden: true`, and its title is left
out. `actor_id` is `null` for system events and service accounts; then
`metadata.actor_service_account_name` names the account.
Items stay in the trash for 30 days. `list_task_trash` leaves out tasks of projects that are not
approved, so a page can hold fewer than `limit` items: follow `next_cursor`. Restoring a task also
restores the sub-issues trashed with it; a task whose parent is still in the trash comes back at
the top level. A task that is only in a trashed project is not in the task trash.
`list_doc_trash` lists only docs trashed directly, with `version` for `restore_doc`; restoring a
doc restores its sub-docs, and a database row comes back only after its database.
Milestone and cycle dates take an RFC 3339 time or `YYYY-MM-DD` (a milestone's day is noon UTC, a
cycle's day is midnight UTC). A cycle's `ends_at` is exclusive. `create_cycle` without dates
starts where the project's last cycle ends and lasts as many days; the first cycle starts today
(UTC) and lasts two weeks. The cycles of a project cannot overlap, but they can touch.
`delete_task_status` and `delete_label` need a member who may manage the project. A status that a
task uses (tasks in the trash too) or the Duplicate status cannot be deleted.
`list_teamspaces` lists every teamspace that the connected person can see, whatever the approved
projects. Use an `id` as `teamspace_id` in `create_doc` or `move_doc`; `is_default` marks the
teamspace that `create_doc` uses without one. `project_id` is `null` for global docs
(`global: true`) or for a project that is not approved.
`list_doc_comments` takes `page_id` and returns `threads`: each with `quote`, `resolved`,
`resolved_by`, `created_by` and `comments` (Markdown `body`, `author` `{id, name}`,
`mentioned_user_ids`, `edited_at`, and `deleted` for a deleted comment). `create_doc_comment`
takes `page_id`, a Markdown `body` and either `thread_id` (a reply) or `quote` (a new thread on
that text). It returns the new comment with its `thread_id`. A doc comment keeps only paragraphs,
bold, italic, code, links and `@Name` mentions; headings and list items become paragraphs. A new
thread keeps the quoted text, but it is not anchored in the doc text.
Status `category` is `unstarted`, `started`, `completed` or `cancelled`; Orbit makes the
Duplicate status itself, and it cannot change its category. A label name must be new in its
project. Colors are `#rrggbb`. A database outage on a doc or chat tool is a JSON-RPC internal error
(-32603), not "Resource is not available".
`list_docs` takes an optional `project_id` (an approved project). A teamspace can hold the docs
of one project (Project settings → Docs). With `project_id`, the list leaves out the docs of
teamspaces of other projects: you get the global docs (teamspaces without a project), that
project's docs and private docs. This filter is not a permission: without `project_id`, and in
`get_doc`, every visible doc stays available.
Databases are docs with `kind: "database"` in `list_docs`, and their rows are docs: `get_doc`
of a row also returns its `database_id` and `values`. `get_database` takes `database_id` and
returns the properties (with option names), views, templates and `row_count`.
`query_database` takes `database_id` and optional `filter`, `sorts`, `view_id`, `limit`
(1–100, default 50), `cursor` and `tz` (an IANA zone for relative dates and computed values;
default UTC). The filter is a view filter: an AND/OR group of up to 3 levels, 10 children per
group and 20 rules, such as `{"op": "and", "children": [{"property_id": "Status", "operator":
"is", "value": ["Done"]}]}`. With `view_id`, the query starts from that view's filter and sorts;
`filter` must then match too, and `sorts` replace the view's. It returns `items`, `next_cursor`
and `total`; a cursor works only with the same query. Orbit filters and sorts on the server
with the web app's rules. `create_database_row` takes `database_id` and optional `title`,
`values`, `markdown` (the body) and `template_id`. `update_database_row` takes `database_id`,
`row_id`, and `values`, `title` or both; the last write wins and no version is needed.
`trash_database_rows` takes `database_id` and 1–100 `row_ids` and returns `trashed`. Values
use property names or IDs as keys (a name that two properties share is an error), option
names or IDs (values never add options), user IDs, member emails or `"me"` for people, and
`YYYY-MM-DD`, a UTC time, a time with a UTC offset (stored as UTC) or `start/end` for dates,
and page IDs for relations. Results show option names, people (also of a formula) as
`{id, name}`, related pages that the token's creator can read as `{id, title}` (`hidden_links`
counts the others) and unique IDs as `"PREFIX-12"`; a unique ID filter takes `"PREFIX-12"` or
`12`. An invalid value or filter fails with "Invalid database input", its field, such as
`values.Status`, and what the field needs. A write has at most 50 values. See
[Databases in the public API](public-api.md#databases) for every value shape.
The schema tools change a database's properties (columns) as the connected person, who must be
able to edit the database. `create_database_property` takes `database_id`, `name`, `type` and
optional `config` and `position`. `update_database_property` takes `database_id`, `property` (a
name or ID) and one or more of `name`, `type`, `config`, `add_options` and `position`, and
optional `dry_run`; it returns the property with `converted` and `cleared` value counts. A new
type converts the values and gives the type's default config; send `config` in a later call.
`config` replaces the whole config: options left out are removed from every row. Select,
multi-select and status options can be plain names or `{name, color, group}` objects; an option
sent by name only keeps the ID, color and group of the option with that name, so its values stay.
`add_options` appends options and skips names the property already has; it cannot be sent with
`config`. `delete_database_property` takes `database_id` and `property` and deletes the property
with its values; the title property cannot be deleted or change its type.
`create_database` takes optional `title`, `icon`, `parent_id`, `teamspace_id` or `private`, `properties`
(`{name, type, config}` after the title property "Name") and `views`, and writes the database in one
step. A view is `{name, type, filter, sorts, columns, group_by}` with property and option names, as
in `query_database`. `list_database_views`, `create_database_view`, `update_database_view` and
`delete_database_view` work on a database's views, or on a linked view block's views with `link_id`.
`update_database_view` replaces only the fields you send. `create_database_link` takes
`database_id` and `host_page_id` and shows the database in that doc as a linked view block at the
end of the doc, with views of its own; it returns `link_id`. `delete_database_link` removes the
block and its views. See [Public API](public-api.md#views) for the rules and limits.
Chat message and thread tools accept `before` and `limit` (1–100) for pagination.
`search_messages` takes `query` (up to 500 characters; every word must match, the last one as a
prefix) and optional `conversation_id`, `author_id`, `has_file` and `cursor` filters. It returns
20 hits per page, newest first, from conversations the user can read. Copy `cursor` from the
result into the next request to get older hits. An empty query needs at least one filter.

Tool annotations describe operations; server-side grants enforce access.
See the [MCP tools specification](https://modelcontextprotocol.io/specification/2025-06-18/server/tools).
