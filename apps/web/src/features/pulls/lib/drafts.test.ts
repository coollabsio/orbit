import { afterEach, expect, test } from 'bun:test'
import { act, renderHook } from '@testing-library/react'
import { draftKey, replyDraftKey, useDraft } from './drafts'

afterEach(() => localStorage.clear())
const ref = { owner: 'acme', repo: 'repo', number: 12 }

test('a draft is keyed by its pull request and lines', () => {
  expect(draftKey('ws', ref, { path: 'a.ts', side: 'RIGHT', line: 9, start_side: 'LEFT', start_line: 3 })).toBe('orbit:pulls:draft:ws:acme/repo#12:a.ts:L3-R9')
  expect(draftKey('ws', ref, { path: 'a.ts', side: 'LEFT', line: 4 })).toBe('orbit:pulls:draft:ws:acme/repo#12:a.ts:L4')
  expect(replyDraftKey('ws', ref, 'T_1')).toBe('orbit:pulls:draft:ws:acme/repo#12:reply:T_1')
})

test('a draft survives a remount and goes when cleared', async () => {
  const first = renderHook(() => useDraft('k'))
  act(() => first.result.current[1]('Half a thought'))
  await new Promise((resolve) => setTimeout(resolve, 350))
  first.unmount()
  const second = renderHook(() => useDraft('k'))
  expect(second.result.current[0]).toBe('Half a thought')
  act(() => second.result.current[2]())
  expect(localStorage.getItem('k')).toBeNull()
})
