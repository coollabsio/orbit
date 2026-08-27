import { ago } from '../../lib/format'
import type { Channel, ChatCategory, ChatMessage } from '../types'

export const chatCategories: ChatCategory[] = [
  { id: 'cc_general', name: 'General' },
  { id: 'cc_engineering', name: 'Engineering' },
  { id: 'cc_support', name: 'Support' },
]

export const channels: Channel[] = [
  { id: 'c_general', name: 'general', description: 'Team-wide announcements and chatter', categoryId: 'cc_general', unreadCount: 0 },
  { id: 'c_design', name: 'design', description: 'UI, UX, and visual direction', categoryId: 'cc_general', unreadCount: 0 },
  { id: 'c_engineering', name: 'engineering', description: 'Backend, frontend, and everything between', categoryId: 'cc_engineering', unreadCount: 3 },
  { id: 'c_infra', name: 'infra', description: 'Servers, deploys, and incidents', categoryId: 'cc_engineering', unreadCount: 1 },
  { id: 'c_support', name: 'support', description: 'Forwarded customer conversations', categoryId: 'cc_support', unreadCount: 2 },
]

let m = 0

function msg(
  channelId: string,
  authorId: string,
  content: string,
  createdAt: string,
  extra?: Partial<ChatMessage>,
): ChatMessage {
  m += 1
  return {
    id: `cm_${m}`,
    channelId,
    authorId,
    authorType: 'user',
    replyToId: null,
    content,
    reactions: [],
    createdAt,
    editedAt: null,
    pinned: false,
    threadRootId: null,
    startsThread: false,
    threadTitle: null,
    ...extra,
  }
}

export const chatMessages: ChatMessage[] = [
  // general
  msg('c_general', 'u_shadow', 'Morning everyone! Reminder: weekly sync moved to 14:00 today.', ago(26, 'h')),
  msg('c_general', 'u_adiology', 'Works for me 👍', ago(25, 'h')),
  msg('c_general', 'u_peak', 'I will share the new empty-state illustrations right after.', ago(25, 'h'), {
    reactions: [{ emoji: '🎉', userIds: ['u_shadow', 'u_adiology'] }],
  }),
  msg('c_general', 'u_cinzya', 'Heads up: rotating TLS certs Thursday evening, expect a brief blip on internal tools.', ago(4, 'h')),

  // engineering
  msg('c_engineering', 'u_andras', 'Found the GitHub duplicate-task bug — webhook retries were not idempotent.', ago(7, 'h')),
  msg('c_engineering', 'u_andras', 'Fix is to dedupe on the issue node id before insert.', ago(7, 'h')),
  msg('c_engineering', 'u_shadow', 'Nice. Can you add an activity entry when a duplicate is skipped?', ago(6, 'h'), {
    reactions: [{ emoji: '👍', userIds: ['u_andras'] }],
  }),
  msg(
    'c_engineering',
    'gh',
    'PR #191 merged: fix(github): dedupe issue webhooks by node id',
    ago(3, 'h'),
    { authorType: 'github', externalAuthor: { name: 'GitHub', source: 'team/workspace' } },
  ),
  msg('c_engineering', 'u_adiology', 'The proxy appears to drop websocket upgrades when the config reloads — @shadowarcanist anyone else seeing this?', ago(2, 'h')),
  msg('c_engineering', 'u_shadow', 'Yes, that matches INF-101. My patch re-attaches listeners after reload.', ago(1, 'h')),

  // infra
  msg('c_infra', 'u_cinzya', 'Staging deploy at 11:40 went clean. Smoke checks green.', ago(8, 'h')),
  msg('c_infra', 'u_shadow', 'Proxy patch is on staging now. Watching error rates for an hour before promoting.', ago(3, 'h')),
  msg('c_infra', 'u_cinzya', '502 rate on staging dropped to zero since the patch 📉', ago(30, 'm'), {
    reactions: [{ emoji: '🚀', userIds: ['u_shadow', 'u_andras', 'u_adiology'] }],
    pinned: true,
  }),

  // design
  msg('c_design', 'u_peak', 'Uploaded the dark-theme token pass — accent switches to yellow like Coolify does.', ago(2, 'd')),
  msg('c_design', 'u_adiology', 'Looks great in the sidebar. The purple gradient stays for primary buttons, right?', ago(2, 'd')),
  msg('c_design', 'u_peak', 'Exactly — gradient primary in both themes, accent only for selection and focus.', ago(2, 'd')),

  // support (with Discord-forwarded messages)
  msg(
    'c_support',
    'dc',
    'The deployment gives me a 502 after tonight\'s update. Anyone around?',
    ago(5, 'h'),
    { authorType: 'discord', externalAuthor: { name: 'JordanR', source: '#support' } },
  ),
  msg('c_support', 'u_shadow', 'That is the proxy issue — replied on the mail thread and posted a status update.', ago(4, 'h')),
  msg(
    'c_support',
    'dc',
    'Thanks! Subscribed to the status page.',
    ago(4, 'h'),
    { authorType: 'discord', externalAuthor: { name: 'JordanR', source: '#support' } },
  ),
]

// the chat reference demo content: markdown + code block, and a thread with replies
chatMessages.push(
  msg(
    'c_engineering',
    'u_andras',
    [
      '## Webhook dedupe',
      'Proposed fix for the duplicate-task bug — dedupe on the **issue node id** before insert:',
      '```rust',
      'fn dedupe(existing: &HashSet<String>, node_id: &str) -> bool {',
      '    // skip when the id was already seen',
      '    !existing.contains(node_id)',
      '}',
      '```',
      '> Retries are idempotent after this change.',
      '- add an activity entry when a duplicate is skipped',
      '- backfill the `node_id` column for old rows',
      'Docs: [GitHub webhooks](https://docs.github.com/webhooks)',
    ].join('\n'),
    ago(90, 'm'),
  ),
  msg('c_engineering', 'u_shadow', 'Should we also retry on 5xx from the GitHub API? Opening a thread for this.', ago(80, 'm')),
)
const threadRoot = chatMessages[chatMessages.length - 1]
chatMessages.push(
  msg('c_engineering', 'u_andras', 'Yes — exponential backoff, max 5 attempts.', ago(70, 'm'), {
    threadRootId: threadRoot.id,
  }),
  msg('c_engineering', 'u_cinzya', 'Please cap the total wait at 2 minutes so deploys do not hang.', ago(60, 'm'), {
    threadRootId: threadRoot.id,
  }),
  msg('c_engineering', 'u_shadow', 'Agreed. Andras, can you add it to the fix PR?', ago(40, 'm'), {
    threadRootId: threadRoot.id,
  }),
)

// the chat reference webhook message with a webhook-compatible embed
chatMessages.push(
  msg('c_infra', 'wh_deploy', '', ago(35, 'm'), {
    authorType: 'webhook',
    webhookName: 'Deploy Bot',
    webhookIconUrl: null,
    embeds: [
      {
        title: ':white_check_mark: Deploy succeeded — website',
        description: 'Build **#412** finished and the new release is live.',
        color: 0x1f8b4c,
        fields: [
          { name: 'Environment', value: 'Production', inline: true },
          { name: 'Duration', value: '42s', inline: true },
          { name: 'Commit', value: '`3f9c2a1`', inline: true },
        ],
        footer: { text: 'Coolify · v4.3.12' },
        timestamp: ago(35, 'm'),
      },
    ],
  }),
)
