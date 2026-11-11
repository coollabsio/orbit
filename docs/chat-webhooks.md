# Chat incoming webhooks

An incoming webhook lets an external tool (CI, monitoring, ticketing, a bot) post a message into one
chat channel. The tool sends JSON to a secret URL. It needs no account and no API token.

Orbit's webhooks take the payload of Slack incoming webhooks. A tool that posts to Slack can post
to Orbit when you replace the URL. See [Slack compatibility](#slack-compatibility) for the few
differences.

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
  -d '{"text": "Deploy *finished* :rocket:"}' \
  'https://orbit.example.com/api/v1/hooks/chat/<webhook_id>/<secret>'
```

Response: `200` with the text body `ok`, as Slack answers. Send the body as JSON
(`Content-Type: application/json`), or as a form (`application/x-www-form-urlencoded`) with the
JSON in the `payload` field.

| Field | Type | Rules |
|---|---|---|
| `text` | string | The message, in Slack mrkdwn or Markdown. A longer text than 4000 characters is cut. Required without `blocks` and `attachments`; with them, it is the plain text for notifications, quotes and search, and Orbit makes it from them if you leave it out. |
| `mrkdwn` | boolean | `false` shows `text` as it is, with no formatting. Links and mentions still work. |
| `blocks` | array | Block Kit blocks that show in place of `text`. See [Blocks](#blocks). |
| `attachments` | array | Legacy attachments. See [Attachments](#attachments). |
| `username` | string | The author name of this message only. Cut to 80 characters. |
| `icon_url` | string | The avatar of this message only: an `https://` image. Other URLs are ignored. |
| `icon_emoji` | string | The avatar of this message only, as an emoji (`:ghost:`). It wins over `icon_url`. |

Orbit ignores other fields (`channel`, `thread_ts`, `unfurl_links`, …).

### Text formatting

Orbit converts Slack mrkdwn: `*bold*`, `_italic_`, `~strike~`, `` `code` ``, ```` ``` ```` code
blocks, `> quote`, `<https://example.com|label>`, `<!here>`, `<!channel>`, `<!everyone>` (as
`@channel`), and the escapes `&amp;`, `&lt;`, `&gt;`. Markdown (`**bold**`,
`[label](https://example.com)`) works too. Emoji names such as `:tada:` show as emoji.

Mentions of people and channels must use Orbit ids: `<@user_id>` and `<#channel_id>`. A Slack id
(`<@U024BE7LH|bob>`) names nobody in Orbit, so it shows as text: its label (`@bob`), or the id.

The message is unread for every member of the channel, also for the member who made the webhook.
It makes no Inbox item, and nobody can edit it. The channel's creator and owners and admins can
delete it.

## Blocks

`blocks` uses the shape of Slack's [Block Kit](https://docs.slack.dev/reference/block-kit/blocks):

| Block | Fields |
|---|---|
| `header` | `text` (`plain_text`, at most 150 characters) |
| `section` | `text` and/or `fields` (at most 10, shown in two columns), optional `accessory` button |
| `actions` | `elements`: 1–25 buttons |
| `context` | `elements`: 1–10 text (`plain_text`, `mrkdwn`) or `image` elements |
| `image` | `image_url` (https), `alt_text`, optional `title` |
| `divider` | none |
| `markdown` | `text` (Markdown, at most 4000 characters) |
| `rich_text` | `elements`: `rich_text_section`, `rich_text_list` (`bullet` or `ordered`, `indent` 0–8), `rich_text_quote`, `rich_text_preformatted` |

Rich text runs are `text` (with `style`: `bold`, `italic`, `strike`, `code`), `link`, `emoji`
(`name`), `user` (`user_id`), `channel` (`channel_id`), `broadcast` (`range`: `here`, `channel` or
`everyone`) and `date` (shown as its `fallback`).

A button has `text` (`plain_text`, at most 75 characters) and an optional `style` (`primary` or
`danger`). A button with a `url` (http or https) opens the link. A button without a `url` sends a
click to the integration (see [Interactivity](#interactivity)); it is disabled while the webhook has
no request URL. Give the block a `block_id` and each button an `action_id`; Orbit makes them
(`b0`, `a0`, …) when they are missing.

Orbit leaves out what it cannot show, as a Slack client does with what it does not know: other block
types (`input`, `video`, …), interactive elements that are not buttons (menus, date pickers,
`overflow`), accessories that are not buttons, other rich text runs (`usergroup`, …), and fields such as
`block_id`, `emoji` and `accessory`. A block of a known type that breaks its rules (for example a
`header` with `mrkdwn` text, or an `http://` image) is refused with `422 validation_failed`, as Slack
refuses it. The `detail` names the place, for example `blocks[0].text: must be plain_text`.

```bash
curl -X POST -H 'Content-Type: application/json' -d '{
  "text": "Deploy v2.4.0 finished",
  "blocks": [
    {"type": "header", "text": {"type": "plain_text", "text": "Deploy v2.4.0 finished :rocket:"}},
    {"type": "section", "fields": [
      {"type": "mrkdwn", "text": "*Service*\napi"},
      {"type": "mrkdwn", "text": "*Duration*\n3m 12s"}]},
    {"type": "context", "elements": [{"type": "mrkdwn", "text": "Pipeline *#812*"}]}
  ]
}' 'https://orbit.example.com/api/v1/hooks/chat/<webhook_id>/<secret>'
```

## Attachments

Legacy attachments show as Slack shows them: a box with a colored bar. Orbit uses `fallback`,
`color` (`good`, `warning`, `danger` or a hex color), `pretext`, `author_name`, `author_link`,
`author_icon`, `title`, `title_link`, `text`, `fields` (`title`, `value`, `short`), `image_url`,
`thumb_url`, `footer`, `footer_icon`, `ts` and `blocks`. Images that are not https and links that are
not http(s) are left out. Long texts are cut. Without `blocks`, `text` shows above the attachments.

```bash
curl -X POST -H 'Content-Type: application/json' -d '{
  "attachments": [{
    "fallback": "Build #812 passed",
    "color": "good",
    "title": "Build #812 passed",
    "title_link": "https://ci.example.com/812",
    "text": "Branch `main` by *bob*",
    "fields": [{"title": "Env", "value": "prod", "short": true}],
    "footer": "Jenkins",
    "ts": 1793900000
  }]
}' 'https://orbit.example.com/api/v1/hooks/chat/<webhook_id>/<secret>'
```

## Interactivity

A click on a button without a `url` is sent to the webhook's **request URL**, as Slack sends it, and
the integration answers through a `response_url`. Code that handles Slack interactivity (for
example Bolt) works with Orbit after you change its URLs and signing secret.

### Set it up

1. Open **⋯ > Edit channel > Integrations**, then select **⋯ > Interactivity** on the webhook.
2. Type the request URL (`https://…`) and select **Save**. Orbit makes a signing secret.
3. Select **Reveal** and copy the signing secret into your integration.

The request URL must be https on a public address. (A development server, `ORBIT_ENV=development`,
also takes `http` and local addresses.) **Make new secret** changes the secret at once. **Turn off**
disables the buttons again. **Last clicks** shows the answers of the last 20 clicks.

### The click

Orbit sends `POST <request URL>` with `Content-Type: application/x-www-form-urlencoded` and the
JSON in the `payload` field, within 3 seconds, one time, without redirects:

```json
{
  "type": "block_actions",
  "api_app_id": "<webhook id>",
  "team": {"id": "<workspace id>", "domain": "<workspace name>"},
  "user": {"id": "<user id>", "username": "Ana", "name": "Ana", "team_id": "<workspace id>"},
  "channel": {"id": "<channel id>", "name": "deploys"},
  "container": {"type": "message", "message_ts": "<message id>", "channel_id": "<channel id>", "is_ephemeral": false},
  "message": {"type": "message", "ts": "<message id>", "text": "…", "blocks": […]},
  "response_url": "https://orbit.example.com/api/v1/hooks/chat/<webhook id>/responses/<token>",
  "actions": [{"type": "button", "action_id": "approve", "block_id": "review", "value": "48",
               "text": {"type": "plain_text", "text": "Approve"}, "style": "primary", "action_ts": "1793917200.123"}]
}
```

Ids are Orbit ids: `message_ts` and `ts` are the message id, not a Slack timestamp. Orbit reads
`value` from the stored message, so a member cannot change it.

Each request is signed as Slack signs it, in `X-Slack-Request-Timestamp` and `X-Slack-Signature`
(also sent as `X-Orbit-Request-Timestamp` and `X-Orbit-Signature`): `v0=` and the hex HMAC-SHA256 of
`v0:{timestamp}:{raw body}` with the signing secret. Check it in constant time, and refuse requests
older than 5 minutes.

Answer `200` within 3 seconds; Orbit does not use the body. Any other answer, or none, shows the
member an error, and the click is not sent again.

### Answer through `response_url`

POST JSON to the `response_url` — a message as for the webhook (`text`, `blocks`, `attachments`,
`username`, `icon_url`, `icon_emoji`), and what to do with it:

| Field | Effect |
|---|---|
| `replace_original: true` | Replaces the message with the button (not marked as edited). |
| `delete_original: true` | Deletes the message with the button. |
| `response_type: "in_channel"` | Posts a new message in the channel. |
| `response_type: "ephemeral"` (default) | Shows a reply only to the member who clicked, until they reload or dismiss it. |

A `response_url` works 5 times in 30 minutes. Orbit answers `200 ok`, or `404 response_url_not_found`.

```python
# A minimal receiver (Python standard library).
import hashlib, hmac, json, time, urllib.parse, urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer

SECRET = b"<signing secret>"

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        raw = self.rfile.read(int(self.headers["Content-Length"]))
        ts = self.headers["X-Slack-Request-Timestamp"]
        expected = "v0=" + hmac.new(SECRET, b"v0:" + ts.encode() + b":" + raw, hashlib.sha256).hexdigest()
        if abs(time.time() - int(ts)) > 300 or not hmac.compare_digest(expected, self.headers["X-Slack-Signature"]):
            self.send_response(401); self.end_headers(); return
        self.send_response(200); self.end_headers()
        payload = json.loads(urllib.parse.parse_qs(raw.decode())["payload"][0])
        answer = {"replace_original": True,
                  "text": f"{payload['actions'][0]['text']['text']} by <@{payload['user']['id']}>"}
        urllib.request.urlopen(urllib.request.Request(payload["response_url"], json.dumps(answer).encode(),
                                                      {"Content-Type": "application/json"}))

HTTPServer(("0.0.0.0", 3000), Handler).serve_forever()
```

## Slack compatibility

What does not carry over from Slack:

- Only buttons are interactive. Menus, date pickers and other interactive elements are left out,
  and there are no modals (`trigger_id` is sent but cannot open anything).
- Slack user, user group and channel ids name nobody in Orbit. Use Orbit ids.
- `thread_ts` (replies in a thread) and `channel` (another channel) are ignored: a webhook posts to
  its own channel.
- Errors are JSON (`application/problem+json`) with an Orbit `code`, not Slack's plain-text codes
  (`invalid_payload`, `no_text`, …). The status codes are close to Slack's.

## Limits and errors

- Body: at most 64 KiB.
- Blocks: at most 50 in `blocks`, 100 with the attachments, and 48 KiB in total.
- Rate: 60 accepted messages a minute for each webhook, and the general API limit for each client
  address. Requests that fail with 404 do not count against the webhook.

Errors are `application/problem+json`. The `instance` field never contains the secret.

| Status | `code` | Cause |
|---|---|---|
| 400 | `invalid_json` | The body (or its `payload` field) is not a JSON object. |
| 402 | `workspace_read_only` | The workspace has no active subscription (Orbit Cloud). |
| 403 | `chat_channel_archived` | The channel is archived. |
| 404 | `webhook_not_found` | Unknown id, wrong secret, or a deleted or rotated webhook. |
| 404 | `response_url_not_found` | A `response_url` used 5 times, older than 30 minutes, or whose message is gone. |
| 413 | `payload_too_large` | The body is larger than 64 KiB. |
| 415 | `unsupported_media_type` | The body is not JSON or a form. |
| 422 | `validation_failed` | Nothing to show (no text, and no blocks or attachments that Orbit can show), or a block or attachment breaks its rules. The `detail` names the place. |
| 429 | `rate_limit_exceeded` | Too many messages. Wait as `Retry-After` tells. |
