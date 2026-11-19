import { expect, test } from 'bun:test'

import { finishResult, githubReturn } from './github-api'
import { pushRoute } from './push-route'

test('reads where GitHub sent the app back', () => {
  expect(githubReturn('orbit://auth/callback?github=finish&token=t1')).toEqual({ kind: 'finish', token: 't1' })
  expect(githubReturn('exp://127.0.0.1:8081/--/auth/callback?x=1&github=finish&token=a%2Bb')).toEqual({ kind: 'finish', token: 'a+b' })
  expect(githubReturn('orbit://auth/callback?github=failed')).toEqual({ kind: 'failed' })
  expect(githubReturn('orbit://auth/callback?github=finish')).toEqual({ kind: 'failed' })
  expect(githubReturn('orbit://auth/callback')).toBeNull()
})

test('maps the finish problems to outcomes', () => {
  expect(finishResult('github_account_taken')).toBe('taken')
  expect(finishResult('github_connect_mismatch')).toBe('mismatch')
  expect(finishResult('github_connect_failed')).toBe('failed')
  expect(finishResult(undefined)).toBe('failed')
})

test('the web GitHub account page opens the app GitHub account screen', () => {
  expect(pushRoute('/profile/github')).toBe('/settings/github-account')
  expect(pushRoute('/settings/github')).toBe('/settings')
})
