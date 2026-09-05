import { expect, test } from 'bun:test'
import { consumeQueryToken } from './authFlow'

test('setup and recovery tokens are consumed from the URL without dropping other navigation state', () => {
  let replacement = ''

  const token = consumeQueryToken(
    'https://orbit.test/setup?token=single-use&workspace=work-1#details',
    (href) => { replacement = href },
  )

  expect(token).toBe('single-use')
  expect(replacement).toBe('/setup?workspace=work-1#details')
})

test('a missing token leaves browser history untouched', () => {
  let replaced = false

  expect(consumeQueryToken('https://orbit.test/login', () => { replaced = true })).toBeNull()
  expect(replaced).toBeFalse()
})
