import { expect, test } from 'bun:test'

import { mailServerMissing } from './mail-server'

test('only an installation admin on Orbit Cloud without a mail server is warned', () => {
  expect(mailServerMissing({ billing_enabled: true, email_enabled: false }, true)).toBe(true)
  expect(mailServerMissing({ billing_enabled: true, email_enabled: false }, false)).toBe(false)
  expect(mailServerMissing({ billing_enabled: false, email_enabled: false }, true)).toBe(false)
  expect(mailServerMissing({ billing_enabled: true, email_enabled: true }, true)).toBe(false)
  expect(mailServerMissing(undefined, true)).toBe(false)
})
