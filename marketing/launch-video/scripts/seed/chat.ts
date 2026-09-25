// Marketing seed (swapped in for apps/web/src/mock/seed/chat.ts by vite.capture.config.ts).
// A small team building Orbit: channels, a lively #engineering with a code block, GitHub chips, reactions,
// a thread, an image attachment in #design, release embeds, and a few DMs.
import { ago } from '@/lib/format'
import type { Attachment, Channel, ChatCategory, ChatMessage, DirectMessage } from '@/mock/types'

export const chatCategories: ChatCategory[] = [
  { id: 'cc_general', name: 'Company' },
  { id: 'cc_engineering', name: 'Product' },
  { id: 'cc_support', name: 'Customers' },
]

export const channels: Channel[] = [
  { id: 'c_general', name: 'general', description: 'Team-wide announcements and chatter', categoryId: 'cc_general', unreadCount: 0 },
  { id: 'c_random', name: 'random', description: 'Coffee, links and cat pictures', categoryId: 'cc_general', unreadCount: 4 },
  { id: 'c_engineering', name: 'engineering', description: 'Backend, frontend, and everything between', categoryId: 'cc_engineering', unreadCount: 0 },
  { id: 'c_design', name: 'design', description: 'UI, UX and visual direction', categoryId: 'cc_engineering', unreadCount: 2 },
  { id: 'c_releases', name: 'releases', description: 'Release notes and deploys', categoryId: 'cc_engineering', unreadCount: 1 },
  { id: 'c_infra', name: 'infra', description: 'Servers, deploys and incidents', categoryId: 'cc_engineering', unreadCount: 0 },
  { id: 'c_support', name: 'support', description: 'Forwarded customer conversations', categoryId: 'cc_support', unreadCount: 3 },
  { id: 'c_feedback', name: 'feedback', description: 'Feature requests from users', categoryId: 'cc_support', unreadCount: 0 },
]

export const directMessages: DirectMessage[] = [
  { id: 'dm_andras', participantId: 'u_andras', unreadCount: 0 },
  { id: 'dm_peak', participantId: 'u_peak', unreadCount: 2 },
  { id: 'dm_cinzya', participantId: 'u_cinzya', unreadCount: 0 },
  { id: 'dm_priya', participantId: 'u_adiology', unreadCount: 0 },
  { id: 'dm_noah', participantId: 'u_noah', unreadCount: 0 },
]

let m = 0

function dataUrl(svg: string): string {
  return `data:image/svg+xml;base64,${btoa(unescape(encodeURIComponent(svg)))}`
}

/** A small UI mock (presence avatars on a doc) drawn as SVG, so the attachment works offline. */
function presenceMock(): Attachment {
  const avatar = (x: number, color: string, letter: string) =>
    `<circle cx='${x}' cy='86' r='22' fill='${color}' stroke='#17171c' stroke-width='5'/><text x='${x}' y='94' text-anchor='middle' font-family='Inter,Arial,sans-serif' font-size='21' font-weight='700' fill='#fff'>${letter}</text>`
  const line = (y: number, w: number, o = 0.16) => `<rect x='96' y='${y}' width='${w}' height='14' rx='7' fill='rgba(255,255,255,${o})'/>`
  const svg = `<svg xmlns='http://www.w3.org/2000/svg' width='960' height='540' viewBox='0 0 960 540'>
<defs><linearGradient id='bg' x1='0' y1='0' x2='1' y2='1'><stop offset='0' stop-color='#2a1b5e'/><stop offset='1' stop-color='#0b3b5c'/></linearGradient></defs>
<rect width='960' height='540' fill='url(#bg)'/>
<rect x='56' y='40' width='848' height='460' rx='22' fill='#17171c' stroke='rgba(255,255,255,0.08)'/>
<rect x='56' y='40' width='848' height='92' rx='22' fill='#1d1d24'/><rect x='56' y='110' width='848' height='22' fill='#1d1d24'/>
${avatar(716, '#8b5cf6', 'M')}${avatar(752, '#0ea5e9', 'L')}${avatar(788, '#f59e0b', 'P')}${avatar(824, '#10b981', 'T')}
<text x='96' y='94' font-family='Inter,Arial,sans-serif' font-size='26' font-weight='700' fill='#f4f4f5'>Q4 roadmap</text>
<text x='270' y='94' font-family='Inter,Arial,sans-serif' font-size='18' fill='#a1a1aa'>4 people editing</text>
${line(176, 520, 0.22)}${line(206, 610)}${line(236, 450)}
<rect x='96' y='286' width='3' height='26' fill='#0ea5e9'/><rect x='99' y='286' width='64' height='22' rx='4' fill='#0ea5e9'/>
<text x='131' y='302' text-anchor='middle' font-family='Inter,Arial,sans-serif' font-size='14' font-weight='700' fill='#fff'>Leo</text>
${line(326, 580)}${line(356, 380)}
<rect x='96' y='404' width='3' height='26' fill='#f59e0b'/><rect x='99' y='404' width='70' height='22' rx='4' fill='#f59e0b'/>
<text x='134' y='420' text-anchor='middle' font-family='Inter,Arial,sans-serif' font-size='14' font-weight='700' fill='#fff'>Priya</text>
${line(444, 540)}
</svg>`
  return { id: 'att_presence', fileName: 'presence-avatars-v3.png', mimeType: 'image/svg+xml', fileSize: 184_320, url: dataUrl(svg) }
}

function msg(channelId: string, authorId: string, content: string, createdAt: string, extra?: Partial<ChatMessage>): ChatMessage {
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

const github = { authorType: 'github' as const, externalAuthor: { name: 'GitHub', source: 'orbit-hq/orbit' } }

export const chatMessages: ChatMessage[] = [
  // ---------- DMs ----------
  msg('dm_andras', 'u_andras', 'Got a sec? I want to sanity-check the release notes for 0.9 before I post them.', ago(58, 'm')),
  msg('dm_andras', 'u_shadow', 'Sure, send them over.', ago(55, 'm')),
  msg('dm_andras', 'u_andras', 'Draft is in Docs → Releases → 0.9.0. Big items: realtime docs, Notion import, My week view.', ago(54, 'm')),
  msg('dm_andras', 'u_andras', 'Also added the migration note for self-hosters:\n```bash\norbit migrate --to 0.9.0 && systemctl restart orbit\n```', ago(53, 'm')),
  msg('dm_andras', 'u_shadow', 'Reads well. I would lead with Notion import, that is what customers keep asking about.', ago(41, 'm'), {
    reactions: [{ emoji: '👍', userIds: ['u_andras'] }],
  }),
  msg('dm_andras', 'u_andras', 'Good call, reordered. Tracking the last blocker here: https://github.com/orbit-hq/orbit/issues/418', ago(37, 'm')),
  msg('dm_andras', 'u_shadow', 'Perfect. Ship it tomorrow morning? 🚢', ago(12, 'm')),
  msg('dm_andras', 'u_andras', 'Tomorrow 9:00. I will cut the RC tonight.', ago(9, 'm'), {
    reactions: [{ emoji: '🎉', userIds: ['u_shadow'] }],
  }),
  msg('dm_peak', 'u_peak', 'New empty states are in Figma, would love your eyes on the Mail one.', ago(2, 'h')),
  msg('dm_peak', 'u_peak', 'Also: can we move the design review to 15:00?', ago(20, 'm')),
  msg('dm_cinzya', 'u_cinzya', 'Backups for the EU region are verified, restore took 4m 12s ✅', ago(1, 'd')),
  msg('dm_priya', 'u_adiology', 'Pushed the search index fix, reindex runs tonight.', ago(3, 'h')),
  msg('dm_noah', 'u_noah', 'Keyboard shortcuts sheet is ready for review.', ago(2, 'd')),

  // ---------- #general ----------
  msg('c_general', 'u_shadow', 'Morning team ☀️ Reminder: all-hands moved to Thursday 16:00 so we can demo 0.9.', ago(26, 'h')),
  msg('c_general', 'u_noah', 'Works for me 👍', ago(25, 'h')),
  msg('c_general', 'u_cinzya', 'Heads up: rotating TLS certificates on Thursday evening, expect a short blip on internal tools.', ago(5, 'h'), {
    pinned: true,
    pinnedAt: ago(4, 'h'),
    pinnedBy: 'u_shadow',
  }),
  msg('c_general', 'u_peak', 'New office plants have arrived 🌿 please water responsibly', ago(3, 'h'), {
    reactions: [{ emoji: '🌱', userIds: ['u_shadow', 'u_noah', 'u_adiology'] }],
  }),

  // ---------- #engineering ----------
  msg('c_engineering', 'u_andras', 'Morning! Realtime sync for docs is merged behind the `docs_realtime` flag. Could use a few eyes on it today 🙏', ago(96, 'm')),
  msg(
    'c_engineering',
    'u_andras',
    [
      'This is the check we run before applying a patch from another client:',
      '```rust',
      'fn can_apply(doc: &Doc, p: &Patch) -> Result<()> {',
      '    if p.base_version != doc.version {',
      '        return Err(SyncError::Stale(doc.version));',
      '    }',
      '    Ok(())',
      '}',
      '```',
    ].join('\n'),
    ago(95, 'm'),
    { id: 'cm_code', reactions: [{ emoji: '👀', userIds: ['u_adiology', 'u_noah'] }] },
  ),
  msg('c_engineering', 'u_adiology', '@leo looks clean. Should stale text patches rebase instead of erroring? Left a note on https://github.com/orbit-hq/orbit/pull/412', ago(82, 'm')),
  msg('c_engineering', 'u_shadow', 'Good question. Opening a thread so we keep #engineering readable.', ago(78, 'm'), {
    id: 'cm_thread_root',
    threadTitle: 'Rebase stale text patches?',
    threadFollowed: true,
  }),
  msg('c_engineering', 'gh', 'PR #412 merged into main: feat(docs): realtime sync with version checks', ago(44, 'm'), github),
  msg('c_engineering', 'u_cinzya', 'Deployed to staging. p95 for page saves went from 180 ms to 42 ms 📉', ago(31, 'm'), {
    id: 'cm_reactions',
    reactions: [
      { emoji: '🚀', userIds: ['u_shadow', 'u_andras', 'u_adiology', 'u_noah'] },
      { emoji: '🔥', userIds: ['u_andras', 'u_peak'] },
      { emoji: '🎉', userIds: ['u_shadow', 'u_adiology', 'u_peak'] },
    ],
  }),
  msg('c_engineering', 'u_noah', 'Huge. The editor finally feels instant on my slow laptop.', ago(29, 'm')),
  msg('c_engineering', 'u_peak', '@maya presence avatars are ready for review, see #design when you have a minute ✨', ago(14, 'm')),
  msg('c_engineering', 'u_shadow', 'Great work everyone. Let\'s cut 0.9.0-rc.1 tomorrow morning :shipit:', ago(6, 'm'), {
    reactions: [{ emoji: '🙌', userIds: ['u_andras', 'u_cinzya'] }],
  }),

  // ---------- #design ----------
  msg('c_design', 'u_peak', 'Presence avatars v3: stacked in the header, colored cursors in the page 👇', ago(16, 'm'), {
    attachments: [presenceMock()],
    reactions: [{ emoji: '😍', userIds: ['u_shadow', 'u_andras'] }],
  }),
  msg('c_design', 'u_noah', 'Love the cursor labels. Should we cap the stack at 4 and show +N?', ago(12, 'm')),
  msg('c_design', 'u_peak', 'Yes, +N with a tooltip listing everyone.', ago(10, 'm')),

  // ---------- #infra ----------
  msg('c_infra', 'u_cinzya', 'EU backups verified, restore drill took 4m 12s.', ago(1, 'd')),
  msg('c_infra', 'wh_deploy', '', ago(35, 'm'), {
    authorType: 'webhook',
    webhookName: 'Deploy Bot',
    webhookIconUrl: null,
    embeds: [
      {
        title: ':white_check_mark: Deploy succeeded: staging',
        description: 'Build **#1287** is live on staging.orbit.dev.',
        color: 0x1f8b4c,
        fields: [
          { name: 'Environment', value: 'Staging', inline: true },
          { name: 'Duration', value: '1m 48s', inline: true },
          { name: 'Commit', value: '`a3f9c21`', inline: true },
        ],
        footer: { text: 'Orbit CI' },
        timestamp: ago(35, 'm'),
      },
    ],
  }),

  // ---------- #releases ----------
  msg('c_releases', 'wh_releases', '', ago(3, 'd'), {
    authorType: 'webhook',
    webhookName: 'Release Notes',
    webhookIconUrl: null,
    embeds: [
      {
        title: 'Orbit 0.8.2',
        url: 'https://github.com/orbit-hq/orbit/releases/tag/v0.8.2',
        description: '- Bulk actions for tasks\n- Roadmap timeline with drag to schedule\n- Faster command palette search',
        color: 0x7c3aed,
        footer: { text: 'github.com/orbit-hq/orbit' },
        timestamp: ago(3, 'd'),
      },
    ],
  }),
  msg('c_releases', 'u_andras', '0.9.0-rc.1 goes out tomorrow 9:00. Release notes draft is in Docs.', ago(20, 'm')),

  // ---------- #support ----------
  msg('c_support', 'dc', 'Is there a way to import our Notion wiki? We have ~300 pages.', ago(5, 'h'), {
    authorType: 'discord',
    externalAuthor: { name: 'hannah.w', source: '#help' },
  }),
  msg('c_support', 'u_shadow', 'Yes! Notion import ships in 0.9, pages, images and files included. I will ping you when it is live.', ago(4, 'h')),
]

// the thread under Maya's message in #engineering
chatMessages.push(
  msg('c_engineering', 'u_andras', 'Rebase for text blocks, keep the error for structural moves (reorder, nesting). Those are too easy to get wrong.', ago(74, 'm'), {
    threadRootId: 'cm_thread_root',
  }),
  msg('c_engineering', 'u_adiology', 'Agreed. The client already has the base version, so a text rebase is cheap:', ago(70, 'm'), {
    threadRootId: 'cm_thread_root',
  }),
  msg('c_engineering', 'u_adiology', '```ts\nconst next = rebase(patch, ops.since(base))\n```', ago(70, 'm'), {
    threadRootId: 'cm_thread_root',
  }),
  msg('c_engineering', 'u_cinzya', 'Can we log every rebase for the first week? I want to see how often it happens in practice.', ago(61, 'm'), {
    threadRootId: 'cm_thread_root',
    reactions: [{ emoji: '👍', userIds: ['u_andras', 'u_shadow'] }],
  }),
  msg('c_engineering', 'u_shadow', 'Ship it with logging, then decide on structural moves after the data. @leo can you add it to #412?', ago(52, 'm'), {
    threadRootId: 'cm_thread_root',
  }),
  msg('c_engineering', 'u_andras', 'Done, pushed to the PR ✅', ago(47, 'm'), {
    threadRootId: 'cm_thread_root',
    reactions: [{ emoji: '🙌', userIds: ['u_shadow', 'u_adiology'] }],
  }),
)
