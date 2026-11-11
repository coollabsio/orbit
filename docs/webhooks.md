# Outgoing webhooks

Orbit can post a signed JSON event to your HTTPS endpoint when a task is
created, changed or moved to the trash. Use webhooks to keep another system in
sync with Orbit without polling the [public REST API](public-api.md).

Each webhook subscription belongs to one workspace. It names a URL, the event
types it takes, and the projects whose tasks send events.

## Add a webhook

1. As a workspace owner or administrator, open **Settings → Webhooks**.
2. Click **Add webhook**. Enter a name and the URL of your endpoint.
3. Select the events and at least one project. Only tasks in the selected
   projects send events.
4. Click **Add webhook**, then copy the signing secret (`whsec_…`). Orbit shows
   it one time only.

The server must have an app key (`ORBIT_APP_KEY`, see
[operations](operations.md)) to store signing secrets. Without it, Orbit
refuses to add a webhook or rotate its secret.

To change the secret, click **Rotate secret**. Orbit signs every request from
then on with the new secret, retries of older events included.

To stop the events for a while, turn off the webhook's switch. Orbit cancels
its pending deliveries. You can send a cancelled or failed delivery again with
**Deliveries → Retry**. **Delete** removes the webhook, its delivery log and
any delivery not yet sent.

The same actions are available in the internal API under
`/api/v1/workspaces/{workspace_id}/webhooks`.

## Event types

| Type | Sent when |
| --- | --- |
| `task.created` | A task is created: in the web or mobile app, through the API or MCP, or by the GitHub or Discord integration. |
| `task.updated` | A task's fields change. Also sent when a task is restored from the trash, closed automatically with its sub-issues, moved to another parent, or marked or unmarked as a duplicate. |
| `task.deleted` | A task moves to the trash. Trashing a task also trashes its sub-issues, and each one sends its own event. |

Orbit sends no event when a task is deleted forever from the trash (purged).
The `task.deleted` event was sent when the task went to the trash. Moving a
whole project to the trash, or deleting it forever, sends no task events.

A bulk update sends one `task.updated` event per changed task. Reordering tasks
and adding or removing task relations other than duplicates send no event.

Live edits of a task's description reach Orbit's database at most every
10 minutes while someone types. Each save sends one `task.updated` event with
`"changes": {"description": true}`. The event holds the saved description, not
every keystroke.

## Request

Orbit sends each event as an HTTP `POST` with these headers:

| Header | Value |
| --- | --- |
| `Content-Type` | `application/json` |
| `User-Agent` | `Orbit-Webhooks/1.0` |
| `webhook-id` | The event id. It is the same on every retry of the event, and for every webhook that receives it. |
| `webhook-timestamp` | When this attempt was sent, in Unix seconds. |
| `webhook-signature` | `v1,` and the base64 HMAC-SHA256 signature (see [Verify the signature](#verify-the-signature)). |
| `orbit-event-type` | The event type, such as `task.updated`. |

The headers and the signature follow the
[Standard Webhooks](https://www.standardwebhooks.com/) specification, so its
libraries can verify them.

Orbit counts any `2xx` answer as delivered and ignores the response body. It
does not follow redirects: a `3xx` answer is a failure. Each attempt times out
after 10 seconds.

Outside development, Orbit sends only to `https` URLs on public addresses. It
checks the address of a host name on each attempt, so a name that later
resolves to a private address is refused.

## Event body

Every event has the same envelope. Orbit serializes the body once, when the
change is saved, and sends the same bytes on every retry.

```json
{
  "id": "019a7c3e-5b1f-7d2a-9c4e-2f8b6a1d0e93",
  "type": "task.updated",
  "schema_version": 1,
  "occurred_at": "2026-11-10T09:41:07.312Z",
  "workspace_id": "0199f0a2-1c3d-7e4f-8a5b-6c7d8e9f0a1b",
  "resource": { "type": "task", "id": "019a7c2d-0e1f-7a2b-8c3d-4e5f6a7b8c9d" },
  "actor": {
    "user_id": "0199f0a2-1c3d-7e4f-8a5b-111111111111",
    "api_token_id": null,
    "service_account_id": null,
    "service_account_name": null
  },
  "origin": { "action": "task.updated" },
  "data": {
    "project_id": "0199f0a2-1c3d-7e4f-8a5b-222222222222",
    "task": {
      "id": "019a7c2d-0e1f-7a2b-8c3d-4e5f6a7b8c9d",
      "project_id": "0199f0a2-1c3d-7e4f-8a5b-222222222222",
      "number": 12,
      "identifier": "ENG-12",
      "status_id": "0199f0a2-1c3d-7e4f-8a5b-333333333333",
      "title": "Rotate the TLS certificates",
      "description": "Before **Friday**.",
      "priority": "high",
      "assignee_ids": ["0199f0a2-1c3d-7e4f-8a5b-111111111111"],
      "label_ids": [],
      "due_start_at": null,
      "due_at": "2026-11-14T17:00:00.000Z",
      "created_at": "2026-11-10T09:12:44.020Z",
      "updated_at": "2026-11-10T09:41:07.312Z",
      "version": 4127309856221345,
      "parent_task_id": null
    },
    "changes": {
      "priority": { "from": "medium", "to": "high" },
      "due": { "start": null, "end": "2026-11-14T17:00:00.000Z" }
    }
  }
}
```

### Envelope fields

| Field | Description |
| --- | --- |
| `id` | The event id, as in the `webhook-id` header. |
| `type` | `task.created`, `task.updated` or `task.deleted`. |
| `schema_version` | `1`. A change that removes or renames a field gets a new version. |
| `occurred_at` | When the change was saved. |
| `workspace_id` | The task's workspace. |
| `resource` | `{"type": "task", "id": …}`. |
| `actor` | Who made the change. `user_id` is `null` for a service-account token and for automatic changes. `api_token_id` is set for a change made with an API token or MCP; `service_account_id` and `service_account_name` for a service-account token. |
| `origin.action` | What happened in Orbit, such as `task.created`, `task.updated`, `task.restored`, `task.auto_closed`, `task.parent_changed`, `task.marked_duplicate`, `task.unmarked_duplicate` or `task.deleted`. |
| `data` | Depends on the type (see below). |

### `task.created` and `task.updated`

`data.task` has the fields of a task in the public REST API, and:

- `identifier`: the project key and the task number, such as `ENG-12`.
- `parent_task_id`: the parent task, or `null`. It is `null` also when the
  parent is in a project that this webhook does not select.

`data.changes` names what changed. It is `{}` for `task.created`. For a field
edit, each changed field has its old and new value, as in the task's activity:
`project`, `status`, `priority`, `assignees` (`added`, `removed`), `labels`
(`added`, `removed`), `due` (`start`, `end`) and `title`. A description change
is `"description": true`. For the other actions, `changes` names one field,
set to `true`:

| `origin.action` | `changes` |
| --- | --- |
| `task.restored` | `{"deleted_at": true}` |
| `task.auto_closed` | `{"status": true}` |
| `task.parent_changed` | `{"parent": true}` |
| `task.marked_duplicate`, `task.unmarked_duplicate` | `{"duplicate_of": true}` |

### A task moves to another project

A move sends `task.updated` to the webhooks of both projects, with the same
event `id`. A webhook that selects the new project gets the whole task, with
`changes.project` set to `{"from": <old project>, "to": <new project>}`. When it
does not select the old project, `from` is `null`, and so is
`changes.status.from`, the old project's status.

A webhook that selects only the old project gets no task fields, because the
task is now outside its projects. Its `data` says that the task left:

```json
{
  "project_id": "0199f0a2-1c3d-7e4f-8a5b-222222222222",
  "moved_out": true,
  "task": {
    "id": "019a7c2d-0e1f-7a2b-8c3d-4e5f6a7b8c9d",
    "version": 1188834450093021
  },
  "changes": {
    "project": { "from": "0199f0a2-1c3d-7e4f-8a5b-222222222222", "to": null }
  }
}
```

Treat it as you would treat `task.deleted` for that project: the task stops
sending events to this webhook until it moves back.

### `task.deleted`

`data.task` holds only what identifies the task in the trash:

```json
{
  "project_id": "0199f0a2-1c3d-7e4f-8a5b-222222222222",
  "task": {
    "id": "019a7c2d-0e1f-7a2b-8c3d-4e5f6a7b8c9d",
    "project_id": "0199f0a2-1c3d-7e4f-8a5b-222222222222",
    "identifier": "ENG-12",
    "version": 1188834450093021,
    "deleted_at": "2026-11-10T10:02:51.877Z"
  }
}
```

A restore from the trash sends `task.updated` with
`"changes": {"deleted_at": true}`.

### Task version

`version` changes with every change of the task. Compare it for equality only:
it is not a counter, and a later version can be a smaller number. It is the
same value as `version` in the public REST API, so you can send it there as
`expected_version`.

## Verify the signature

The signed content is the event id, the timestamp and the raw request body,
joined by dots: `{webhook-id}.{webhook-timestamp}.{body}`. The key is the part
of the secret after `whsec_`, base64-decoded. The `webhook-signature` header
holds `v1,` and the base64 HMAC-SHA256 of the signed content. Standard
Webhooks allows several signatures separated by spaces. Orbit sends one, but
accept the request when any of them matches.

Check the signature against the raw body bytes, before you parse the JSON.
Refuse a request whose timestamp is more than 5 minutes from your clock, so a
captured request cannot be replayed later.

Node.js:

```js
import { createHmac, timingSafeEqual } from 'node:crypto'

const TOLERANCE_SECONDS = 5 * 60

export function verifyOrbitWebhook(secret, headers, rawBody) {
  const id = headers['webhook-id']
  const timestamp = headers['webhook-timestamp']
  const signatures = (headers['webhook-signature'] ?? '').split(' ')
  if (!id || !timestamp) return false
  if (Math.abs(Date.now() / 1000 - Number(timestamp)) > TOLERANCE_SECONDS) return false
  const key = Buffer.from(secret.replace(/^whsec_/, ''), 'base64')
  const expected = createHmac('sha256', key).update(`${id}.${timestamp}.${rawBody}`).digest()
  return signatures.some((entry) => {
    const [version, value] = entry.split(',')
    if (version !== 'v1' || !value) return false
    const actual = Buffer.from(value, 'base64')
    return actual.length === expected.length && timingSafeEqual(actual, expected)
  })
}
```

Python:

```python
import base64
import hashlib
import hmac
import time

TOLERANCE_SECONDS = 5 * 60


def verify_orbit_webhook(secret: str, headers: dict, raw_body: bytes) -> bool:
    event_id = headers.get("webhook-id")
    timestamp = headers.get("webhook-timestamp")
    if not event_id or not timestamp:
        return False
    if abs(time.time() - int(timestamp)) > TOLERANCE_SECONDS:
        return False
    key = base64.b64decode(secret.removeprefix("whsec_"))
    signed = f"{event_id}.{timestamp}.".encode() + raw_body
    expected = base64.b64encode(hmac.new(key, signed, hashlib.sha256).digest()).decode()
    for entry in headers.get("webhook-signature", "").split(" "):
        version, _, value = entry.partition(",")
        if version == "v1" and hmac.compare_digest(value, expected):
            return True
    return False
```

## Delivery guarantees

Orbit writes the delivery in the same database transaction as the task change.
A change that is saved always gets its events; a change that fails sends none.

Delivery is **at least once**. A lost answer or a server restart can send the
same event again. Store the `webhook-id` of each event that you processed, and
skip an event whose id you already have.

Events have **no order**. Retries and parallel sends can deliver a newer event
before an older one. Use `data.task.updated_at`, or read the task from the
public REST API, to find its current state.

Orbit tries each delivery up to 8 times. After a failed attempt it waits 10
seconds, 30 seconds, 2 minutes, 10 minutes, 1 hour, 6 hours and 24 hours before
the next one, so the last attempt is about 31 hours after the change. After the
last attempt, the delivery is `failed`. **Settings → Webhooks → Deliveries**
shows the last 50 deliveries with their state, attempts and last answer.
Orbit keeps finished deliveries for 30 days.

Before each attempt, Orbit checks the webhook again. It cancels the delivery
when the webhook is off, when the task's project left the webhook or went to
the trash, or when the workspace went to the trash.

## Avoid loops

When your endpoint writes back to Orbit, it receives an event for its own
write. To ignore those events, write with an API token of a service account,
and skip events whose `actor.service_account_id` is that account. With a
personal token, compare `actor.api_token_id` with the token's id.
