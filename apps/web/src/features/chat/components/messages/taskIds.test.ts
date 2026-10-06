import { expect, test } from 'bun:test'
import { extractLinkUrls, standaloneLinkUrls } from './messageText'
import { findTaskIds, findTypedTaskIds, linkTaskIds } from './taskIds'

const KEYS = ['ENG', 'OPS_2']
const ids = (text: string) => findTaskIds(text, KEYS).map((match) => match.identifier)

test('an identifier of a project of the workspace is found, in any case', () => {
  expect(findTaskIds('see ENG-12 now', KEYS)).toEqual([{ start: 4, end: 10, escaped: false, identifier: 'ENG-12' }])
  expect(ids('eng-3, (OPS_2-40). Eng-7?\n**ENG-8** ENG-9\'s')).toEqual(['ENG-3', 'OPS_2-40', 'ENG-7', 'ENG-8', 'ENG-9'])
})

test('other keys and other words are text', () => {
  expect(ids('UTF-8 COVID-19 2024-10-05 WEB-1')).toEqual([])
  expect(ids('abc-ENG-12 ENG-12-rc xENG-12 ENG-12x ENG-012 ENG-0 ENG-12.txt ENG-1.5 a/ENG-12 ENG-12/b #eng-12 me@ENG-12 _ENG-12')).toEqual([])
  expect(findTaskIds('ENG-12', [])).toEqual([])
})

test('code, URLs and markdown links are left alone', () => {
  expect(ids('`ENG-1` ```\nENG-2\n``` https://x.test/ENG-3 [ENG-4](https://x.test/ENG-5) ENG-6')).toEqual(['ENG-6'])
  expect(ids('```\nENG-1 in an open fence')).toEqual([])
})

test('an identifier shares the link cards with the URL of its task', () => {
  const origin = 'https://orbit.test'
  const body = linkTaskIds('eng-12, https://orbit.test/tasks/ENG-12 and "ENG-13"', KEYS, origin)
  expect(extractLinkUrls(body)).toEqual(['https://orbit.test/tasks/ENG-12', 'https://orbit.test/tasks/ENG-13'])
  expect(standaloneLinkUrls(linkTaskIds('look\nENG-12\nENG-13 too', KEYS, origin))).toEqual(['https://orbit.test/tasks/ENG-12'])
  expect(linkTaskIds('no ids here', KEYS, origin)).toBe('no ids here')
})

test('a backslash before an identifier keeps it text, with no card', () => {
  const origin = 'https://orbit.test'
  expect(ids('\\ENG-1 C:\\eng-2 ENG-3')).toEqual(['ENG-3'])
  expect(findTypedTaskIds('a \\ENG-1 ENG-3', KEYS)).toEqual([
    { start: 2, end: 8, escaped: true, identifier: 'ENG-1' },
    { start: 9, end: 14, escaped: false, identifier: 'ENG-3' },
  ])
  // a backslash inside a word or a path escapes nothing, and what follows it is no identifier: all stays as typed
  expect(findTypedTaskIds('C:\\ENG-1 D:\\work\\ENG-1 x\\ENG-1 \\\\ENG-1 a/\\ENG-1', KEYS)).toEqual([])
  expect(findTypedTaskIds('(\\ENG-1)', KEYS)).toEqual([{ start: 1, end: 7, escaped: true, identifier: 'ENG-1' }])
  // not an identifier of the workspace: nothing to escape
  expect(findTypedTaskIds('C:\\WEB-1 \\UTF-8 \\ENG-1x', KEYS)).toEqual([])
  expect(linkTaskIds('\\ENG-1', KEYS, origin)).toBe('\\ENG-1')
  expect(extractLinkUrls(linkTaskIds('\\ENG-1 and ENG-2', KEYS, origin))).toEqual(['https://orbit.test/tasks/ENG-2'])
})
