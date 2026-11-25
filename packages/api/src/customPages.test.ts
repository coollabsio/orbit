import { expect, test } from 'bun:test'
import { customPageUrl, isSlug, slugOf } from './customPages'

test('a path and query after the page address go to the page URL, never to another origin', () => {
  const base = 'https://grafana.example.com/d/abc?orgId=1'
  expect(customPageUrl(base)).toBe(base)
  expect(customPageUrl(base, 'panel/2', '?from=now-1h')).toBe('https://grafana.example.com/d/abc/panel/2?orgId=1&from=now-1h')
  expect(customPageUrl('https://grafana.example.com/', 'x')).toBe('https://grafana.example.com/x')
  expect(customPageUrl(base, '//evil.test/x')).toBe('https://grafana.example.com/d/abc/evil.test/x?orgId=1')
  expect(customPageUrl(base, 'https://evil.test')?.startsWith('https://grafana.example.com/')).toBeTrue()
  expect(customPageUrl(base, '../../..')?.startsWith('https://grafana.example.com/')).toBeTrue()
  expect(customPageUrl('http://grafana.example.com/')).toBeNull()
  expect(customPageUrl('not a url')).toBeNull()
})

test('page addresses come from names', () => {
  expect(slugOf('Deploy board')).toBe('deploy-board')
  expect(slugOf('  KPIs — Q4 2026! ')).toBe('kpis-q4-2026')
  expect(slugOf('Café')).toBe('cafe')
  expect(slugOf('a'.repeat(47) + ' b')).toBe('a'.repeat(47))
  expect(isSlug('deploy-board')).toBeTrue()
  for (const bad of ['', 'Deploy', 'a--b', '-a', 'a-', 'a/b', 'a'.repeat(49)]) expect(isSlug(bad)).toBeFalse()
})
