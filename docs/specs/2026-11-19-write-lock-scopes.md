# Write-lock scopes for write transactions

Date: 2026-11-19. Status: classification only. No code changes yet.

## What this is

On Postgres, every `Database::immediate_transaction()` and `CollabHub::write_transaction` takes one global
advisory lock (`POSTGRES_WRITE_LOCK`, `crates/platform/src/db/mod.rs:57`). All writers of the cluster run one at a
time. `Database::workspace_transaction(workspace_id)` will take the global lock in shared mode and an exclusive lock
for one workspace. Transactions of different workspaces then run in parallel. Transactions of the same workspace
still run one at a time. `immediate_transaction()` keeps the global lock in exclusive mode, so it still excludes
every other writer. SQLite does not change: every writer keeps `BEGIN IMMEDIATE`.

This document puts each write transaction in one of three classes:

- **W (workspace-safe).** The transaction reads and then writes only rows of one workspace. The workspace id is
  known before the transaction starts. A global or cross-workspace row that it touches is protected by a unique
  constraint, by `ON CONFLICT`, or by one atomic statement. Insert-only rows (audit, notifications, job enqueue,
  live events) are safe.
- **G (needs global).** The transaction reads and then writes rows that several workspaces share, touches several
  workspaces, or finds its workspace only inside the transaction. Keep `immediate_transaction()`.
- **A (atomic).** The transaction is one atomic statement, or independent atomic statements, guarded by
  constraints. It is correct with no serialization at all. A later step can move these to a plain
  `transaction()`. Until then, `workspace_transaction` is also correct for an A site that has a workspace id.

**The rule.** Move a site to `workspace_transaction` only when it is W. A W site and a G site still exclude each
other, because G takes the global lock in exclusive mode. Thus a mixed state is always safe: you can move one
site at a time, and any site that stays on `immediate_transaction()` stays correct. The only new risk is between
two W transactions of different workspaces. Each W row below names why that pair cannot break an invariant.

**How the list was made.** The site list is `rg -n "immediate_transaction\(\)|write_transaction\(\)" apps crates
--glob '!**/tests/**'` on commit `1454f60c`. It gives 340 hits. Three hits are doc comments, not calls
(`apps/server/src/mail_intake.rs:7`, `apps/server/src/repositories/mail/ingest.rs:6`,
`apps/server/src/repositories/mail/keys.rs:25`). One hit is test code (`apps/server/src/app.rs:1685`). This leaves
336 production call sites.

## Shared helpers checked

These helpers run inside many W candidates. Each one is per workspace:

- `ensure_storage_quota` (`crates/platform/src/files/upload.rs:476`) reads the quota from `instance_settings` and
  only reads it. It sums `attachment_blobs` of one workspace. The invariant is per workspace. `save_limits` writes
  the quota under the global exclusive lock, so it never runs during a W transaction.
- `ensure_trial_capacity` (`apps/server/src/repositories/billing.rs:345`) reads the subscription and the usage of
  one workspace.
- `live::events::append` (`apps/server/src/live/events.rs:37`) numbers events with `MAX(seq)+1` per workspace.
- Task numbers come from a trigger that runs one atomic `UPDATE projects SET task_counter = task_counter + 1` per
  project. The unique index `tasks_project_number` backs it.
- Positions (`MAX(position)+1`, renumbering) are per project, status, database, parent page, category or
  workspace. A workspace lock covers them.
- `notifications` and `oauth_grants` have a `workspace_id` column. Notifications are insert-only or use
  `ON CONFLICT`.
- `attachment_blobs` are unique per `(workspace_id, sha256, byte_size)`. Profile-picture blobs use the user id in
  the `workspace_id` column (see the unsure list).
- Collab updates (`page_collab_updates`) use a global identity `seq`. Every reader filters by `page_id AND seq > $n`
  (`apps/server/src/collab/hub.rs:464`, `:780`). Every writer to the log of one page holds that page's workspace
  lock. Per-page order thus matches commit order.

## Chat

| Site | Function | Class | Workspace id source | Reason |
|---|---|---|---|---|
| `apps/server/src/repositories/chat/conversations.rs:158` | create_channel | W | `workspace_id` | Name check, position and members are per workspace. |
| `apps/server/src/repositories/chat/conversations.rs:208` | update_channel | W | `workspace_id` | Name check, position and member backfill per workspace. |
| `apps/server/src/repositories/chat/conversations.rs:298` | archive_channel | W | `workspace_id` | One UPDATE after an access check. |
| `apps/server/src/repositories/chat/conversations.rs:326` | join_channel | W | `workspace_id` | Membership check, then member and system row. |
| `apps/server/src/repositories/chat/conversations.rs:364` | leave_channel | W | `workspace_id` | Deletes members of one conversation. |
| `apps/server/src/repositories/chat/conversations.rs:388` | add_channel_members | W | `workspace_id` | Reads members, inserts the difference. |
| `apps/server/src/repositories/chat/conversations.rs:439` | remove_channel_member | W | `workspace_id` | Same shape as leave_channel. |
| `apps/server/src/repositories/chat/conversations.rs:475` | open_dm | W | `workspace_id` | Select by `(workspace_id, dm_key)`, then insert. Reads `user_blocks` only. |
| `apps/server/src/repositories/chat/conversations.rs:540` | create_category | W | `workspace_id` | `MAX(position)+1` per workspace. |
| `apps/server/src/repositories/chat/conversations.rs:568` | rename_category | W | `workspace_id` | One UPDATE in the workspace. |
| `apps/server/src/repositories/chat/conversations.rs:590` | delete_category | W | `workspace_id` | Moves channels, deletes the category. |
| `apps/server/src/repositories/chat/conversations.rs:644` | place_category | W | `workspace_id` | Renumbers categories of the workspace. |
| `apps/server/src/repositories/chat/conversations.rs:689` | place_channel | W | `workspace_id` | Renumbers sibling channels. |
| `apps/server/src/repositories/chat/messages.rs:313` | send_message | W | `workspace_id` | Nonce dedupe, counters, mentions, notifications, mark-read: all per conversation. |
| `apps/server/src/repositories/chat/messages.rs:465` | post_as_bot | W | `target.workspace_id` (webhook lookup before the tx) | Same as send_message, plus `last_used_at` of this webhook. |
| `apps/server/src/repositories/chat/messages.rs:614` | forward_message | W | `workspace_id` | The source must be in the same workspace. |
| `apps/server/src/repositories/chat/messages.rs:728` | edit_message | W | `workspace_id` | Recounts counters of one conversation. |
| `apps/server/src/repositories/chat/messages.rs:763` | delete_message | W | `workspace_id` | `remove_message`, one conversation. |
| `apps/server/src/repositories/chat/messages.rs:799` | set_reaction | W | `workspace_id` | Reaction cap per message, insert `ON CONFLICT`. |
| `apps/server/src/repositories/chat/messages.rs:859` | set_pinned | W | `workspace_id` | Reads pin state, then UPDATE and system row. |
| `apps/server/src/repositories/chat/state.rs:94` | mark_read | W | `workspace_id` | One member row and notifications of one conversation. |
| `apps/server/src/repositories/chat/state.rs:135` | mark_unread | W | `workspace_id` | Cursor and recount, one conversation. |
| `apps/server/src/repositories/chat/state.rs:192` | mark_thread_read | W | `workspace_id` | Thread member upsert `ON CONFLICT`. |
| `apps/server/src/repositories/chat/state.rs:223` | set_thread_follow | W | `workspace_id` | Reads, then upserts the thread member. |
| `apps/server/src/repositories/chat/state.rs:253` | mark_all_read | W | `workspace_id` | Loops over the actor's conversations, filtered by workspace. |
| `apps/server/src/repositories/chat/state.rs:377` | restore_read | W | `workspace_id` | Every cursor is limited to the workspace. |
| `apps/server/src/repositories/chat/state.rs:435` | update_state | W | `workspace_id` | Reads, then updates one member row. |
| `apps/server/src/repositories/chat/files.rs:158` | finalize_file | W | `workspace_id` (= `upload.workspace_id`) | Upload state and blob, both per workspace. |
| `apps/server/src/repositories/chat/interactions.rs:60` | prepare_click | W | `call.workspace_id` | Token insert by unique hash. See unsure list: it also deletes expired tokens of all workspaces. |
| `apps/server/src/repositories/chat/interactions.rs:180` | use_response_token | G | - | Workspace found inside the tx. Read-then-decrement of `uses_left`; a race gives a CHECK error. See unsure list. |
| `apps/server/src/repositories/chat/interactions.rs:255` | replace_bot_message | W | `response.target.workspace_id` | Loads the conversation in that workspace, then rewrites. |
| `apps/server/src/repositories/chat/interactions.rs:277` | delete_bot_message | W | `response.target.workspace_id` | `remove_message`. |
| `apps/server/src/repositories/chat/webhooks.rs:270` | create_webhook | W | `workspace_id` | Webhook cap per channel. |
| `apps/server/src/repositories/chat/webhooks.rs:316` | update_webhook | W | `workspace_id` | One webhook row. |
| `apps/server/src/repositories/chat/webhooks.rs:341` | rotate_webhook | W | `workspace_id` | One webhook row. |
| `apps/server/src/repositories/chat/webhooks.rs:377` | set_request_url | W | `workspace_id` | One webhook row. |
| `apps/server/src/repositories/chat/webhooks.rs:415` | signing_secret | W | `workspace_id` | Reads, then maybe rotates one row. |
| `apps/server/src/repositories/chat/webhooks.rs:486` | record_delivery | A | - | Insert, then trim to the newest 20 of one webhook. Callers have `target.workspace_id` for W. |
| `apps/server/src/repositories/chat/webhooks.rs:522` | revoke_webhook | W | `workspace_id` | One UPDATE. |
| `apps/server/src/repositories/chat/emoji.rs:154` | create_emoji | W | `workspace_id` | Emoji cap and name per workspace. |
| `apps/server/src/repositories/chat/emoji.rs:198` | delete_emoji | W | `workspace_id` | One DELETE. |
| `apps/server/src/repositories/chat/stickers.rs:187` | create_sticker | W | `workspace_id` | Sticker cap and name per workspace. |
| `apps/server/src/repositories/chat/stickers.rs:231` | delete_sticker | W | `workspace_id` | One DELETE. |
| `apps/server/src/repositories/chat/safety.rs:129` | report_message | W | `workspace_id` | One report per (message, reporter); notifications `ON CONFLICT`. |
| `apps/server/src/repositories/chat/safety.rs:222` | delete_reported_message | W | `workspace_id` | `remove_message` in one conversation. |
| `apps/server/src/repositories/chat/safety.rs:292` | block_user | A | - | `user_blocks` is user-level. One `INSERT ON CONFLICT DO NOTHING`. Keep on the global lock until the unsure item is settled. |

## Docs, pages and collab

| Site | Function | Class | Workspace id source | Reason |
|---|---|---|---|---|
| `apps/server/src/repositories/pages.rs:766` | insert_page | W | `workspace_id` | Trial capacity, sibling renumber, schema, mentions, audit. Reads the global collab generation only. |
| `apps/server/src/repositories/pages.rs:907` | order_children | W | `workspace_id` | Renumbers siblings. |
| `apps/server/src/repositories/pages.rs:1094` | write_page | W | `workspace_id` | Version-guarded UPDATE, per-page versions, per-page collab log. |
| `apps/server/src/repositories/pages.rs:1355` | move_page_expecting | W | `workspace_id` | Siblings, descendants and row values in the workspace. |
| `apps/server/src/repositories/pages.rs:1551` | delete_page_expecting | W | `workspace_id` | Trashes a subtree of the workspace. |
| `apps/server/src/repositories/pages.rs:1647` | unlink_trashed | W | `workspace_id` | One tx per linking page of the workspace. |
| `apps/server/src/repositories/pages.rs:1676` | restore_page | W | `workspace_id` | Trial capacity, then siblings and restore. |
| `apps/server/src/repositories/pages.rs:1992` | purge_trashed_root | W | `workspace_id` | Deletes pages in batches; cascades are per page. |
| `apps/server/src/repositories/pages.rs:2059` | duplicate_page | W | `workspace_id` | Copies pages and files of the same workspace; trial capacity. |
| `apps/server/src/repositories/pages.rs:2273` | add_favorite | W | `workspace_id` | Favorites of (user, this workspace). |
| `apps/server/src/repositories/pages.rs:2310` | remove_favorite | W | `workspace_id` | Same. |
| `apps/server/src/repositories/pages.rs:2339` | move_favorite | W | `workspace_id` | Same. |
| `apps/server/src/repositories/pages.rs:2392` | set_lock | W | `workspace_id` | Reads lock state, then UPDATE and audit. |
| `apps/server/src/repositories/pages.rs:2452` | record_visit | A | - | Upsert `ON CONFLICT (user_id, page_id)` and a trim. W with `workspace_id` is also fine. |
| `apps/server/src/repositories/page_comments.rs:329` | create_thread | W | `workspace_id` | Thread, comment, mentions, audit. |
| `apps/server/src/repositories/page_comments.rs:374` | add_comment | W | `workspace_id` | Comment, `touch_thread`, notifications. |
| `apps/server/src/repositories/page_comments.rs:408` | update_comment | W | `workspace_id` | Mentions of one comment. |
| `apps/server/src/repositories/page_comments.rs:456` | delete_comment | W | `workspace_id` | Counts comments of one thread, then deletes. |
| `apps/server/src/repositories/page_comments.rs:517` | delete_thread | W | `workspace_id` | Role check and count per thread. |
| `apps/server/src/repositories/page_comments.rs:570` | set_resolved | W | `workspace_id` | Conditional UPDATE and audit. |
| `apps/server/src/repositories/page_versions.rs:244` | restore_page_version | W | `workspace_id` | Per-page versions and collab log. |
| `apps/server/src/repositories/page_files.rs:126` | finalize | W | `workspace_id` (= `upload.workspace_id`) | Upload state, blob, file row, audit. |
| `apps/server/src/collab/hub.rs:826` | initialize | W | `room.workspace_id` (set in `load()` before the tx) | Insert `page_collab_docs ON CONFLICT (page_id)`. |
| `apps/server/src/collab/hub.rs:881` | rebuild | W | `room.workspace_id` | Overwrites the snapshot of one page, deletes its log. |
| `apps/server/src/collab/hub.rs:967` | persist | W | `room.workspace_id` | Per-page log and snapshot; projection writes that page or task only. A page never changes workspace. |

## Tasks

| Site | Function | Class | Workspace id source | Reason |
|---|---|---|---|---|
| `apps/server/src/repositories/tasks.rs:769` | reconcile_github_attribution | W | loop `workspace_id` (read before each tx) | One workspace per tx. Runs once at startup. |
| `apps/server/src/repositories/tasks.rs:976` | create_project_with_id | W | `workspace_id` | Key unique per `(workspace_id, project_key)`. |
| `apps/server/src/repositories/tasks.rs:1053` | update_project | W | `workspace_id` | Version check; key constraint. |
| `apps/server/src/repositories/tasks.rs:1137` | delete_project | W | `workspace_id` | Version-checked tombstone. |
| `apps/server/src/repositories/tasks.rs:1183` | restore_project | W | `workspace_id` | Key conflict check in the workspace. |
| `apps/server/src/repositories/tasks.rs:1373` | empty_trash_batches | W | `workspace_id` | Blobs per workspace; deletion queue `ON CONFLICT`. |
| `apps/server/src/repositories/tasks.rs:1450` | purge_trashed_project | W | `workspace_id` | Token rows changed by project id in single statements. |
| `apps/server/src/repositories/tasks.rs:1569` | create_status | W | `workspace_id` | `MAX(position)+1` per project. |
| `apps/server/src/repositories/tasks.rs:1615` | update_status | W | `workspace_id` | Version check; bulk update of that status's tasks. |
| `apps/server/src/repositories/tasks.rs:1695` | delete_status | W | `workspace_id` | Counts tasks, then deletes. |
| `apps/server/src/repositories/tasks.rs:1737` | reorder_statuses | W | `workspace_id` | Positions per project. |
| `apps/server/src/repositories/tasks.rs:1840` | create_label | W | `workspace_id` | `UNIQUE(project_id, name)`. |
| `apps/server/src/repositories/tasks.rs:1882` | update_label | W | `workspace_id` | Version check; name constraint. |
| `apps/server/src/repositories/tasks.rs:1922` | delete_label | W | `workspace_id` | Version-checked delete. |
| `apps/server/src/repositories/tasks.rs:2170` | insert_task | W | `workspace_id` | Trial capacity, positions, atomic per-project counter, notifications. |
| `apps/server/src/repositories/tasks.rs:2328` | sync_github_work_item | W | `workspace_id` | Service account per workspace (unique index); link key includes `workspace_id`. |
| `apps/server/src/repositories/tasks.rs:2594` | create_discord_task | W | `workspace_id` | Idempotency in `integration_events` per workspace. |
| `apps/server/src/repositories/tasks.rs:2730` | update_task | W | `workspace_id` | Task, relations, assignees, labels, sub-issue automation; project move uses the atomic counter. |
| `apps/server/src/repositories/tasks.rs:2936` | update_many_tasks | W | `workspace_id` | Same as update_task, several tasks of one workspace. |
| `apps/server/src/repositories/tasks.rs:3063` | delete_task | W | `workspace_id` | Trash of task and descendants. |
| `apps/server/src/repositories/tasks.rs:3100` | purge_task | W | `workspace_id` | Blob reference counts per workspace. |
| `apps/server/src/repositories/tasks.rs:3215` | restore_task | W | `workspace_id` | Trial capacity, then restore. |
| `apps/server/src/repositories/tasks.rs:3657` | import_comment | W | `workspace_id` | Comment insert and notifications. |
| `apps/server/src/repositories/tasks.rs:3774` | update_comment | W | `workspace_id` | Version check; mentions. |
| `apps/server/src/repositories/tasks.rs:3840` | delete_comment | W | `workspace_id` | Version-checked delete. |
| `apps/server/src/repositories/tasks.rs:3944` | mark_notification_read | A | - | One `UPDATE … COALESCE(read_at)` on one row. |
| `apps/server/src/repositories/tasks/cycles.rs:118` | create_cycle | W | `workspace_id` | Overlap check, `MAX(number)+1` per project. |
| `apps/server/src/repositories/tasks/cycles.rs:171` | find_or_create_cycle | W | `workspace_id` | Same. |
| `apps/server/src/repositories/tasks/cycles.rs:239` | update_cycle | W | `workspace_id` | Version and overlap check. |
| `apps/server/src/repositories/tasks/cycles.rs:289` | delete_cycle | W | `workspace_id` | Version-checked delete. |
| `apps/server/src/repositories/tasks/cycles.rs:327` | move_unfinished_tasks | W | `workspace_id` | Bulk task update; carryover `ON CONFLICT`. |
| `apps/server/src/repositories/tasks/milestones.rs:98` | create_milestone | W | `workspace_id` | Position per project; name unique. |
| `apps/server/src/repositories/tasks/milestones.rs:148` | update_milestone | W | `workspace_id` | Version check. |
| `apps/server/src/repositories/tasks/milestones.rs:203` | delete_milestone | W | `workspace_id` | Version-checked delete. |
| `apps/server/src/repositories/tasks/milestones.rs:241` | reorder_milestones | W | `workspace_id` | Positions per project. |
| `apps/server/src/repositories/task_relations.rs:615` | add_task_relation | W | `workspace_id` | Pair lookup; unique pair indexes. |
| `apps/server/src/repositories/task_relations.rs:727` | remove_task_relation | W | `workspace_id` | Delete, or unmark through `update_task_in_tx`. |
| `apps/server/src/repositories/views.rs:182` | create_view | A | - | Plain insert and audit. |
| `apps/server/src/repositories/views.rs:232` | update_view | W | `workspace_id` | Version check, then update. |
| `apps/server/src/repositories/views.rs:299` | delete_view | W | `workspace_id` | Permission check, then delete. |
| `apps/server/src/repositories/views.rs:337` | set_favorite | W | `workspace_id` | Favorite position over this workspace's views. |
| `apps/server/src/repositories/views.rs:397` | reorder_favorites | W | `workspace_id` | Same. |
| `apps/server/src/repositories/views.rs:491` | set_project_order | W | `workspace_id` | Positions of (workspace, user). |
| `apps/server/src/repositories/views.rs:567` | put_preference | A | - | One upsert `ON CONFLICT (workspace_id, user_id, page_key)`. |
| `apps/server/src/repositories/project_members.rs:110` | add_project_member | W | `workspace_id` | Insert `ON CONFLICT`. |
| `apps/server/src/repositories/project_members.rs:160` | remove_project_member | W | `workspace_id` | One delete and audit. |
| `apps/server/src/repositories/teamspaces.rs:137` | create_teamspace | W | `workspace_id` | `MAX(position)+1` per workspace. |
| `apps/server/src/repositories/teamspaces.rs:197` | update_teamspace | W | `workspace_id` | Unique index `teamspaces_project`. |
| `apps/server/src/repositories/teamspaces.rs:271` | move_teamspace | W | `workspace_id` | Renumbers teamspaces of the workspace. |
| `apps/server/src/repositories/teamspaces.rs:365` | delete_teamspace_unchecked | W | `workspace_id` | Counts, then deletes in the workspace. |

## Databases

| Site | Function | Class | Workspace id source | Reason |
|---|---|---|---|---|
| `apps/server/src/repositories/databases/rows.rs:60` | create_row_with_id | W | `workspace_id` | `open` rejects other workspaces; counter per property. |
| `apps/server/src/repositories/databases/rows.rs:103` | insert_row | W | `workspace_id` | Row cap per database, trial capacity, positions, unique_id per property. |
| `apps/server/src/repositories/databases/rows.rs:259` | update_values | W | `workspace_id` | Values, relations, dependents in the workspace. |
| `apps/server/src/repositories/databases/rows.rs:328` | trash_rows | W | `workspace_id` | Subtree UPDATE in the workspace. |
| `apps/server/src/repositories/databases/rows.rs:395` | move_row | W | `workspace_id` | Renumbers rows of one database. |
| `apps/server/src/repositories/databases/views.rs:25` | create_view | W | `workspace_id` | View cap and position per database. |
| `apps/server/src/repositories/databases/views.rs:93` | update_view | W | `workspace_id` | Renumbers views of one database. |
| `apps/server/src/repositories/databases/views.rs:156` | delete_view | W | `workspace_id` | Last-view check per database. |
| `apps/server/src/repositories/databases/properties.rs:44` | create_property | W | `workspace_id` | Property cap per database; relation target in the same workspace. |
| `apps/server/src/repositories/databases/properties.rs:201` | update_property | W | `workspace_id` | Counter per property. |
| `apps/server/src/repositories/databases/properties.rs:376` | delete_property | W | `workspace_id` | Relations, positions, views in the workspace. |
| `apps/server/src/repositories/databases/templates.rs:801` | create_template | W | `workspace_id` | Template cap per database; trial capacity. |
| `apps/server/src/repositories/databases/templates.rs:940` | update_template | W | `workspace_id` | Renumbers per database. |
| `apps/server/src/repositories/databases/templates.rs:1004` | delete_template | W | `workspace_id` | Deletes a subtree of this database. |
| `apps/server/src/repositories/databases/links.rs:59` | create_link | W | `workspace_id` | Link, view, audit. |
| `apps/server/src/repositories/databases/links.rs:142` | delete_link | W | `workspace_id` | Delete by id, audit. |
| `apps/server/src/repositories/databases/forms.rs:187` | submit_form | W | `workspace_id` | Hourly cap counts audit rows per (workspace, actor). |
| `apps/server/src/repositories/databases/buttons.rs:272` | press_button | W | `workspace_id` | Target database in the same workspace. |
| `apps/server/src/repositories/databases/import.rs:634` | import_csv | W | `workspace_id` | Row cap, trial capacity, counter per property. |
| `apps/server/src/repositories/databases/relation_import.rs:63` | import_relation | W | `workspace_id` (Notion import job) | Property id PK; links `ON CONFLICT`. |
| `apps/server/src/repositories/databases/relation_import.rs:158` | import_rollup | W | `workspace_id` (Notion import job) | Same. |
| `apps/server/src/repositories/databases/search.rs:210` | fill_search_text | G | - | Picks any row of any workspace inside the tx. Could become W (see unsure list). |

## Mail

| Site | Function | Class | Workspace id source | Reason |
|---|---|---|---|---|
| `apps/server/src/mail_intake.rs:169` | accept | A | - | Mail keys: SELECT, then `INSERT … ON CONFLICT (workspace_id, version)`, then SELECT. |
| `apps/server/src/mail_intake.rs:189` | accept | G | - | One tx writes receipts and jobs for every recipient workspace. |
| `apps/server/src/repositories/mail/ingest.rs:245` | ingest | W | `receipt.workspace_id` (loaded before the tx; a trigger keeps it fixed) | Message unique per receipt; thread message unique per mailbox. |
| `apps/server/src/repositories/mail/ingest.rs:380` | retry_receipt | W | `workspace_id` | Receipt and jobs of the workspace; enqueue only. |
| `apps/server/src/repositories/mail/ingest.rs:425` | purge | G | - | Daily cleanup over all workspaces. |
| `apps/server/src/repositories/mail/reading.rs:675` | update_threads | W | `workspace_id` | Threads, user state upserts, labels of the workspace. |
| `apps/server/src/repositories/mail/content.rs:146` | set_sender_pref | A | - | One upsert `ON CONFLICT (membership_id, address)`. |
| `apps/server/src/repositories/mail/mailboxes.rs:809` | create_mailbox | W | `workspace_id` | Address `UNIQUE(domain_id, local_part)`; domain must be in the workspace. |
| `apps/server/src/repositories/mail/mailboxes.rs:900` | update_mailbox | W | `workspace_id` | One mailbox. |
| `apps/server/src/repositories/mail/mailboxes.rs:939` | delete_mailbox | W | `workspace_id` | One mailbox; cascades stay inside. |
| `apps/server/src/repositories/mail/mailboxes.rs:976` | set_mailbox_status | W | `workspace_id` | One mailbox. |
| `apps/server/src/repositories/mail/mailboxes.rs:1023` | add_member | W | `workspace_id` | Insert `ON CONFLICT`. |
| `apps/server/src/repositories/mail/mailboxes.rs:1073` | remove_member | W | `workspace_id` | One member row. |
| `apps/server/src/repositories/mail/mailboxes.rs:1114` | add_alias | W | `workspace_id` | Address unique constraint. |
| `apps/server/src/repositories/mail/mailboxes.rs:1161` | remove_address | W | `workspace_id` | One alias. |
| `apps/server/src/repositories/mail/mailboxes.rs:1231` | bulk_personal | W | `workspace_id` | Same as create_mailbox for members of one workspace. |
| `apps/server/src/repositories/mail/labels.rs:138` | create_label | W | `workspace_id` | `UNIQUE(workspace_id, lower(name))`. |
| `apps/server/src/repositories/mail/labels.rs:183` | update_label | W | `workspace_id` | Same. |
| `apps/server/src/repositories/mail/labels.rs:232` | delete_label | W | `workspace_id` | One label. |
| `apps/server/src/repositories/mail/providers.rs:517` | create_provider | W or G | `ProviderScope::Workspace(id)` | Workspace scope is W. Instance scope is G (instance-wide provider). |
| `apps/server/src/repositories/mail/providers.rs:571` | update_provider | W or G | `ProviderScope::Workspace(id)` | Same split. |
| `apps/server/src/repositories/mail/providers.rs:645` | delete_provider | W or G | `ProviderScope::Workspace(id)` | Instance scope counts domains of all workspaces. |
| `apps/server/src/repositories/mail/providers.rs:814` | set_domain_route | W | `workspace_id` | Updates its own domain; FK guards the provider. |
| `apps/server/src/repositories/mail/domains.rs:482` | create_domain | G | - | Parent and child name claim across workspaces. No constraint covers subdomains. |
| `apps/server/src/repositories/mail/domains.rs:579` | delete_domain | W | `workspace_id` | Address count of its own domain. |
| `apps/server/src/repositories/mail/domains.rs:614` | set_catch_all | W | `workspace_id` | Mailbox and domain in the workspace. |
| `apps/server/src/repositories/mail/domains.rs:666` | set_domain_enabled | W | `workspace_id` | Own domain. |
| `apps/server/src/repositories/mail/domains.rs:713` | request_dns_check | W | `workspace_id` | Job dedupe by domain id; a duplicate only adds a check. |
| `apps/server/src/repositories/mail/domains.rs:761` | record_dns_report | G | - | Workspace found inside the tx; `move_domain` can change it. |
| `apps/server/src/repositories/mail/domains.rs:869` | move_domain | G | - | Touches two workspaces; cross-workspace name claim. |
| `apps/server/src/repositories/mail/health.rs:93` | create_health_test | G | - | Instance admin action; deletes expired tests of all workspaces. |

## Accounts and auth

Only two sites here are per workspace. Login, sessions, passwords, 2FA and instance settings stay global.

| Site | Function | Class | Workspace id source | Reason |
|---|---|---|---|---|
| `apps/server/src/auth_routes/two_factor.rs:158` | insert_for | G | - | Install-wide `MAX_PENDING` cap: count, then insert. |
| `apps/server/src/auth_routes/two_factor.rs:190` | take | A | - | One `DELETE … RETURNING`. |
| `apps/server/src/auth_routes/two_factor.rs:219` | restore | A | - | One `INSERT … ON CONFLICT DO NOTHING`. |
| `apps/server/src/auth_routes/throttle.rs:129` | record_failure | G | - | Per-email rate counter: read, compute, upsert. |
| `apps/server/src/email_quota.rs:28` | take | G | - | Email caps by address, user or workspace: count, then insert. |
| `apps/server/src/repositories/api_idempotency.rs:32` | claim_idempotency_key | A | - | `INSERT ON CONFLICT DO NOTHING` decides the claim. |
| `apps/server/src/oauth.rs:347` | register | G | - | Per-IP and install-wide client caps. |
| `apps/server/src/oauth.rs:409` | prune_clients | G | - | Cluster-wide client cleanup. |
| `apps/server/src/oauth.rs:515` | authorize | A | - | Deletes expired requests, inserts one. |
| `apps/server/src/oauth.rs:570` | consent | A | - | One guarded UPDATE, then reads. |
| `apps/server/src/oauth.rs:639` | approve | G | - | Grants for up to 20 workspaces; final UPDATE has no single-use guard. |
| `apps/server/src/oauth.rs:768` | token | G | - | Code and refresh reuse detection: read, then write. |
| `apps/server/src/oauth.rs:924` | disconnect | A | - | One guarded UPDATE and audit. |
| `apps/server/src/repositories/instance_settings.rs:205` | set_registration_open | G | - | `instance_settings`: open only when email works. |
| `apps/server/src/repositories/instance_settings.rs:285` | set_mail_switches | G | - | `instance_settings` singleton. |
| `apps/server/src/repositories/instance_settings.rs:353` | save_smtp | G | - | `instance_settings` read-then-write. |
| `apps/server/src/repositories/instance_settings.rs:405` | clear_smtp | G | - | `instance_settings` with the registration rule. |
| `apps/server/src/repositories/instance_settings.rs:459` | save_limits | G | - | Storage quota and owned-workspace limit that W txs read. |
| `apps/server/src/repositories/instance_settings.rs:545` | save_s3 | G | - | `instance_settings` S3 and files bucket. |
| `apps/server/src/repositories/instance_settings.rs:592` | clear_s3 | G | - | Same. |
| `apps/server/src/repositories/instance_settings.rs:628` | save_storage_options | G | - | Files bucket rule. |
| `apps/server/src/repositories/instance_settings.rs:727` | keep_files_bucket | G | - | Files bucket record. |
| `apps/server/src/repositories/instance_settings.rs:765` | forget_files_bucket | G | - | Files bucket record (CLI). |
| `apps/server/src/repositories/instance_settings.rs:817` | save_job_limits | G | - | `job_limits`, which job claim reads. |
| `apps/server/src/repositories/account.rs:100` | delete_account_unchecked | G | - | User row and memberships in every workspace. |
| `apps/server/src/repositories/account.rs:206` | store_email_change_token | G | - | One pending link per user. |
| `apps/server/src/repositories/account.rs:249` | confirm_email_change | G | - | Email unique across users. |
| `apps/server/src/repositories/account.rs:304` | change_email_as_admin | G | - | Email unique across users. |
| `apps/server/src/repositories/sign_in_providers.rs:240` | provider_account | G | - | Email and identity unique; may create a user. |
| `apps/server/src/repositories/sign_in_providers.rs:352` | create_provider_session_audited | G | - | Session vs suspend or unlink. |
| `apps/server/src/repositories/sign_in_providers.rs:416` | connect_identity | G | - | One identity per (provider, subject). |
| `apps/server/src/repositories/sign_in_providers.rs:448` | confirm_with_provider | G | - | Reauthentication vs disconnect. |
| `apps/server/src/repositories/sign_in_providers.rs:508` | disconnect_identity | G | - | A user keeps at least one sign-in method. |
| `apps/server/src/repositories/sign_in_providers.rs:614` | save_sign_in_provider | A | - | Upsert `ON CONFLICT (provider)` and audit. |
| `apps/server/src/repositories/sign_in_providers.rs:669` | remove_sign_in_provider | G | - | Registration rule counts providers. |
| `apps/server/src/repositories/api_tokens.rs:274` | create | W | `workspace_id` | Service account name unique per workspace (index). |
| `apps/server/src/repositories/api_tokens.rs:535` | revoke | W | `workspace_id` | Guarded UPDATE of a token of the workspace. |
| `apps/server/src/repositories/identity.rs:315` | record_security_event | A | - | One global audit insert. |
| `apps/server/src/repositories/identity.rs:336` | initialize_setup_token | G | - | `installation_state` singleton. |
| `apps/server/src/repositories/identity.rs:371` | rotate_setup_token | G | - | Same. |
| `apps/server/src/repositories/identity.rs:428` | store_setup_token | G | - | Same. |
| `apps/server/src/repositories/identity.rs:504` | complete_setup_inner | G | - | Creates the root user and first workspace. |
| `apps/server/src/repositories/identity.rs:697` | set_suspended_unchecked | G | - | User row and all sessions. |
| `apps/server/src/repositories/identity.rs:783` | store_registration_token | G | - | One link per email; no unique index. |
| `apps/server/src/repositories/identity.rs:815` | register_open_account | G | - | Email unique; creates user and session. |
| `apps/server/src/repositories/identity.rs:993` | issue_recovery_token_as_admin | G | - | Admin checks on users. |
| `apps/server/src/repositories/identity.rs:1042` | set_instance_admin | G | - | `users.installation_admin`. |
| `apps/server/src/repositories/identity.rs:1106` | set_root_as_operator | G | - | `installation_state.root_user_id`. |
| `apps/server/src/repositories/identity.rs:1160` | is_root | A | - | Read only. Needs no write tx. |
| `apps/server/src/repositories/identity.rs:1298` | update_profile_audited | A | - | One UPDATE of `users`, audits in each workspace of the user. |
| `apps/server/src/repositories/identity.rs:1456` | set_avatar_audited | G | - | Avatar blob scope is the user id; races reconcile and `replace_blob`. |
| `apps/server/src/repositories/identity.rs:1483` | remove_avatar_audited | A | - | One DELETE and audits. |
| `apps/server/src/repositories/identity.rs:1504` | change_password_keeping_session | G | - | Password vs concurrent login. |
| `apps/server/src/repositories/identity.rs:1551` | store_recovery_token | G | - | One token per user; no unique index. |
| `apps/server/src/repositories/identity.rs:1565` | store_recovery_token_audited | G | - | Same. |
| `apps/server/src/repositories/identity.rs:1614` | complete_recovery | G | - | Single-use token; password vs login. |
| `apps/server/src/repositories/identity.rs:1667` | complete_recovery_audited | G | - | Same. |
| `apps/server/src/repositories/identity.rs:1778` | create_session_audited | G | - | Login vs password change or suspend. |
| `apps/server/src/repositories/identity.rs:2028` | revoke_session_audited_unchecked | A | - | One guarded UPDATE and audit. |
| `apps/server/src/repositories/two_factor.rs:200` | enable_totp | G | - | Recovery code set vs regenerate. |
| `apps/server/src/repositories/two_factor.rs:256` | use_recovery_code | A | - | One DELETE; row count decides. |
| `apps/server/src/repositories/two_factor.rs:295` | regenerate_recovery_codes | G | - | Reads TOTP state, replaces codes. |
| `apps/server/src/repositories/two_factor.rs:329` | disable_totp | A | - | Two unconditional DELETEs. |
| `apps/server/src/repositories/two_factor.rs:356` | reset_two_factor_as_admin | G | - | Admin checks on users. |
| `apps/server/src/repositories/two_factor.rs:429` | save_passkey | A | - | `INSERT ON CONFLICT (credential_id)`. |
| `apps/server/src/repositories/two_factor.rs:475` | delete_passkey | G | - | Last sign-in method rule. |
| `apps/server/src/repositories/two_factor.rs:540` | create_passkey_session_audited | G | - | Login vs suspend or delete. |

## Imports

No importer creates users. `import_people` only maps a person to a member who already exists. The `create`,
`start`, `insert` and `store_upload` sites are G for one reason: they count the user's active imports across all
workspaces, then write. No unique index enforces this.

| Site | Function | Class | Workspace id source | Reason |
|---|---|---|---|---|
| `apps/server/src/orbit_import/mod.rs:630` | create | G | - | One active import per user, across workspaces. |
| `apps/server/src/orbit_import/mod.rs:723` | store_upload | G | - | Same. |
| `apps/server/src/orbit_import/mod.rs:856` | start | G | - | Same. |
| `apps/server/src/orbit_import/mod.rs:1447` | plan | A | - | Inserts `ON CONFLICT (import_id, kind, source_id)`. |
| `apps/server/src/orbit_import/mod.rs:1682` | fail_item | A | - | Two atomic UPDATEs by import id. |
| `apps/server/src/orbit_import/chat.rs:567` | write_message | W | `run.workspace_id` (pass it in) | Message, reactions and links `ON CONFLICT`. |
| `apps/server/src/orbit_import/chat.rs:724` | refresh_conversation | W | `run.workspace_id` (pass it in) | Rewrites counters; must exclude `send_message` of the same workspace. |
| `apps/server/src/obsidian/import.rs:612` | insert | G | - | One active import per user. |
| `apps/server/src/obsidian/import.rs:762` | start | G | - | Same. |
| `apps/server/src/obsidian/import.rs:1739` | fail_item | A | - | Two atomic UPDATEs by import id. |
| `apps/server/src/asana/import.rs:476` | create | G | - | One active import per user. |
| `apps/server/src/asana/import.rs:607` | start | G | - | Same. |
| `apps/server/src/asana/import.rs:1151` | list_tasks | W | `run.workspace_id` | Item inserts `ON CONFLICT`; reuse reads this workspace only. |
| `apps/server/src/asana/import.rs:1776` | fail_item | A | - | Two atomic UPDATEs. |
| `apps/server/src/trello/import.rs:540` | create | G | - | One active import per user. |
| `apps/server/src/trello/import.rs:672` | start | G | - | Same. |
| `apps/server/src/trello/import.rs:1172` | prepare_board | A | - | Two plain UPDATEs by import id. |
| `apps/server/src/trello/import.rs:1266` | list_cards | W | `run.workspace_id` | Same as asana list_tasks. |
| `apps/server/src/trello/import.rs:1760` | fail_item | A | - | Two atomic UPDATEs. |
| `apps/server/src/linear/import.rs:557` | create | G | - | One active import per user. |
| `apps/server/src/linear/import.rs:687` | start | G | - | Same. |
| `apps/server/src/linear/import.rs:1298` | list_issues | W | `run.workspace_id` | Inserts `ON CONFLICT`, cursor, reuse in this workspace. |
| `apps/server/src/linear/import.rs:2284` | fail_item | A | - | Two atomic UPDATEs. |
| `apps/server/src/notion/import.rs:517` | create | G | - | One active import per user. |
| `apps/server/src/notion/import.rs:692` | start | G | - | Same. |
| `apps/server/src/notion/import.rs:1938` | fail_item | A | - | Two atomic UPDATEs. |
| `apps/server/src/import_people.rs:415` | map | W | `workspace_id` | Maps to an existing member; reads users only. |
| `apps/server/src/import_people.rs:478` | move_tasks | W | `workspace_id` (job payload) | Up to 200 `update_task_in_tx` calls per tx, all in the workspace. |

## Jobs and platform

| Site | Function | Class | Workspace id source | Reason |
|---|---|---|---|---|
| `crates/platform/src/jobs/store.rs:115` | claim | G | - | Per-kind and per-workspace running limits read across all workspaces, then a lease. |
| `crates/platform/src/jobs/store.rs:340` | complete | A | - | Compare-and-set on `claim_token`. |
| `crates/platform/src/jobs/store.rs:365` | fail | A | - | Only the claim-token UPDATE decides. |
| `crates/platform/src/jobs/store.rs:432` | release | A | - | Compare-and-set, then delete its own attempt row. |
| `apps/server/src/live/events.rs:37` | append | W | `workspace_id` | `MAX(seq)+1` per workspace, insert, trim. |
| `crates/platform/src/files/upload.rs:789` | finalize | W | `upload.workspace_id` | Upload and reference in one workspace. Only tests call it. |
| `crates/platform/src/files/upload.rs:823` | delete_blob | W | `workspace_id` | One UPDATE with NOT EXISTS; races an emoji finalize without a lock, so not A. |
| `crates/platform/src/files/upload.rs:907` | replace_blob | W | blob `workspace_id`, read before the tx (user id for avatars) | CAS on the blob, then references. See unsure list. |
| `crates/platform/src/files/upload.rs:1026` | unclaim | W | `claimed.blob.workspace_id` (user id for avatars) | Counts references, then deletes. See unsure list. |
| `crates/platform/src/files/upload.rs:1057` | claim_blob | W | `upload.workspace_id` (user id for avatars) | Quota per workspace; blob row `ON CONFLICT`. |
| `crates/platform/src/files/upload.rs:1149` | reconcile | G | - | Reference count and delete over blobs of all workspaces. |
| `crates/platform/src/files/upload.rs:1205` | reconcile | G | - | Untracked object has no workspace; deletes from the store inside the tx. |
| `apps/server/src/object_storage.rs:375` | move_images_to_blobs | G | - | One-off move of avatars (user scope) and emoji. |
| `apps/server/src/app.rs:1685` | composed_backup_waits_for_retention_attachment_mutation | test | - | Test code; not counted. |

## Other: workspaces, members, files, billing, export, integrations

| Site | Function | Class | Workspace id source | Reason |
|---|---|---|---|---|
| `apps/server/src/repositories/workspaces.rs:491` | create | G | - | Per-user cap of owned workspaces, counted across workspaces; `users.trial_started_at`. |
| `apps/server/src/repositories/workspaces.rs:705` | delete_storage_file | A | - | Audit insert only. |
| `apps/server/src/repositories/workspaces.rs:732` | rename | W | `workspace_id` | Version CAS; names are not unique. |
| `apps/server/src/repositories/workspaces.rs:884` | set_note | W | `workspace_id` | `user_notes` written by one upsert or one delete. |
| `apps/server/src/repositories/workspaces.rs:965` | invite | W | `workspace_id` | Invitations per (workspace, email) with a unique index. |
| `apps/server/src/repositories/workspaces.rs:1077` | revoke_invitation | W | `workspace_id` | One conditional UPDATE. |
| `apps/server/src/repositories/workspaces.rs:1166` | accept_invitation | W | invitation `workspace_id`, read by token hash before the tx (`invitation_write_lock`) | Membership unique; `email_verified_at` by one COALESCE. If the token is not found, only a global audit insert runs. |
| `apps/server/src/repositories/workspaces.rs:1323` | register_invited_account | G | - | Creates a user and a session; email unique. |
| `apps/server/src/repositories/workspaces.rs:1581` | record_registration_failure | A | - | Audit insert only. |
| `apps/server/src/repositories/workspaces.rs:1629` | change_role | W | `workspace_id` | Version CAS; trial capacity; channel joins `ON CONFLICT`. |
| `apps/server/src/repositories/workspaces.rs:1754` | remove_member_unchecked | W | `workspace_id` | Mailboxes, membership, tokens, grants and exports filtered by workspace. |
| `apps/server/src/repositories/workspaces.rs:1860` | transfer_ownership | G | - | Target's owned-workspace cap across workspaces. |
| `apps/server/src/repositories/workspaces.rs:1976` | set_deleted_unchecked | G | - | Restore checks the owner's owned-workspace cap. |
| `apps/server/src/repositories/workspaces.rs:2316` | purge_expired | G | - | Picks tasks across all workspaces. |
| `apps/server/src/repositories/workspaces.rs:2354` | purge_expired | A | - | One DELETE over page versions. |
| `apps/server/src/repositories/workspaces.rs:2358` | purge_expired | G | - | Picks projects across workspaces. |
| `apps/server/src/repositories/workspaces.rs:2409` | purge_expired | W | `workspace_id` from the expired list | Blob batch of one deleted workspace. |
| `apps/server/src/repositories/workspaces.rs:2452` | purge_expired | W | `workspace_id` from the expired list | Pending uploads of one workspace. |
| `apps/server/src/repositories/workspaces.rs:2492` | purge_expired | W | `workspace_id` from the expired list | Deletes the workspace row. |
| `apps/server/src/repositories/workspaces.rs:2520` | purge_expired | A | - | One DELETE. |
| `apps/server/src/repositories/workspaces.rs:2543` | delete_in_batches | G | - | `DELETE … WHERE id IN (SELECT … LIMIT)` across workspaces; the subquery is not re-checked on Postgres. |
| `apps/server/src/repositories/workspaces.rs:2563` | record_maintenance | A | - | Insert only. |
| `apps/server/src/repositories/workspaces.rs:2611` | purge_attachment_files | G | - | Cluster-wide deletion queue checked against blobs of any workspace. |
| `apps/server/src/repositories/workspaces.rs:2669` | requeue_file_deletion | A | - | One `INSERT ON CONFLICT DO NOTHING`. |
| `apps/server/src/repositories/workspace_roles.rs:211` | set_project_grant | W | `workspace_id` | Grants of one project and user. |
| `apps/server/src/repositories/workspace_roles.rs:284` | revoke_project_grant | W | `workspace_id` | One DELETE. |
| `apps/server/src/repositories/workspace_roles.rs:328` | create_role | W | `workspace_id` | Role cap and name per workspace. |
| `apps/server/src/repositories/workspace_roles.rs:392` | update_role | W | `workspace_id` | Version CAS, name per workspace. |
| `apps/server/src/repositories/workspace_roles.rs:459` | delete_role | W | `workspace_id` | One delete. |
| `apps/server/src/repositories/workspace_roles.rs:499` | set_member_custom_role | W | `workspace_id` | Membership version check. |
| `apps/server/src/repositories/workspace_storage.rs:104` | save | W | `workspace_id` | Upsert `ON CONFLICT (workspace_id)`. |
| `apps/server/src/repositories/workspace_storage.rs:151` | remove | W | `workspace_id` | Conditional UPDATE. |
| `apps/server/src/repositories/workspace_storage.rs:182` | forget_drained | A | - | One DELETE. |
| `apps/server/src/repositories/attachments.rs:161` | list | A | - | Read only. Needs no write tx. |
| `apps/server/src/repositories/attachments.rs:256` | delete | W | `workspace_id` | Reference delete; quarantine bump is one UPDATE. |
| `apps/server/src/repositories/attachments.rs:502` | finalize_for_member | W | `workspace_id` (= `upload.workspace_id`) | Permission read, then finalize in one workspace. |
| `apps/server/src/repositories/attachments.rs:589` | finalize_inner | W | `workspace_id` (checked equal to `upload.workspace_id`) | Comment and reference insert in one workspace. |
| `apps/server/src/repositories/billing.rs:408` | record_audit | A | - | Audit insert only. |
| `apps/server/src/repositories/billing.rs:469` | apply_subscription | W | `workspace_id` | Upsert `ON CONFLICT (workspace_id)`; customer id unique. |
| `apps/server/src/repositories/backup_items.rs:483` | write_pages | W | `meta.workspace_id` | Trial and row capacity, page tree, files in one workspace. |
| `apps/server/src/repositories/backup_items.rs:856` | write_task | W | `meta.workspace_id` | Trial capacity, positions, task rows. |
| `apps/server/src/workspace_export/mod.rs:324` | queue | W | `workspace_id` | One active export per (workspace, requester, mode); unique index. |
| `apps/server/src/workspace_export/mod.rs:473` | download | A | - | Audit insert only. |
| `apps/server/src/workspace_export/mod.rs:509` | run | A | - | One `UPDATE … RETURNING`. |
| `apps/server/src/workspace_export/mod.rs:572` | run | W | `workspace` (returned by the first tx) | Keep-newest update per (workspace, requester, mode). |
| `apps/server/src/workspace_export/mod.rs:614` | fail | A | - | One conditional UPDATE. |
| `apps/server/src/workspace_export/mod.rs:628` | expire | A | - | One UPDATE over all workspaces. |
| `apps/server/src/workspace_export/mod.rs:663` | delete_expired | A | - | One DELETE by id. |
| `apps/server/src/webhooks/routes.rs:244` | write | W | `self.workspace_id` (route path) | Subscriptions and deliveries of the workspace; audit and enqueue. |
| `apps/server/src/integration_routes.rs:460` | save_github_project_connection | A | - | One upsert `ON CONFLICT (project_id)`. W with path `workspace` is also fine. |
| `apps/server/src/integration_routes.rs:507` | delete_github_project_connection | A | - | One DELETE. |
| `apps/server/src/integration_routes.rs:766` | store_github_app | A | - | Atomic DELETE of the registration, insert with unique `app_id`. |
| `apps/server/src/integration_routes.rs:1029` | github_webhook_from_database | A | - | One guarded UPDATE. |
| `apps/server/src/integration_routes.rs:1176` | update_github_installation | W | `workspace` (resolved by webhook signature before the tx) | Upsert `ON CONFLICT … WHERE workspace_id = excluded.workspace_id`, then ownership check. |

## Helpers that open the transaction for many callers

Move these first: one change covers many uses.

- `live::events::append` through `LiveHub::publish` (`apps/server/src/live/hub.rs:737`). 21 callers. It runs as a
  second write transaction after almost every chat, page and task write.
- `CollabHub::write_transaction` (`apps/server/src/collab/hub.rs:361`). Used by `initialize`, `rebuild` and
  `persist`. `persist` runs on the tick, on flush, on `flush_workspace` and at shutdown.
- `insert_task` (`tasks.rs:2170`), through `create_task` (8 callers) and `import_task` (8 callers).
- `update_task` (`tasks.rs:2730`), 9 callers.
- `import_comment` (`tasks.rs:3657`), through `create_comment` (7 callers) and 2 direct callers.
- `insert_page`, `write_page` (3 callers each), `move_page_expecting`, `delete_page_expecting`.
- `DatabaseRepository::insert_row` (`rows.rs:103`), 4 callers. `update_values` (`rows.rs:259`), 3 callers.
- `UploadService::claim_blob` (`upload.rs:1057`), reached from every upload, emoji, sticker, mail receipt and avatar
  (12 sites through `prepare`, `prepare_bytes` and `prepare_unmetered_bytes`).
- `webhooks::routes::Call::write` (`webhooks/routes.rs:244`), 5 handlers.
- `JobStore::complete`, `fail` and `release`: run once after every job (`crates/platform/src/jobs/worker.rs:384`).
  They are A. Moving them to a plain `transaction()` removes one or two global lock waits per job.
- `JobStore::claim`: every worker on every server polls it, and it stays G. After the W move, it is the main
  remaining global writer. Each claim stops all workspace transactions while it runs.

## Counts

Out of 336 production call sites:

| Class | Sites |
|---|---|
| W | 198 |
| G | 83 |
| A | 52 |
| W or G by scope (mail providers) | 3 |

By area: chat W 43, G 1, A 2. Docs and collab W 24, A 1. Tasks W 47, A 3. Databases W 21, G 1. Mail W 20, G 6,
A 2, split 3. Accounts and auth W 2, G 50, A 15. Imports W 7, G 13, A 8. Jobs and platform W 6, G 4, A 3. Other W 28,
G 8, A 18. A script checked these numbers against the tables and the `rg` list.

## First wave

These W sites carry the most traffic. Each one is safe for the reason given. They are in order of expected gain.

1. **`live::events::append`** (`live/events.rs:37`). It runs after most user writes. The sequence is `MAX(seq)+1`
   per workspace, and the trim deletes only rows of that workspace.
2. **`send_message`** (`chat/messages.rs:313`). All rows hang off one conversation that `load_access` ties to
   `workspace_id`. It writes only chat tables and per-workspace notifications. The DM block check only reads
   `user_blocks`.
3. **`mark_read`, `mark_thread_read`, `update_state`** (`chat/state.rs:94`, `:192`, `:435`). They change one user's
   member row of one conversation in the workspace, and that workspace's notifications.
4. **`set_reaction`** (`chat/messages.rs:799`). The reaction cap is per message. The insert uses `ON CONFLICT`.
5. **Collab `persist`** (`collab/hub.rs:967`), with `initialize` (`:826`). The workspace id is set in `load()`
   before any transaction, and a page never changes workspace. The log and the snapshot are per page. Every
   writer to one page's log holds that page's workspace lock, so the per-page `seq` order stays correct.
6. **`write_page`** (`pages.rs:1094`). A version-guarded UPDATE, per-page versions, and the per-page collab log.
7. **`update_task`** (`tasks.rs:2730`). Every row is in the workspace. A project move uses the atomic per-project
   counter. Notifications use `ON CONFLICT`.
8. **`update_many_tasks`** (`tasks.rs:2936`). Board drag and bulk edit. Version-checked updates in one workspace.
9. **`insert_task`** (`tasks.rs:2170`). Trial capacity and positions are per workspace. The number comes from the
   atomic counter trigger, backed by `tasks_project_number`.
10. **`edit_message`, `delete_message`** (`chat/messages.rs:728`, `:763`). Counters of one conversation.
11. **`import_comment`** (`tasks.rs:3657`). Task comment create. Insert and notifications in the workspace.
12. **`add_comment`** (`page_comments.rs:374`). Per-thread writes, notification and audit inserts.
13. **`update_values`** (`databases/rows.rs:259`). Cell edits. Value rows and dependents of the workspace.
14. **`insert_row`, `move_row`** (`databases/rows.rs:103`, `:395`). Row cap, positions and counters are per database.
15. **`update_threads`** (`mail/reading.rs:675`). Mail read, star, label, move. Every row is filtered by workspace
    and membership. User state uses upserts.
16. **`ingest`** (`mail/ingest.rs:245`). Every received mail. The receipt's workspace is read before the
    transaction, and a trigger keeps it fixed. Unique constraints make retries safe.
17. **`claim_blob`, `finalize_inner`** (`upload.rs:1057`, `attachments.rs:589`). Every upload. The quota is per
    workspace. The blob row uses `ON CONFLICT` on a per-workspace key. Read the avatar item in the unsure list first.
18. **`post_as_bot`** (`chat/messages.rs:465`) and **`webhooks::routes::Call::write`** (`webhooks/routes.rs:244`).
    Bot and webhook bursts. The workspace comes from the webhook or the route before the transaction.
19. **`move_tasks`** (`import_people.rs:478`) and the importer list steps (`asana/import.rs:1151`,
    `trello/import.rs:1266`, `linear/import.rs:1298`). Few calls, but they hold the lock for a long time. A code
    comment in `import_people.rs` says that 4,000 tasks in one transaction held every writer for 15 seconds. All rows
    are in `run.workspace_id`. Inserts use `ON CONFLICT`.

## Unsure

Do not move these sites until the question has an answer.

1. **`use_response_token`** (`chat/interactions.rs:180`). The workspace is found inside the transaction through the
   webhook. Two parallel uses can both read `uses_left = 1`. The CHECK then fails one of them with an error, not
   with `None`. Should it look up the workspace from `webhook_id` before the transaction (a webhook never changes
   workspace), or become one `UPDATE … WHERE uses_left > 0 AND expires_at >= $n RETURNING`?
2. **`prepare_click`** (`chat/interactions.rs:60`). It also runs `DELETE FROM chat_action_responses WHERE
   expires_at < $n` over all workspaces. The statement is atomic, but two workspaces can now run it at the same
   time. Can the two deletes deadlock on rows in a different order? Should the delete move to a job, or be scoped
   to the webhook?
3. **Avatar blob scope** (`upload.rs:907`, `:1026`, `:1057`). For a profile picture, `attachment_blobs.workspace_id`
   holds the user id. Does `workspace_transaction` accept a scope id that is not a workspace? Do all avatar writers
   (`identity.rs:1456`, `replace_blob`, `unclaim`) use the same key? Until then, the avatar path must keep the
   global lock.
4. **`record_dns_report`** (`mail/domains.rs:761`). The workspace is found inside the transaction, and
   `move_domain` can change it. Is it enough to read the workspace before the transaction and check it again inside
   (retry on a mismatch)?
5. **`fill_search_text`** (`databases/search.rs:210`). It picks a row of any workspace inside the transaction.
   Can the pick move before the transaction, with that row's workspace as the key?
6. **`MailIntake::accept`** (`mail_intake.rs:189`). One transaction writes for every recipient workspace. Should
   `workspace_transaction` take several workspace locks in sorted order, or should the transaction split per
   workspace?
7. **Importer `create` and `start`** (13 sites in `orbit_import`, `obsidian`, `asana`, `trello`, `linear`,
   `notion`). The rule "one active import per user" spans workspaces. A partial unique index on
   `user_id WHERE status IN (active)` would make each of them W. Do we want that index?
8. **`block_user`** (`chat/safety.rs:292`). It is atomic. If it ever leaves the global lock, a DM send that is
   already running can commit just after the block. Is that acceptable? On the global lock it stays exact.
9. **`delete_in_batches`** (`workspaces.rs:2543`). On Postgres the `IN (SELECT … LIMIT)` subquery is not checked
   again after a concurrent update. A parallel chat send or page restore could lose a row. It stays G. Should the
   predicate also go into the outer `WHERE`?
10. **Mail providers** (`mail/providers.rs:517`, `:571`, `:645`). The class depends on the scope. The call site must
    branch: `workspace_transaction` for `ProviderScope::Workspace`, `immediate_transaction` for the instance scope.
11. **`write_message`, `refresh_conversation`** (`orbit_import/chat.rs:567`, `:724`). They are W, but they do not
    take a workspace id today. The caller has `run.workspace_id` and must pass it in.

## Notes for the change

- **`LiveHub::write_lock`.** `invitation_write_lock` (`workspaces.rs:1140`) already takes an in-process lock per
  workspace through `LiveHub::write_lock`. Check how it orders against the new database lock, so that a server
  does not wait on its own mutex while it holds the workspace lock in a different order.
- **Read-only sites.** `attachments.rs:161` (`list`) and `identity.rs:1160` (`is_root`) open a write transaction
  but write nothing. They can use the pool, with no lock.
- **Postgres errors.** A W transaction can still fail with a lock timeout (55P03). The 30 second `lock_timeout` in
  `POSTGRES_WRITE_LOCK` must apply to both advisory locks.

## Status (2026-11-19)

These sites use the new scopes now. All other sites still use `immediate_transaction()` (global).

| Scope | Sites |
|---|---|
| Jobs (`jobs_transaction`) | `JobStore::claim`, `complete`, `fail`, `release` (`crates/platform/src/jobs/store.rs`) |
| Workspace | `live::events::append`; chat `send_message`, `edit_message`, `delete_message`, `set_reaction`, `mark_read`, `mark_thread_read`, `update_state`; collab `persist`, `initialize`; `write_page`; page `add_comment`; `insert_task`, `update_task`, `update_many_tasks`, `import_comment`; database `insert_row`, `update_values`, `move_row`; mail `update_threads`, `ingest` |

Writers of one page or task collab log now hold that workspace's lock or the global lock. `rebuild`, `unlink_trashed`, `restore_page_version`, the GitHub sync and the cascade deletes stay global, which is still correct. `ORBIT_WRITE_LOCK=global` turns every scope back into the global lock. The race tests are in `crates/platform/tests/write_lock.rs`.
