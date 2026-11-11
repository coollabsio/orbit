import { expect, test } from 'bun:test'

import { githubConnectResult } from './github-api'
import { pushRoute } from './push-route'

test('reads the outcome GitHub sends the app back with', () => {
  expect(githubConnectResult('orbit://auth/callback?github=taken')).toBe('taken')
  expect(githubConnectResult('orbit://auth/callback?github=connected')).toBe('connected')
  expect(githubConnectResult('exp://127.0.0.1:8081/--/auth/callback?x=1&github=failed')).toBe('failed')
  expect(githubConnectResult('orbit://auth/callback?github=other')).toBeNull()
  expect(githubConnectResult('orbit://auth/callback')).toBeNull()
})

test('the web GitHub account page opens the app GitHub account screen', () => {
  expect(pushRoute('/profile/github')).toBe('/settings/github-account')
  expect(pushRoute('/settings/github')).toBe('/settings')
})
