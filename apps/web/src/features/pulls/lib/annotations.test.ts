import { expect, test } from 'bun:test'
import type { GithubReviewThread } from '@orbit/api/generated/types.gen'
import { threadAnnotations } from './annotations'

const thread = (id: string, path: string, side: 'LEFT' | 'RIGHT', line: number | null): GithubReviewThread => ({
  id, path, side, line, start_line: null, start_side: null, is_resolved: false, is_outdated: line === null, can_resolve: true, can_reply: true, comments: [],
})

test('threads go on their side and line of their file', () => {
  const threads = [thread('a', 'x.ts', 'LEFT', 3), thread('b', 'x.ts', 'RIGHT', 7), thread('c', 'y.ts', 'RIGHT', 1)]
  expect(threadAnnotations(threads, 'x.ts')).toEqual([
    { side: 'deletions', lineNumber: 3, metadata: threads[0] },
    { side: 'additions', lineNumber: 7, metadata: threads[1] },
  ])
})

test('outdated threads that are on no line are left out', () => {
  expect(threadAnnotations([thread('a', 'x.ts', 'RIGHT', null)], 'x.ts')).toEqual([])
})
