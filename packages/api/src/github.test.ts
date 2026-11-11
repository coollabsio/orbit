import { expect, test } from 'bun:test'

import { parseGithubPullUrl, pullPath, threadsOutsideDiff, toGitPatch } from './github'

test('reads GitHub pull request URLs', () => {
  expect(parseGithubPullUrl('https://github.com/acme/repo/pull/12')).toEqual({ owner: 'acme', repo: 'repo', number: 12 })
  expect(parseGithubPullUrl('https://github.com/acme/repo/pull/12/files')).toEqual({ owner: 'acme', repo: 'repo', number: 12 })
  expect(parseGithubPullUrl('https://github.com/acme/my.repo/pull/7#discussion_r1')).toEqual({ owner: 'acme', repo: 'my.repo', number: 7 })
  expect(parseGithubPullUrl('http://www.github.com/acme/repo/pull/3?x=1')).toEqual({ owner: 'acme', repo: 'repo', number: 3 })
})

test('refuses everything that is not a GitHub pull request', () => {
  for (const url of [
    'https://github.com/acme/repo/issues/12',
    'https://gist.github.com/acme/abc',
    'https://github.com.evil.io/acme/repo/pull/1',
    'https://evil.io/github.com/acme/repo/pull/1',
    'https://github.com/acme/repo/pull/0',
    'https://github.com/acme/repo/pull/abc',
    'https://github.com/../repo/pull/1',
    'not a url',
  ]) expect(parseGithubPullUrl(url)).toBeNull()
})

test('builds the Orbit path of a pull request', () => {
  expect(pullPath({ owner: 'acme', repo: 'repo', number: 12 })).toBe('/tasks/pulls/acme/repo/12')
  expect(pullPath({ owner: 'acme', repo: 'repo', number: 12 }, 55)).toBe('/tasks/pulls/acme/repo/12?comment=55')
})

test('adds git headers to GitHub patches', () => {
  const hunk = '@@ -1 +1,2 @@\n a\n+b'
  expect(toGitPatch({ path: 'src/a.rs', status: 'modified', patch: hunk })).toBe(`diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n${hunk}\n`)
  expect(toGitPatch({ path: 'new.rs', status: 'added', patch: '@@ -0,0 +1 @@\n+x' })).toBe('diff --git a/new.rs b/new.rs\nnew file mode 100644\n--- /dev/null\n+++ b/new.rs\n@@ -0,0 +1 @@\n+x\n')
  expect(toGitPatch({ path: 'gone.rs', status: 'removed', patch: '@@ -1 +0,0 @@\n-x' })).toBe('diff --git a/gone.rs b/gone.rs\ndeleted file mode 100644\n--- a/gone.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n')
  expect(toGitPatch({ path: 'b.rs', previous_path: 'a.rs', status: 'renamed', patch: hunk })).toBe(`diff --git a/a.rs b/b.rs\nrename from a.rs\nrename to b.rs\n--- a/a.rs\n+++ b/b.rs\n${hunk}\n`)
  expect(toGitPatch({ path: 'big.lock', status: 'modified', patch: null })).toBeNull()
})

test('threads the diff cannot show: outdated, file-level, or on a file without a diff', () => {
  const files = [{ path: 'a.ts', patch: '@@' }, { path: 'yarn.lock', patch: null }]
  const threads = [{ id: 1, path: 'a.ts', line: 3 }, { id: 2, path: 'a.ts', line: null }, { id: 3, path: 'yarn.lock', line: 9 }, { id: 4, path: 'gone.ts', line: 1 }]
  expect(threadsOutsideDiff(threads, files).map((thread) => thread.id)).toEqual([2, 3, 4])
})
