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
3. Select a workspace and the projects that the client may read.
4. Click **Allow read access**. Orbit returns an authorization code to the client.
5. The client exchanges the code using its S256 PKCE verifier and connects.

The only scope is `tasks:read`. Project selection is stored in the grant, not in
client-supplied tool arguments. Access tokens last 15 minutes. Refresh tokens
last 30 days and rotate on every successful refresh. Reuse of a consumed refresh
token revokes the entire connection. Authorization codes last one minute, can
be used once, and require the exact client and registered callback URI.

Open **Account settings → Agentic → Connected apps** to disconnect a client. This stops
all its access and refresh tokens. Current workspace membership, account
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
| `list_tasks` | Optional `project_id`, `query`, `parent_task_id`, `cursor`, `limit` | Tasks and `next_cursor`. |
| `get_task` | Required `task_id` | One task in an approved project. |
| `create_project` | Required `name`, `key`; optional `color` | The new project, with the default statuses. Needs task write and an all-projects connection. |
| `list_task_statuses`, `list_labels` | Required `project_id`; optional `cursor`, `limit` | Statuses or labels (names, colors) of an approved project. |
| `list_members` | Optional `cursor`, `limit` | Workspace members: `user_id` and `display_name`. |
| `list_comments` | Required `task_id`; optional `cursor`, `limit` | Comments on the task, oldest first, and `next_cursor`. |
| `get_me` | None | The connection: its user (`null` for a service account), workspace, scopes and approved project IDs. Needs no permission. |

`list_tasks` searches titles and descriptions. `parent_task_id` (a task ID or an identifier
such as `ENG-12`) lists only the direct sub-issues of that task. The default limit is 50; the valid
range is 1–100. Results include completed tasks and sub-issues, but not deleted
tasks or tasks in deleted projects. Copy `next_cursor` into the next request and
keep the same project and search query.

Task results include title, description, project/status IDs, priority,
assignee/label IDs, due dates, timestamps, version (an opaque number that changes
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
| Task read | `list_projects`, `list_tasks`, `get_task`, `list_task_statuses`, `list_labels`, `list_members`, `list_comments` |
| Task write | `create_task`, `update_task`, `delete_task`, `create_comment`, `create_project` |
| None | `get_me` |
| Docs read | `list_docs`, `get_doc`, `get_database`, `query_database` |
| Docs write | `create_doc`, `update_doc`, `create_database_row`, `update_database_row`, `trash_database_rows` |
| Chat read | `list_conversations`, `list_messages`, `get_thread`, `search_messages` |

Task create takes `project_id`, `status_id`, `title`, optional markdown `description`,
`priority`, `parent_task_id` (creates a sub-issue) and `assignee_ids`. Task update takes `task_id`,
`expected_version`, and optional title, description, status, priority, `assignee_ids` or
`parent_task_id`: a task ID or identifier moves the task under that task, and `null` detaches it.
`assignee_ids` replaces the whole list with member user IDs from `list_members` or `get_me`, and
`[]` unassigns everyone; to claim a task, send the current list plus your own `get_me` user ID.
`create_project` works only for a connection approved for all projects (choose all projects at
consent, or when you create the token): a connection for chosen projects could not use the new
project. `key` is the identifier prefix (1 to 20 letters, digits or underscores, unique in the
workspace).
`create_comment` takes `task_id` and a Markdown `body` and writes as the connection's user (or
its service account). A task
cannot become a sub-issue of itself or of its own sub-issues. Delete takes `task_id` and `expected_version` and moves the task subtree
to the trash for 30 days. With `permanent: true` it deletes the subtree forever instead, with
its comments and attachments; this cannot be undone. It also works for a task that is
already in the trash (then the version is not checked), and needs a connection of a workspace
owner or admin. Read the latest task version before updating or deleting. `update_task`,
`delete_task` and `update_doc` fail with "version changed" if the task or doc changed after
your read, including edits in the editor. Read it again and retry. A malformed task
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
and change nothing. A database outage on a doc or chat tool is a JSON-RPC internal error
(-32603), not "Resource is not available".
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
Chat message and thread tools accept `before` and `limit` (1–100) for pagination.
`search_messages` takes `query` (up to 500 characters; every word must match, the last one as a
prefix) and optional `conversation_id`, `author_id`, `has_file` and `cursor` filters. It returns
20 hits per page, newest first, from conversations the user can read. Copy `cursor` from the
result into the next request to get older hits. An empty query needs at least one filter.

Tool annotations describe operations; server-side grants enforce access.
See the [MCP tools specification](https://modelcontextprotocol.io/specification/2025-06-18/server/tools).
