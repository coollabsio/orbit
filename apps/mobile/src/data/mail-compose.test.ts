import { expect, test } from 'bun:test'

import type { MailDraft, MailFromAddress } from '@orbit/api/generated/types.gen'

import { commitTyped, draftChange, htmlToText, initialFields, isUntouched, isValidAddress, preSendWarnings, recipientSummary, serverFields, splitRecipients, swapSignature, textToHtml } from './mail-compose'

const draft = (fields: Partial<MailDraft> = {}): MailDraft => ({
  id: 'd1',
  mailbox_id: 'm1',
  mode: 'new',
  from_address_id: 'a1',
  to: [],
  cc: [],
  bcc: [],
  subject: '',
  body_html: '',
  files: [],
  version: 1,
  created_at: '2026-11-20T10:00:00Z',
  updated_at: '2026-11-20T10:00:00Z',
  ...fields,
})
const address = (id: string, signature: string | null): MailFromAddress => ({ address_id: id, address: `${id}@orbit.test`, display_name: id, mailbox_id: 'm1', ready: 'ready', signature_html: signature })

test('plain text becomes paragraphs and line breaks, with HTML escaped', () => {
  expect(textToHtml('Hi Ana,\n\nThe <b>plan</b> & "dates":\nMonday\nTuesday\n\n\n-- \nBob')).toBe(
    '<p>Hi Ana,</p><p>The &lt;b&gt;plan&lt;/b&gt; &amp; &quot;dates&quot;:<br>Monday<br>Tuesday</p><p>-- <br>Bob</p>',
  )
  expect(textToHtml('')).toBe('')
  expect(textToHtml('\n\n  \n')).toBe('')
  expect(textToHtml('a\r\nb')).toBe('<p>a<br>b</p>')
})

test('mail HTML reads back as the text it was written from', () => {
  const text = 'Hi Ana,\n\nThe <b>plan</b> & "dates":\nMonday'
  expect(htmlToText(textToHtml(text))).toBe(text)
  expect(htmlToText('<div>One</div><div>Two<br/>three</div><ul><li>a</li><li>b</li></ul><p>&#169; &#x41; &nbsp;x</p>')).toBe('One\n\nTwo\nthree\n\n• a\n• b\n\n© A  x')
  expect(htmlToText('<style>p{color:red}</style><p>x</p>')).toBe('x')
})

test('recipients split like a paste and keep the address of Name <address>', () => {
  expect(splitRecipients('Ana <ana@x.test>, bob@y.test; "Lee, Kim" <lee@z.test>\ncat@w.test dan@v.test')).toEqual(['ana@x.test', 'bob@y.test', 'lee@z.test', 'cat@w.test', 'dan@v.test'])
  expect(commitTyped(['ana@x.test'], 'ANA@x.test, eve@x.test')).toEqual(['ana@x.test', 'eve@x.test'])
  expect(isValidAddress('ana@x.test')).toBe(true)
  expect(isValidAddress('ana@localhost')).toBe(false)
  expect(isValidAddress('a%b@x.test')).toBe(false)
  expect(isValidAddress('Ana <ana@x.test>')).toBe(false)
  expect(recipientSummary(['ana@x.test'], ['b@x.test'], [])).toBe('To ana@x.test +1')
  expect(recipientSummary([], [], [])).toBe('No recipients')
})

test('the first Send warns in the web order; suppressed recipients are named', () => {
  const base = { text: 'See the attached file', subject: '', fileCount: 0, mode: 'reply_all', to: [] as string[], cc: [] as string[], bcc: [] as string[], threadAddresses: ['ana@orbit.test'], workspaceDomains: ['orbit.test'], suppressed: [] }
  expect(preSendWarnings({ ...base, to: ['ana@orbit.test', 'out@else.test'], suppressed: [{ address: 'OUT@else.test', reason: 'hard_bounce', since: '2026-11-01T00:00:00Z' }] })).toEqual([
    'You mention an attachment, but no file is attached.',
    'The subject is empty.',
    'out@else.test is outside the workspace. This thread had only workspace addresses.',
    'out@else.test: the address does not take mail. Orbit will skip it.',
  ])
  const many = Array.from({ length: 11 }, (_, index) => `p${index}@orbit.test`)
  expect(preSendWarnings({ ...base, text: 'Hi {name}', subject: 'Plan', to: many })).toEqual(['{name} is still a placeholder.', 'Reply all goes to 11 people.'])
  expect(preSendWarnings({ ...base, text: 'Hi', subject: 'Plan', mode: 'new', threadAddresses: null, to: ['x@else.test'] })).toEqual([])
})

test('a new draft starts with the signature; a written one with its own text', () => {
  const addresses = [address('a1', '<p>Bob<br>Orbit</p>')]
  expect(initialFields(draft(), addresses).body).toBe('\n\nBob\nOrbit')
  expect(initialFields(draft({ body_html: '<p>Hello</p>' }), addresses).body).toBe('Hello')
  expect(serverFields(draft()).body).toBe('')
})

test('the signature follows the From address unless the user changed it', () => {
  const one = address('a1', '<p>Bob</p>')
  const two = address('a2', '<p>Support team</p>')
  expect(swapSignature('Hi\n\nBob', one, two)).toBe('Hi\n\nSupport team')
  expect(swapSignature('Hi\n\nBob, see you', one, two)).toBe('Hi\n\nBob, see you')
  expect(swapSignature('Hi\n\nBob', one, address('a3', null))).toBe('Hi')
  expect(swapSignature('', address('a3', null), two)).toBe('\n\nSupport team')
})

test('a save sends only what changed; an untouched new draft can go', () => {
  const opened = initialFields(draft(), [address('a1', '<p>Bob</p>')])
  const saved = serverFields(draft())
  expect(draftChange(saved, opened)).toEqual({ body_html: '<p>Bob</p>' })
  expect(draftChange(saved, { ...saved, to: ['ana@x.test'], subject: 'Hi' })).toEqual({ to: ['ana@x.test'], subject: 'Hi' })
  expect(isUntouched(draft(), opened, opened)).toBe(true)
  expect(isUntouched(draft(), opened, { ...opened, subject: 'Hi' })).toBe(false)
  expect(isUntouched(draft({ version: 2 }), opened, opened)).toBe(false)
})
