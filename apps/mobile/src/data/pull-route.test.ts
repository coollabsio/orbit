import { expect, test } from 'bun:test'

import { fileHref, pullRoute } from './pull-route'

test('pull request routes live outside the tabs, so they open over the current tab', () => {
  expect(pullRoute({ owner: 'acme', repo: 'repo', number: 12 })).toBe('/pulls/acme/repo/12')
  expect(pullRoute({ owner: 'acme', repo: 'repo', number: 12 }, 5)).toBe('/pulls/acme/repo/12?comment=5')
})

test('a file route pushes inside the pull request modal and encodes the path and thread', () => {
  expect(fileHref({ repository: 'acme/repo', number: 12 }, 'src/a b.ts')).toBe('/pulls/acme/repo/12/file?path=src%2Fa%20b.ts')
  expect(fileHref({ repository: 'acme/repo', number: 12 }, 'x.ts', 'T_1')).toBe('/pulls/acme/repo/12/file?path=x.ts&thread=T_1')
})
