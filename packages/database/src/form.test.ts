import { describe, expect, test } from 'bun:test'
import {
  addableProperties,
  answerable,
  answersBody,
  answerState,
  blockingAnswers,
  formConfig,
  formPath,
  formQuestions,
  movedQuestion,
  withQuestion,
  type DatabaseFormConfig,
} from './form'
import type { DatabaseProperty, Property, ViewConfig } from './types'
import { parseProperty } from './values'

const properties: Property[] = (
  [
    { id: 'p-title', name: 'Name', type: 'title', config: {}, position: 0 },
    { id: 'p-num', name: 'Score', type: 'number', config: {}, position: 1 },
    { id: 'p-url', name: 'Link', type: 'url', config: {}, position: 2 },
    { id: 'p-sel', name: 'Kind', type: 'select', config: { options: [{ id: 'o-a', name: 'Bug', color: 'red' }] }, position: 3 },
    { id: 'p-check', name: 'Urgent', type: 'checkbox', config: {}, position: 4 },
    { id: 'p-date', name: 'Due', type: 'date', config: {}, position: 5 },
    { id: 'p-files', name: 'Files', type: 'files', config: {}, position: 6 },
    { id: 'p-uid', name: 'ID', type: 'unique_id', config: {}, position: 7 },
    { id: 'p-rel', name: 'Project', type: 'relation', config: {}, position: 8 },
    { id: 'p-btn', name: 'Close', type: 'button', config: {}, position: 9 },
    { id: 'p-ct', name: 'Created', type: 'created_time', config: {}, position: 10 },
    { id: 'p-text', name: 'Notes', type: 'text', config: {}, position: 11 },
  ] satisfies DatabaseProperty[]
).map(parseProperty)

const byId = (id: string) => properties.find((property) => property.id === id)!
const form = (questions: DatabaseFormConfig['questions']): DatabaseFormConfig => ({ questions, access: 'members' })
const order = (config: DatabaseFormConfig) => (config.questions ?? []).map((question) => question.property_id)

test('answerable leaves out computed, unique ID, files, relation and button properties', () => {
  expect(properties.filter(answerable).map((property) => property.id)).toEqual(['p-title', 'p-num', 'p-url', 'p-sel', 'p-check', 'p-date', 'p-text'])
})

test('addableProperties lists answerable properties not asked yet, in property order', () => {
  expect(addableProperties(form([{ property_id: 'p-url' }, { property_id: 'p-title' }]), [...properties].reverse()).map((property) => property.id)).toEqual(['p-num', 'p-sel', 'p-check', 'p-date', 'p-text'])
})

describe('formConfig', () => {
  test('a view without a form asks for the title, required, open to members', () => {
    expect(formConfig({} as ViewConfig, properties)).toEqual({ questions: [{ property_id: 'p-title', required: true }], access: 'members' })
  })

  test('a stored form is used as is', () => {
    const stored: DatabaseFormConfig = { questions: [{ property_id: 'p-num' }], access: 'closed', title: 'Intake' }
    expect(formConfig({ form: stored } as ViewConfig, properties)).toEqual({ questions: [{ property_id: 'p-num' }], access: 'closed', title: 'Intake' })
  })
})

test('formQuestions keeps order, drops deleted, unanswerable and repeated questions, and fills label defaults', () => {
  const questions = formQuestions(
    form([
      { property_id: 'p-num', label: 'Your score', description: 'From 1 to 10', required: true },
      { property_id: 'p-gone' },
      { property_id: 'p-files' },
      { property_id: 'p-title', label: '' },
      { property_id: 'p-num' },
      { property_id: 'p-btn' },
    ]),
    properties,
  )
  expect(questions.map(({ property, ...question }) => ({ id: property.id, ...question }))).toEqual([
    { id: 'p-num', label: 'Your score', description: 'From 1 to 10', required: true },
    { id: 'p-title', label: 'Name', description: '', required: false },
  ])
})

describe('withQuestion and movedQuestion', () => {
  const base: DatabaseFormConfig = { questions: [{ property_id: 'p-title', required: true }, { property_id: 'p-num' }, { property_id: 'p-url' }], access: 'members', title: 'T' }

  test('withQuestion changes one question and keeps its property id', () => {
    expect(withQuestion(base, 'p-num', { label: 'Score?', required: true, property_id: 'p-url' })).toEqual({
      questions: [{ property_id: 'p-title', required: true }, { property_id: 'p-num', label: 'Score?', required: true }, { property_id: 'p-url' }],
      access: 'members',
      title: 'T',
    })
  })

  test('withQuestion with null removes the question', () => {
    expect(order(withQuestion(base, 'p-title', null))).toEqual(['p-num', 'p-url'])
  })

  test('movedQuestion moves to the index, clamped; an unknown property leaves the form as is', () => {
    expect(order(movedQuestion(base, 'p-url', 0))).toEqual(['p-url', 'p-title', 'p-num'])
    expect(order(movedQuestion(base, 'p-title', 99))).toEqual(['p-num', 'p-url', 'p-title'])
    expect(order(movedQuestion(base, 'p-gone', 0))).toEqual(['p-title', 'p-num', 'p-url'])
  })
})

test('answerState tells empty, invalid and filled answers apart', () => {
  expect([
    answerState(byId('p-title'), '  '),
    answerState(byId('p-title'), 'Crash'),
    answerState(byId('p-num'), undefined),
    answerState(byId('p-num'), 3),
    answerState(byId('p-url'), 'not a link'),
    answerState(byId('p-url'), 'https://example.com'),
    answerState(byId('p-sel'), 'o-gone'),
    answerState(byId('p-check'), false),
    answerState(byId('p-check'), true),
    answerState(byId('p-date'), { start: '2026-11-10', end: null }),
  ]).toEqual(['empty', 'filled', 'empty', 'filled', 'invalid', 'filled', 'invalid', 'empty', 'filled', 'filled'])
})

describe('blockingAnswers and answersBody', () => {
  const questions = formQuestions(
    form([{ property_id: 'p-title', required: true }, { property_id: 'p-num' }, { property_id: 'p-url' }, { property_id: 'p-check' }]),
    properties,
  )

  test('an empty required question and an invalid optional one block the form', () => {
    expect(blockingAnswers(questions, { 'p-title': ' ', 'p-url': 'nope' })).toEqual(['p-title', 'p-url'])
  })

  test('filled answers do not block, empty optional ones neither', () => {
    expect(blockingAnswers(questions, { 'p-title': 'Crash' })).toEqual([])
    expect(blockingAnswers(questions, {})).toEqual(['p-title'])
  })

  test('the body holds the filled answers by property id, the title trimmed', () => {
    expect(answersBody(questions, { 'p-title': '  Crash on save ', 'p-num': 7, 'p-url': '', 'p-check': false, 'p-text': 'not asked' })).toEqual({ 'p-title': 'Crash on save', 'p-num': 7 })
  })
})

test('formPath is the in-app form link', () => {
  expect(formPath('db', 'v1')).toBe('/docs/db/form/v1')
})
