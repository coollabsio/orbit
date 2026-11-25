import { expect, test } from 'bun:test'
import { fieldBody, moveId, nextOptionColor, optionsProblem } from './task-fields'

test('options need a name each and distinct names in any case', () => {
  expect(optionsProblem([{ name: 'Low' }, { name: 'High' }])).toBeNull()
  expect(optionsProblem([])).toBeNull()
  expect(optionsProblem([{ name: 'Low' }, { name: '  ' }])).toBe('Each option needs a name, and no two options can have the same name.')
  expect(optionsProblem([{ name: 'Low' }, { name: ' low ' }])).toBe('Each option needs a name, and no two options can have the same name.')
})

test('option colours follow the label palette in turn', () => {
  expect(nextOptionColor([])).toBe('#5e6ad2')
  expect(nextOptionColor([{}])).toBe('#26b5ce')
  expect(nextOptionColor(Array.from({ length: 9 }))).toBe('#5e6ad2')
})

test('the body trims names and sends options only for select kinds', () => {
  const options = [{ id: 'o1', name: ' Low ', color: '#5e6ad2' }]
  expect(fieldBody(' Budget ', 'number', options)).toEqual({ name: 'Budget', kind: 'number', options: [] })
  expect(fieldBody('Size', 'multi_select', options)).toEqual({ name: 'Size', kind: 'multi_select', options: [{ id: 'o1', name: 'Low', color: '#5e6ad2' }] })
})

test('moving a field swaps it with its neighbour and stops at the ends', () => {
  expect(moveId(['a', 'b', 'c'], 0, 1)).toEqual(['b', 'a', 'c'])
  expect(moveId(['a', 'b', 'c'], 2, -1)).toEqual(['a', 'c', 'b'])
  expect(moveId(['a', 'b', 'c'], 0, -1)).toEqual(['a', 'b', 'c'])
  expect(moveId(['a', 'b', 'c'], 2, 1)).toEqual(['a', 'b', 'c'])
})
