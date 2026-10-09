# Read-only task MCP

Orbit serves a read-only MCP endpoint at `<http.public_origin>/mcp`. It uses
Streamable HTTP in the existing server. No separate process is needed.

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
   and projects, then click **Allow read access**.
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
ORBIT__HTTP__PUBLIC_ORIGIN=https://devserver.tail661ee3.ts.net:18888 \
ORBIT_DEV_API_PORT=18080 ORBIT_DEV_WEB_PORT=18888 just dev
```

Replace the hostname with your server's Tailscale hostname. Stop the old Run
environment before starting this command. A Tailscale HTTPS proxy must already
forward port 18888 to `http://127.0.0.1:18888`; setting the public origin does
not enable HTTPS. Use the HTTPS address for Orbit sign-in too.

Add the HTTPS endpoint to your selected backend:

```bash
claude mcp add --scope user --transport http orbit https://devserver.tail661ee3.ts.net:18888/mcp

codex mcp add orbit --url https://devserver.tail661ee3.ts.net:18888/mcp
codex mcp login orbit

grok mcp add --scope user --transport http orbit https://devserver.tail661ee3.ts.net:18888/mcp
grok mcp doctor orbit
```

No fixed callback port is required. Let the client choose its callback port.
If the CLI runs on a remote server and your browser runs on your computer,
the browser must still reach the CLI's loopback callback. Forward the port
shown in the callback URL through SSH, or use an existing remote callback
forwarding method. Tailscale access to Orbit alone does not forward this
callback. If remote OAuth cannot be completed, use the read-only API-token
alternative.

### Disconnect and troubleshooting

- To revoke access, open **Settings → Sessions → Connected apps** in Orbit and
  disconnect the client.
- If the protected-resource address does not match, correct
  `ORBIT__HTTP__PUBLIC_ORIGIN`, restart Orbit, then authenticate again.
- If tool discovery fails after a server update, reconnect the server in
  Claude's `/mcp` menu or start a new Claude session.

## OAuth connection

Orbit includes its own authorization server. No external identity provider is
needed. In your MCP client, choose **Streamable HTTP** and **OAuth**, then enter
`<http.public_origin>/mcp`.

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

Open **Settings → Sessions → Connected apps** to disconnect a client. This stops
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
`ORBIT__HTTP__PUBLIC_ORIGIN` value. Do not expose a development server to the
internet without separate access controls. Production still requires HTTPS.

## API-token alternative

API tokens remain supported for scripts or clients with custom HTTP headers.
As a workspace owner or administrator, open **Settings → API tokens**, enable
**Read**, disable **Write**, select projects, and set an expiry. Configure
`Authorization: Bearer <token>` on the MCP client. Never put the token in a URL
or a repository. A service-account token does not bypass the creator's current
workspace membership check for repository reads.

## Tools

| Tool | Arguments | Result |
| --- | --- | --- |
| `list_projects` | None | Approved live projects only. |
| `list_tasks` | Optional `project_id`, `query`, `cursor`, `limit` | Tasks and `next_cursor`. |
| `get_task` | Required `task_id` | One task in an approved project. |

`list_tasks` searches titles and descriptions. The default limit is 50; the valid
range is 1–100. Results include completed tasks and sub-issues, but not deleted
tasks or tasks in deleted projects. Copy `next_cursor` into the next request and
keep the same project and search query.

Task results include title, description, project/status IDs, priority,
assignee/label IDs, due dates, timestamps, and version. Embedded task relations
and ancestors are omitted so they cannot disclose titles from unapproved
projects. Descriptions remain user-authored text; clients must treat them as
data, not as instructions.

There are no document, mail, chat, administration, or mutation tools.

## Deployment

Set `http.public_origin` to the public HTTPS origin. The reverse proxy must keep
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
Orbit's existing rate and transport limits.

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
export ORBIT_URL='https://orbit.example.com'
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
