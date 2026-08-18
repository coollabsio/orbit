import { ago } from '../../lib/format'
import type { Channel, ChatMessage } from '../types'

export const channels: Channel[] = [
  { id: 'c_general', name: 'general', description: 'Team-wide announcements and chatter', unreadCount: 0 },
  { id: 'c_engineering', name: 'engineering', description: 'Backend, frontend, and everything between', unreadCount: 3 },
  { id: 'c_infra', name: 'infra', description: 'Servers, deploys, and incidents', unreadCount: 1 },
  { id: 'c_design', name: 'design', description: 'UI, UX, and visual direction', unreadCount: 0 },
  { id: 'c_support', name: 'support', description: 'Forwarded customer conversations', unreadCount: 2 },
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
    ...extra,
  }
}

export const chatMessages: ChatMessage[] = [
  // general
  msg('c_general', 'u_alice', 'Morning everyone! Reminder: weekly sync moved to 14:00 today.', ago(26, 'h')),
  msg('c_general', 'u_carol', 'Works for me 👍', ago(25, 'h')),
  msg('c_general', 'u_eve', 'I will share the new empty-state illustrations right after.', ago(25, 'h'), {
    reactions: [{ emoji: '🎉', userIds: ['u_alice', 'u_carol'] }],
  }),
  msg('c_general', 'u_dan', 'Heads up: rotating TLS certs Thursday evening, expect a brief blip on internal tools.', ago(4, 'h')),

  // engineering
  msg('c_engineering', 'u_bob', 'Found the GitHub duplicate-task bug — webhook retries were not idempotent.', ago(7, 'h')),
  msg('c_engineering', 'u_bob', 'Fix is to dedupe on the issue node id before insert.', ago(7, 'h')),
  msg('c_engineering', 'u_alice', 'Nice. Can you add an activity entry when a duplicate is skipped?', ago(6, 'h'), {
    reactions: [{ emoji: '👍', userIds: ['u_bob'] }],
  }),
  msg(
    'c_engineering',
    'gh',
    'PR #191 merged: fix(github): dedupe issue webhooks by node id',
    ago(3, 'h'),
    { authorType: 'github', externalAuthor: { name: 'GitHub', source: 'team/workspace' } },
  ),
  msg('c_engineering', 'u_carol', 'The proxy appears to drop websocket upgrades when the config reloads — @alice anyone else seeing this?', ago(2, 'h')),
  msg('c_engineering', 'u_alice', 'Yes, that matches INF-101. My patch re-attaches listeners after reload.', ago(1, 'h')),

  // infra
  msg('c_infra', 'u_dan', 'Staging deploy at 11:40 went clean. Smoke checks green.', ago(8, 'h')),
  msg('c_infra', 'u_alice', 'Proxy patch is on staging now. Watching error rates for an hour before promoting.', ago(3, 'h')),
  msg('c_infra', 'u_dan', '502 rate on staging dropped to zero since the patch 📉', ago(30, 'm'), {
    reactions: [{ emoji: '🚀', userIds: ['u_alice', 'u_bob', 'u_carol'] }],
  }),

  // design
  msg('c_design', 'u_eve', 'Uploaded the dark-theme token pass — accent switches to yellow like Coolify does.', ago(2, 'd')),
  msg('c_design', 'u_carol', 'Looks great in the sidebar. The purple gradient stays for primary buttons, right?', ago(2, 'd')),
  msg('c_design', 'u_eve', 'Exactly — gradient primary in both themes, accent only for selection and focus.', ago(2, 'd')),

  // support (with Discord-forwarded messages)
  msg(
    'c_support',
    'dc',
    'The deployment gives me a 502 after tonight\'s update. Anyone around?',
    ago(5, 'h'),
    { authorType: 'discord', externalAuthor: { name: 'JordanR', source: '#support' } },
  ),
  msg('c_support', 'u_alice', 'That is the proxy issue — replied on the mail thread and posted a status update.', ago(4, 'h')),
  msg(
    'c_support',
    'dc',
    'Thanks! Subscribed to the status page.',
    ago(4, 'h'),
    { authorType: 'discord', externalAuthor: { name: 'JordanR', source: '#support' } },
  ),
]
