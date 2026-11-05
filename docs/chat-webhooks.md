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
`danger`). A button with a `url` (http or https) opens the link. A button without a `url` sends its
`action_id` and `value` to the integration in Slack; Orbit does not support that yet, so it shows
the button disabled.

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

## Slack compatibility

What does not carry over from Slack:

- Buttons without a `url` show disabled: Orbit does not send interaction payloads yet. Menus,
  date pickers and other interactive elements are left out.
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
| 413 | `payload_too_large` | The body is larger than 64 KiB. |
| 415 | `unsupported_media_type` | The body is not JSON or a form. |
| 422 | `validation_failed` | Nothing to show (no text, and no blocks or attachments that Orbit can show), or a block or attachment breaks its rules. The `detail` names the place. |
| 429 | `rate_limit_exceeded` | Too many messages. Wait as `Retry-After` tells. |
