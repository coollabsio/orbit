import { expect, test } from 'bun:test'

import { mergeDisabledReason, mergeMethodKey, parseGithubPullUrl, pickMergeMethod, pullPath, squashDefaults, threadsOutsideDiff, toCommentTarget, toGitPatch } from './github'

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

test('a Pierre line range becomes GitHub comment coordinates', () => {
  // one line on the old side
  expect(toCommentTarget('a.ts', { start: 4, end: 4, side: 'deletions' })).toEqual({ path: 'a.ts', side: 'LEFT', line: 4 })
  // a range dragged upwards on the new side
  expect(toCommentTarget('a.ts', { start: 9, end: 5, side: 'additions' })).toEqual({ path: 'a.ts', side: 'RIGHT', line: 9, start_side: 'RIGHT', start_line: 5 })
  // a range from a deleted line to an added line keeps both sides
  expect(toCommentTarget('a.ts', { start: 3, end: 6, side: 'deletions', endSide: 'additions' })).toEqual({ path: 'a.ts', side: 'RIGHT', line: 6, start_side: 'LEFT', start_line: 3 })
})

const merge = { methods: ['merge', 'squash'], mergeable: 'mergeable', blocked_reason: null, default_method: 'squash' }
const pull = { title: 'Fix it', number: 12, body: 'Body', state: 'open', merge, viewer: { can_comment: true, can_review: true, can_merge: true, is_author: false } }

test('the merge method: the stored one if allowed, else the default, else the first', () => {
  expect(pickMergeMethod(merge, 'merge')).toBe('merge')
  expect(pickMergeMethod(merge, 'rebase')).toBe('squash')
  expect(pickMergeMethod({ ...merge, default_method: 'rebase' }, null)).toBe('merge')
  expect(pickMergeMethod({ ...merge, methods: [] }, 'merge')).toBeNull()
  expect(mergeMethodKey({ owner: 'acme', repo: 'repo', number: 1 })).toBe('orbit:pulls:merge-method:acme/repo')
})

test('why merging is not possible, in the order a person fixes it', () => {
  expect(mergeDisabledReason(pull)).toBeNull()
  expect(mergeDisabledReason({ ...pull, viewer: { ...pull.viewer, can_merge: false } })).toBe("You can't merge this pull request on GitHub.")
  expect(mergeDisabledReason({ ...pull, merge: { ...merge, blocked_reason: 'Branch protection blocks merging.' } })).toBe('Branch protection blocks merging.')
  expect(mergeDisabledReason({ ...pull, merge: { ...merge, mergeable: 'conflicting' } })).toBe('This branch has conflicts.')
  expect(mergeDisabledReason({ ...pull, merge: { ...merge, methods: [] } })).toBe('This repository allows no merge method.')
})

test('a squash commit starts as the title with the number, and the description', () => {
  expect(squashDefaults(pull)).toEqual({ title: 'Fix it (#12)', message: 'Body' })
})
