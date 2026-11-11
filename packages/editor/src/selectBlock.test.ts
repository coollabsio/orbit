import { describe, expect, test } from 'bun:test'
import { Schema } from '@tiptap/pm/model'
import { EditorState, TextSelection } from '@tiptap/pm/state'
import { selectBlockText } from './selectBlock'

const schema = new Schema({ nodes: { doc: { content: 'block+' }, block: { content: 'text*' }, text: {} } })
const doc = schema.node('doc', null, [
  schema.node('block', null, [schema.text('first')]),
  schema.node('block', null, [schema.text('second')]),
  schema.node('block'),
])
const at = (from: number, to = from) => EditorState.create({ doc, selection: TextSelection.create(doc, from, to) })
const selected = (state: EditorState) => {
  const tr = selectBlockText(state)
  return tr && tr.doc.textBetween(tr.selection.from, tr.selection.to)
}

describe('selectBlockText', () => {
  test('selects the text of the block with the cursor, not the first block', () => {
    expect(selected(at(11))).toBe('second')
  })
  test('lets the whole page be selected when the block text is already selected', () => {
    expect(selectBlockText(at(9, 15))).toBeNull()
  })
  test('lets the whole page be selected from an empty block or across blocks', () => {
    expect(selectBlockText(at(17))).toBeNull()
    expect(selectBlockText(at(2, 11))).toBeNull()
  })
})
