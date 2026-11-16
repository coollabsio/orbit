import { expect, test } from 'bun:test'
import {
  encodedSize,
  isValidAddress,
  likelyLimit,
  linkRefusal,
  preSendWarnings,
  recipientSummary,
  sendLaterPresets,
  signatureStart,
  splitRecipients,
  suggestLinks,
  type PreSendInput,
} from './composeLib'

const base: PreSendInput = {
  text: 'Hello, see you tomorrow.',
  subject: 'Plans',
  fileCount: 0,
  mode: 'new',
  to: ['ann@acme.io'],
  cc: [],
  bcc: [],
  threadAddresses: null,
  workspaceDomains: ['acme.io'],
  suppressed: [],
}

const kinds = (input: Partial<PreSendInput>) => preSendWarnings({ ...base, ...input }).map((warning) => warning.kind)

test('no warnings for a plain message', () => {
  expect(kinds({})).toEqual([])
})

test('missing_attachment: the text says attached or attachment and there is no file', () => {
  for (const text of ['I attached the plan.', 'See the attachment', 'Two attachments here', 'Please ATTACH it']) {
    expect(kinds({ text })).toEqual(['missing_attachment'])
  }
  // with a file, or a word that only contains it
  expect(kinds({ text: 'I attached the plan.', fileCount: 1 })).toEqual([])
  expect(kinds({ text: 'The attachedness of it' })).toEqual([])
  // a forward as attachment carries the original
  expect(kinds({ text: 'Attached is the mail.', mode: 'forward_attached' })).toEqual([])
})

test('empty_subject: a blank subject', () => {
  expect(kinds({ subject: '   ' })).toEqual(['empty_subject'])
})

test('placeholder: a {placeholder} left from a snippet, in the text or the subject', () => {
  const [warning] = preSendWarnings({ ...base, text: 'Hi {first name}, thanks.' })
  expect(warning?.kind).toBe('placeholder')
  expect(warning?.message).toContain('{first name}')
  expect(kinds({ subject: 'Offer for {company}' })).toEqual(['placeholder'])
  expect(kinds({ text: 'Braces {} alone are fine' })).toEqual([])
})

test('new_outside_recipient: an outside address joins a thread that had only workspace addresses', () => {
  const thread = { mode: 'reply_all', threadAddresses: ['ann@acme.io', 'bob@acme.io'] }
  const [warning] = preSendWarnings({ ...base, ...thread, to: ['ann@acme.io', 'eve@other.com'] })
  expect(warning?.kind).toBe('new_outside_recipient')
  expect(warning?.addresses).toEqual(['eve@other.com'])
  // case does not matter; workspace addresses are not outside
  expect(kinds({ ...thread, to: ['ANN@acme.io', 'carl@Acme.io'] })).toEqual([])
  // the thread had an outside address already
  expect(kinds({ ...thread, threadAddresses: ['ann@acme.io', 'eve@other.com'], to: ['eve@other.com', 'zed@else.org'] })).toEqual([])
  // a new message has no thread
  expect(kinds({ to: ['eve@other.com'] })).toEqual([])
})

test('reply_all_over_10: reply all to more than 10 people', () => {
  const people = (count: number) => Array.from({ length: count }, (_, index) => `p${index}@acme.io`)
  expect(kinds({ mode: 'reply_all', to: people(6), cc: people(11).slice(6) })).toEqual(['reply_all_over_10'])
  expect(kinds({ mode: 'reply_all', to: people(10) })).toEqual([])
  // the same person twice counts once
  expect(kinds({ mode: 'reply_all', to: people(10), cc: ['P0@acme.io'] })).toEqual([])
  expect(kinds({ mode: 'new', to: people(11) })).toEqual([])
})

test('suppressed: a recipient on the suppression list', () => {
  const [warning] = preSendWarnings({ ...base, to: ['ann@acme.io'], bcc: ['Gone@Old.com'], suppressed: ['gone@old.com'] })
  expect(warning?.kind).toBe('suppressed')
  expect(warning?.addresses).toEqual(['Gone@Old.com'])
})

test('encoded_size_counts_base64', () => {
  // 3,000 bytes → 4,000 base64 → 4,080 with 2 % line breaks; a link file adds nothing
  expect(encodedSize([{ size: 3000, asLink: false }, { size: 9_000_000, asLink: true }], 1000)).toBe(5080)
  // base64 pads to whole groups of 3
  expect(encodedSize([{ size: 1, asLink: false }], 0)).toBe(Math.ceil(4 * 1.02))
  expect(encodedSize([], 250)).toBe(250)
})

test('suggest_links_largest_first', () => {
  const MB = 1_000_000
  const files = [
    { id: 'a', size: 4 * MB, asLink: false, linkable: true },
    { id: 'b', size: 8 * MB, asLink: false, linkable: true },
    { id: 'c', size: 1 * MB, asLink: false, linkable: true },
    // an inline image or a forwarded file cannot be a link
    { id: 'd', size: 6 * MB, asLink: false, linkable: false },
  ]
  expect(suggestLinks(files, 10 * MB)).toEqual(['b', 'a'])
  // a file that is a link already is not suggested, and counts as such
  expect(suggestLinks(files.map((file) => (file.id === 'b' ? { ...file, asLink: true } : file)), 10 * MB)).toEqual(['a'])
  // fits already
  expect(suggestLinks(files, 100 * MB)).toEqual([])
  // the body counts too
  expect(suggestLinks([{ id: 'x', size: 100, asLink: false, linkable: true }], 1000, 990)).toEqual(['x'])
})

test('forwarded files and images in the text cannot be links', () => {
  expect(linkRefusal({ source_attachment_id: 'att1', content_id: null })).toContain('forwarded')
  expect(linkRefusal({ content_id: 'img1@orbit' })).toContain('image')
  expect(linkRefusal({ source_attachment_id: null, content_id: '  ' })).toBeNull()
})

test('the limit a refused message was over is the largest route limit below its size', () => {
  expect(likelyLimit(32_000_000)).toBe(26_214_400)
  expect(likelyLimit(26_000_000)).toBe(25_000_000)
  expect(likelyLimit(12_000_000)).toBe(10_000_000)
  // below every limit: the smallest one
  expect(likelyLimit(5_000_000)).toBe(10_000_000)
})

test('recipient_summary', () => {
  expect(recipientSummary([{ address: 'andras@coollabs.io', name: 'Andras Bacsai' }], ['b@x.io'], ['c@x.io'])).toBe('To Andras +2')
  expect(recipientSummary(['ann@acme.io'], [], [])).toBe('To ann@acme.io')
  expect(recipientSummary([], ['b@x.io', 'c@x.io'], [])).toBe('Cc b@x.io +1')
  expect(recipientSummary([], [], [{ address: 'c@x.io', name: '  ' }])).toBe('Bcc c@x.io')
  expect(recipientSummary([], [], [])).toBe('No recipients')
})

test('send_later_presets_sunday_and_monday', () => {
  const tz = 'Europe/Budapest' // UTC+1 in November
  // Sunday 2026-11-15 20:00 local
  const sunday = sendLaterPresets(new Date('2026-11-15T19:00:00Z'), tz)
  expect(sunday.map((preset) => preset.label)).toEqual(['Tomorrow 8:00', 'Monday 8:00'])
  expect(sunday.map((preset) => preset.at.toISOString())).toEqual(['2026-11-16T07:00:00.000Z', '2026-11-16T07:00:00.000Z'])
  // Monday 2026-11-16 09:00 local: Monday is in 7 days
  const monday = sendLaterPresets(new Date('2026-11-16T08:00:00Z'), tz)
  expect(monday.map((preset) => preset.at.toISOString())).toEqual(['2026-11-17T07:00:00.000Z', '2026-11-23T07:00:00.000Z'])
  // Monday 01:00 local (still Sunday in UTC): the local day decides
  const early = sendLaterPresets(new Date('2026-11-16T00:00:00Z'), tz)
  expect(early.map((preset) => preset.at.toISOString())).toEqual(['2026-11-17T07:00:00.000Z', '2026-11-23T07:00:00.000Z'])
  // across a daylight saving change: Saturday 2026-03-07 in New York, clocks go forward on Sunday
  const dst = sendLaterPresets(new Date('2026-03-07T15:00:00Z'), 'America/New_York')
  expect(dst.map((preset) => preset.at.toISOString())).toEqual(['2026-03-08T12:00:00.000Z', '2026-03-09T12:00:00.000Z'])
})

test('pasted recipients split into addresses, names and brackets dropped', () => {
  expect(splitRecipients('Andras <andras@coollabs.io>, "Bacsai, B" <b@x.io>; c@x.io\nd@x.io e@x.io')).toEqual([
    'andras@coollabs.io',
    'b@x.io',
    'c@x.io',
    'd@x.io',
    'e@x.io',
  ])
  expect(splitRecipients('  not an address  ')).toEqual(['not an address'])
  expect(splitRecipients(' , ;')).toEqual([])
})

test('an address is valid when the server would take it', () => {
  for (const address of ['ann@acme.io', 'a.b+tag@sub.example.co.uk']) expect(isValidAddress(address)).toBe(true)
  for (const address of ['ann', 'ann@', '@acme.io', 'a@b@c.io', 'ann@acme', 'a b@acme.io', '"a"@acme.io', 'a%b@acme.io']) {
    expect(isValidAddress(address)).toBe(false)
  }
})

test('the signature is found at the end of the text, empty lines after it ignored', () => {
  const signature = ['Ann', 'Acme  Inc']
  expect(signatureStart(['Hi', '', 'Ann', 'Acme Inc', ''], signature)).toBe(2)
  expect(signatureStart(['Hi', 'Ann', 'Acme Inc changed'], signature)).toBeNull()
  expect(signatureStart(['Hi'], [])).toBeNull()
})
