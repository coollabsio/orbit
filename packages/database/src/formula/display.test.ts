import { describe, expect, test } from 'bun:test'
import { database } from '../testing'
import { parseProperty, readsEmpty } from '../values'
import { isDeletedProperty, tokenize } from './tokens'
import { formulaMessageParts, plainFormulaMessage } from './values'

const DELETED_ID = '0199a0b0-0000-7000-8000-0000000000aa'

describe('formula display', () => {
  test('messages split at code spans', () => {
    expect(formulaMessageParts('`+` cannot add text and number.')).toEqual([
      { text: '+', code: true },
      { text: ' cannot add text and number.', code: false },
    ])
    expect(plainFormulaMessage('Unknown function `lenght`. Did you mean `length`?')).toBe('Unknown function lenght. Did you mean length?')
    expect(plainFormulaMessage('Unexpected ```.')).toBe('Unexpected ```.')
  })

  test('a prop id no property has is a deleted property, an unknown name is not', () => {
    const deleted = tokenize(`prop("${DELETED_ID}") + prop("Nmae")`).filter((token) => token.kind === 'prop')
    expect(deleted.map(isDeletedProperty)).toEqual([true, false])
  })

  test('empty formula and rollup values read Empty, errors and true/false results do not', () => {
    const formula = (resultType: string) =>
      parseProperty({ id: 'f', name: 'F', type: 'formula', config: { expression: '', result_type: resultType }, position: 0 })
    const rollup = parseProperty({ id: 'r', name: 'R', type: 'rollup', config: { relation_property_id: 'x', target_property_id: 'y', function: 'show_original' }, position: 0 })
    const empty = { ...database.rows[0]!, values: {} }
    expect(readsEmpty(empty, formula('text'))).toBe(true)
    expect(readsEmpty(empty, rollup)).toBe(true)
    expect(readsEmpty({ ...empty, formulaErrors: { f: 'Unknown name `x`.' } }, formula('text'))).toBe(false)
    expect(readsEmpty(empty, formula('boolean'))).toBe(false)
  })
})
