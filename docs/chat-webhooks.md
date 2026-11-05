# Chat incoming webhooks

An incoming webhook lets an external tool (CI, monitoring, ticketing, a bot) post a message into one
chat channel. The tool sends JSON to a secret URL. It needs no account and no API token.

## Make a webhook

1. Open the channel, then select **⋯ > Edit channel > Integrations**.
2. Select **New webhook**. Type a name (for example `CI`). An icon URL (https) is optional.
3. Copy the URL. Orbit shows it only once, because it stores only a hash of the secret.

The channel's creator and the workspace's owners and admins can make, edit, rotate and delete
webhooks. A channel can have at most 10. A webhook keeps working when the member who made it leaves
the workspace.

**Make new URL** (rotate) gives the same webhook a new secret. The old URL stops working at once.
**Delete** stops the URL for good. The messages that the webhook posted stay.

## Post a message

```bash
curl -X POST -H 'Content-Type: application/json' \
  -d '{"text": "Deploy **finished** :rocket:"}' \
  'https://orbit.example.com/api/v1/hooks/chat/<webhook_id>/<secret>'
```

Response: `200 {"ok": true, "message_id": "…"}`.

| Field | Type | Rules |
|---|---|---|
| `text` | string | Required. Markdown, 1–4000 characters. |
| `username` | string | Optional. The author name of this message only, 1–80 characters. |
| `icon_url` | string | Optional. The avatar of this message only. An `https://` image. |

Orbit ignores other fields, so a Slack-style payload (`channel`, `icon_emoji`, `attachments`,
`blocks`) still posts its `text`.

Mentions use tokens: `<@user_id>`, `<#channel_id>`, `<!channel>` and `<!here>`. The message is unread
for every member of the channel, also for the member who made the webhook. It makes no Inbox item,
and nobody can edit it. The channel's creator and owners and admins can delete it.

## Limits and errors

- Body: at most 64 KiB of JSON, with `Content-Type: application/json`.
- Rate: 60 accepted messages a minute for each webhook, and the general API limit for each client
  address. Requests that fail with 404 do not count against the webhook.

Errors are `application/problem+json`. The `instance` field never contains the secret.

| Status | `code` | Cause |
|---|---|---|
| 400 | `invalid_json` | The body is not a JSON object. |
| 402 | `workspace_read_only` | The workspace has no active subscription (Orbit Cloud). |
| 403 | `chat_channel_archived` | The channel is archived. |
| 404 | `webhook_not_found` | Unknown id, wrong secret, or a deleted or rotated webhook. |
| 413 | `payload_too_large` | The body is larger than 64 KiB. |
| 415 | `unsupported_media_type` | The `Content-Type` is not `application/json`. |
| 422 | `validation_failed` | `text` is missing or empty, or `username` or `icon_url` is not valid. The `detail` names the field. |
| 422 | `chat_message_too_long` | `text` has more than 4000 characters. |
| 429 | `rate_limit_exceeded` | Too many messages. Wait as `Retry-After` tells. |
