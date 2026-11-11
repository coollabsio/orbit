import { describe, expect, test } from 'bun:test'

import { paragraphBlocks } from './docs'
import { applyMention, encodeMentions, insertTrigger, looksLikeTaskId, mentionedUserIds, mentionQuery, mentionSuggestions, mixSuggestions, rankTaskSuggestions, taskSearchText } from './mentions'

const people = [
  { id: 'u1', name: 'Ada Lovelace', handle: 'ada' },
  { id: 'u2', name: 'Ada', handle: 'ada2' },
  { id: 'u3', name: 'Grace Hopper', handle: 'grace' },
]
const channels = [{ id: 'c1', name: 'general' }]

describe('mentionQuery', () => {
  test('opens at the start or after white space, with the text up to the cursor', () => {
    expect(mentionQuery('@', 1)).toEqual({ start: 0, end: 1, query: '', trigger: '@' })
    expect(mentionQuery('hi @gr', 6)).toEqual({ start: 3, end: 6, query: 'gr', trigger: '@' })
    expect(mentionQuery('hi @Ada Lo', 10)?.query).toBe('Ada Lo')
    expect(mentionQuery('hi @gr there', 6)?.query).toBe('gr')
  })

  test('stays closed in an email, across a new line, and for # without channels', () => {
    expect(mentionQuery('me@example', 10)).toBeNull()
    expect(mentionQuery('@ada\nnext', 9)).toBeNull()
    expect(mentionQuery('#gen', 4)).toBeNull()
    expect(mentionQuery('#gen', 4, true)).toEqual({ start: 0, end: 4, query: 'gen', trigger: '#' })
  })

  test('a cursor past the end (the draft was just cleared) reads the whole text', () => {
    expect(mentionQuery('', 12)).toBeNull()
  })
})

test('suggestions match name or handle without regard to case, people for @ and channels for #', () => {
  expect(mentionSuggestions({ start: 0, end: 3, query: 'GR', trigger: '@' }, people, channels).map((item) => item.id)).toEqual(['u3'])
  expect(mentionSuggestions({ start: 0, end: 5, query: 'ada2', trigger: '@' }, people, channels).map((item) => item.id)).toEqual(['u2'])
  expect(mentionSuggestions({ start: 0, end: 1, query: '', trigger: '#' }, people, channels)).toEqual([{ id: 'c1', label: 'general', detail: '', kind: 'channel' }])
})

test('picking replaces the query with the name and a space, and the @ button adds an @ at the cursor', () => {
  const query = mentionQuery('hi @gr!', 6)!
  expect(applyMention('hi @gr!', query, { id: 'u3', label: 'Grace Hopper', detail: 'grace', kind: 'user' })).toEqual({ text: 'hi @Grace Hopper !', cursor: 17 })
  expect(insertTrigger('', 0)).toEqual({ text: '@', cursor: 1 })
  expect(insertTrigger('hi', 2)).toEqual({ text: 'hi @', cursor: 4 })
  expect(insertTrigger('hi there', 3)).toEqual({ text: 'hi @there', cursor: 4 })
})

test('a chat body stores mentions as tokens: longest name first, code left alone', () => {
  expect(encodeMentions('@Ada Lovelace and @ada, see #general @here', people, channels)).toBe('<@u1> and <@u2>, see <#c1> <!here>')
  expect(encodeMentions('`@Ada` me@Ada', people, channels)).toBe('`@Ada` me@Ada')
})

test('a task comment names its mentioned people once each', () => {
  expect(mentionedUserIds('@Grace Hopper @Ada Lovelace @grace hopper @channel', people)).toEqual(['u3', 'u1'])
})

test('a page comment line holds mention nodes for @Name', () => {
  expect(paragraphBlocks('Hi @Ada Lovelace!', () => 'b1', people)[0].content).toEqual([
    { type: 'text', text: 'Hi ', styles: {} },
    { type: 'mention', props: { userId: 'u1', name: 'Ada Lovelace' } },
    { type: 'text', text: '!', styles: {} },
  ])
})

describe('task suggestions', () => {
  const task = (id: string, detail: string, label: string) => ({ id, label, detail, kind: 'task' as const })

  test('a picked task goes in as its identifier', () => {
    const query = mentionQuery('see #ship', 9, true)!
    expect(applyMention('see #ship', query, task('t1', 'ENG-12', 'Ship it'))).toEqual({ text: 'see ENG-12 ', cursor: 11 })
  })

  test('a short query searches tasks, none lists recent ones, a heading or plain text none', () => {
    expect(taskSearchText('login')).toBe('login')
    expect(taskSearchText('')).toBe('')
    expect(taskSearchText(' Heading')).toBeNull()
    expect(taskSearchText('can you check this one please')).toBeNull()
  })

  test('the exact identifier ranks first, and an identifier puts tasks before channels', () => {
    const tasks = [task('a', 'ENG-120', 'Other'), task('b', 'ENG-12', 'The one')]
    expect(rankTaskSuggestions(tasks, 'eng-12').map((item) => item.detail)).toEqual(['ENG-12', 'ENG-120'])
    expect(looksLikeTaskId('eng-12')).toBe(true)
    expect(looksLikeTaskId('login')).toBe(false)
    const channel = { id: 'c1', label: 'general', detail: '', kind: 'channel' as const }
    expect(mixSuggestions([channel], tasks, 'eng-12').map((item) => item.id)).toEqual(['a', 'b', 'c1'])
    expect(mixSuggestions([channel], tasks, 'gen').map((item) => item.id)).toEqual(['c1', 'a', 'b'])
  })
})
