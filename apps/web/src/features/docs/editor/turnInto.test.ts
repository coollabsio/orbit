import { describe, expect, test } from 'bun:test'
import { BlockNoteEditor } from '@blocknote/core'
import { pageEditorSchema } from './schema'
import { applyTurnInto, canTurnBlock, databaseTurn, turnIntoChoices, type TurnIntoChoice } from './turnInto'

function editor(content: unknown[]) {
  return BlockNoteEditor.create({ schema: pageEditorSchema, initialContent: content as never })
}

function choice(editorInstance: ReturnType<typeof editor>, name: string): TurnIntoChoice {
  const found = turnIntoChoices(editorInstance).find((item) => item.name === name)
  if (!found) throw new Error(`missing choice ${name}`)
  return found
}

describe('turn into', () => {
  test('offers text, headings, lists, code, quote, callout, and toggle headings', () => {
    const names = turnIntoChoices(editor([{ type: 'paragraph', content: 'Hi' }])).map((item) => item.name)
    expect(names).toEqual([
      'Text',
      'Heading 1',
      'Heading 2',
      'Heading 3',
      'Heading 4',
      'Heading 5',
      'Heading 6',
      'Bulleted list',
      'Numbered list',
      'To-do list',
      'Toggle list',
      'Code',
      'Quote',
      'Callout',
      'Toggle heading 1',
      'Toggle heading 2',
      'Toggle heading 3',
      'Toggle heading 4',
    ])
  })

  test('turns a paragraph into a heading and keeps its text', () => {
    const instance = editor([{ id: 'a', type: 'paragraph', content: 'Hi' }])
    applyTurnInto(instance, instance.document[0], choice(instance, 'Heading 2'))
    const props = instance.document[0].props as Record<string, unknown>
    expect(instance.document[0].type).toBe('heading')
    expect(props.level).toBe(2)
    expect(props.isToggleable).toBe(false)
    expect(instance.document[0].content).toEqual([{ type: 'text', text: 'Hi', styles: {} }])
  })

  test('a new callout uses the gray background', () => {
    const instance = editor([{ id: 'a', type: 'paragraph', content: 'Note' }])
    applyTurnInto(instance, instance.document[0], choice(instance, 'Callout'))
    const props = instance.document[0].props as Record<string, unknown>
    expect(instance.document[0].type).toBe('callout')
    expect(props.backgroundColor).toBe('gray')
    expect(props.emoji).toBe('💡')
  })

  test('clicking the current type keeps a callout color', () => {
    const instance = editor([{ id: 'a', type: 'callout', props: { emoji: '⭐', backgroundColor: 'red' }, content: 'Hot' }])
    applyTurnInto(instance, instance.document[0], choice(instance, 'Callout'))
    const props = instance.document[0].props as Record<string, unknown>
    expect(props.backgroundColor).toBe('red')
    expect(props.emoji).toBe('⭐')
  })

  test('a selection changes every text block and leaves an image', () => {
    const instance = editor([
      { id: 'a', type: 'paragraph', content: 'One' },
      { id: 'c', type: 'image' },
      { id: 'b', type: 'paragraph', content: 'Two' },
    ])
    instance.setSelection('a', 'b')
    applyTurnInto(instance, instance.document[0], choice(instance, 'Bulleted list'))
    expect(instance.document.map((block) => block.type)).toEqual(['bulletListItem', 'image', 'bulletListItem'])
  })

  test('an image, a page, and a table have no turn-into rows', () => {
    const instance = editor([
      { id: 'img', type: 'image' },
      { id: 'page', type: 'page', props: { pageId: 'x' } },
      { id: 'table', type: 'table', content: { type: 'tableContent', rows: [{ cells: [['']] }] } },
    ])
    for (const block of instance.document) expect(canTurnBlock(instance, block)).toBe(false)
  })
})

describe('databaseTurn', () => {
  const isDatabase = (pageId: string) => pageId === 'db'
  test('a page block that links to a database turns into an inline database block, and back', () => {
    expect(databaseTurn({ id: 'a', type: 'page', props: { pageId: 'db' } }, isDatabase)).toEqual({
      label: 'Inline database',
      type: 'database',
      props: { databaseId: 'db', linkId: '' },
    })
    expect(databaseTurn({ id: 'a', type: 'database', props: { databaseId: 'db', linkId: 'l1' } }, isDatabase)).toEqual({
      label: 'Page link',
      type: 'page',
      props: { pageId: 'db' },
    })
  })

  test('a link to a plain page and other blocks have no database turn', () => {
    expect(databaseTurn({ id: 'a', type: 'page', props: { pageId: 'doc' } }, isDatabase)).toBeNull()
    expect(databaseTurn({ id: 'a', type: 'paragraph', props: {} }, isDatabase)).toBeNull()
    expect(databaseTurn({ id: 'a', type: 'database', props: { databaseId: '', linkId: '' } }, isDatabase)).toBeNull()
  })
})
