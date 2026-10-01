import { beforeEach, expect, test } from 'bun:test'
import {
  clearDraft,
  getDraft,
  getLastOpenedConversation,
  setDraft,
  setLastOpenedConversation,
  subscribeDrafts,
} from './drafts'

let workspace = ''
beforeEach(() => {
  // A fresh workspace id for each test: the module keeps a parsed copy for each workspace.
  workspace = `w-${crypto.randomUUID()}`
})

const stored = (workspaceId: string) => JSON.parse(localStorage.getItem(`orbit:chat:drafts:${workspaceId}`) ?? 'null')

test('a draft is kept for each conversation and each thread, in localStorage', () => {
  setDraft(workspace, 'c1', null, 'hello')
  setDraft(workspace, 'c1', 'm1', 'a reply')
  setDraft(workspace, 'c2', null, 'other')
  expect(getDraft(workspace, 'c1')).toBe('hello')
  expect(getDraft(workspace, 'c1', 'm1')).toBe('a reply')
  expect(getDraft(workspace, 'c2')).toBe('other')
  expect(getDraft(workspace, 'c3')).toBe('')
  expect(stored(workspace)).toEqual({ c1: 'hello', 'c1:m1': 'a reply', c2: 'other' })
})

test('drafts of one workspace do not show in another', () => {
  setDraft(workspace, 'c1', null, 'hello')
  expect(getDraft(`${workspace}-other`, 'c1')).toBe('')
})

test('empty or blank text removes the draft, and the last one removes the storage entry', () => {
  setDraft(workspace, 'c1', null, 'hello')
  setDraft(workspace, 'c1', 'm1', 'reply')
  setDraft(workspace, 'c1', null, '   \n')
  expect(getDraft(workspace, 'c1')).toBe('')
  expect(stored(workspace)).toEqual({ 'c1:m1': 'reply' })
  clearDraft(workspace, 'c1', 'm1')
  expect(stored(workspace)).toBeNull()
})

test('subscribers hear changes, and only changes', () => {
  let calls = 0
  const unsubscribe = subscribeDrafts(() => calls++)
  setDraft(workspace, 'c1', null, 'a')
  setDraft(workspace, 'c1', null, 'a')
  clearDraft(workspace, 'c2')
  expect(calls).toBe(1)
  clearDraft(workspace, 'c1')
  expect(calls).toBe(2)
  unsubscribe()
  setDraft(workspace, 'c1', null, 'b')
  expect(calls).toBe(2)
})

test('broken stored JSON reads as no drafts', () => {
  localStorage.setItem(`orbit:chat:drafts:${workspace}`, '{not json')
  expect(getDraft(workspace, 'c1')).toBe('')
  setDraft(workspace, 'c1', null, 'ok')
  expect(stored(workspace)).toEqual({ c1: 'ok' })
})

test('the last opened conversation is kept for each workspace', () => {
  expect(getLastOpenedConversation(workspace)).toBeNull()
  setLastOpenedConversation(workspace, 'c7')
  expect(getLastOpenedConversation(workspace)).toBe('c7')
  expect(getLastOpenedConversation(`${workspace}-other`)).toBeNull()
  setLastOpenedConversation(workspace, null)
  expect(getLastOpenedConversation(workspace)).toBeNull()
})
