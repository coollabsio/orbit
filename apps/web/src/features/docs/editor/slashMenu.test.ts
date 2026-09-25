import { describe, expect, mock, test } from 'bun:test'
import { BlockNoteEditor } from '@blocknote/core'
import { getPageSlashMenuItems, mergeSlashItems, PAGE_ITEMS_GROUP, preferTitleMatches } from './slashMenu'
import { EDITOR_BLOCK_TYPES, FILE_BLOCK_TYPES, pageEditorSchema, REMOVED_BLOCK_TYPES } from './schema'

function editor() {
  return BlockNoteEditor.create({ schema: pageEditorSchema })
}

describe('mergeSlashItems', () => {
  test('puts custom items at the start of the group run so the group label is printed once', () => {
    const defaults = [{ id: 'h1', group: 'Headings' }, { id: 'p', group: 'Basic blocks' }, { id: 'q', group: 'Basic blocks' }, { id: 't', group: 'Advanced' }]
    const merged = mergeSlashItems(defaults, [{ id: 'sub', group: 'Basic blocks' }])
    expect(merged.map((item) => item.id)).toEqual(['h1', 'sub', 'p', 'q', 't'])
  })

  test('prepends when the group is missing', () => {
    expect(mergeSlashItems([{ id: 'a', group: 'Other' }], [{ id: 'sub', group: 'Basic blocks' }]).map((item) => item.id)).toEqual(['sub', 'a'])
  })
})

describe('schema', () => {
  test('has the page block, image and file blocks, and no video or audio blocks', () => {
    expect(EDITOR_BLOCK_TYPES.has('page')).toBe(true)
    expect(EDITOR_BLOCK_TYPES.has('paragraph')).toBe(true)
    expect([...FILE_BLOCK_TYPES]).toEqual(['image', 'file'])
    for (const type of FILE_BLOCK_TYPES) expect(EDITOR_BLOCK_TYPES.has(type)).toBe(true)
    expect([...REMOVED_BLOCK_TYPES]).toEqual(['video', 'audio'])
    for (const type of REMOVED_BLOCK_TYPES) expect(EDITOR_BLOCK_TYPES.has(type)).toBe(false)
    expect(pageEditorSchema.blockSchema.image.propSchema.url.default).toBe('')
    expect(pageEditorSchema.blockSchema.page.content).toBe('none')
    expect(pageEditorSchema.blockSchema.page.propSchema.pageId.default).toBe('')
  })

  test('has the callout block: inline content, emoji and color props with Notion-like defaults', () => {
    const callout = pageEditorSchema.blockSchema.callout
    expect(EDITOR_BLOCK_TYPES.has('callout')).toBe(true)
    expect(callout.content).toBe('inline')
    expect(callout.propSchema.emoji.default).toBe('💡')
    expect(callout.propSchema.backgroundColor.default).toBe('gray')
    expect(callout.propSchema.textColor.default).toBe('default')
  })

  test('a callout keeps rich text, props and nested children through the internal clipboard HTML', async () => {
    const e = BlockNoteEditor.create({
      schema: pageEditorSchema,
      initialContent: [
        {
          type: 'callout',
          props: { emoji: '⭐', backgroundColor: 'red' },
          content: [
            { type: 'text', text: 'Hello ', styles: {} },
            { type: 'text', text: 'bold', styles: { bold: true } },
          ],
          children: [{ type: 'paragraph', content: 'inside' }],
        },
      ],
    })
    const parsed = await e.tryParseHTMLToBlocks(await e.blocksToFullHTML(e.document))
    expect(parsed).toHaveLength(1)
    expect(parsed[0].type).toBe('callout')
    expect(parsed[0].props).toEqual({ emoji: '⭐', backgroundColor: 'red', textColor: 'default' })
    expect(parsed[0].content).toEqual(e.document[0].content)
    expect(parsed[0].children.map((child) => child.type)).toEqual(['paragraph'])

    // Other apps get the emoji followed by the text.
    expect(await e.blocksToHTMLLossy(e.document)).toContain('⭐ </span>')
  })
})

describe('getPageSlashMenuItems', () => {
  const actions = { onSubpage: mock(() => {}), onLinkPage: mock(() => {}) }

  test('offers the page items inside a single "Basic blocks" group, Image and File, but no Video or Audio', () => {
    const items = getPageSlashMenuItems(editor(), actions, '')
    const titles = items.map((item) => item.title)
    expect(titles).toContain('Sub-page')
    expect(titles).toContain('Link a page')
    for (const title of ['Image', 'File']) expect(titles).toContain(title)
    for (const title of ['Video', 'Audio']) expect(titles).not.toContain(title)

    const groupRuns = items.map((item) => item.group).filter((group, index, all) => group !== all[index - 1])
    expect(groupRuns.filter((group) => group === PAGE_ITEMS_GROUP)).toHaveLength(1)
    const subpage = items.find((item) => item.title === 'Sub-page')
    expect(subpage?.group).toBe(PAGE_ITEMS_GROUP)
    expect(subpage?.icon).toBeDefined()
  })

  test('filters by title and aliases and runs the matching action', () => {
    const e = editor()
    expect(getPageSlashMenuItems(e, actions, 'subpage').map((item) => item.title)).toEqual(['Sub-page'])
    const linkItems = getPageSlashMenuItems(e, actions, 'mention')
    expect(linkItems.map((item) => item.title)).toEqual(['Link a page'])

    linkItems[0].onItemClick()
    expect(actions.onLinkPage).toHaveBeenCalledTimes(1)
    expect(actions.onSubpage).not.toHaveBeenCalled()
  })

  test('offers Callout right after Quote in Basic blocks; it turns the empty line into a callout', () => {
    const e = editor()
    const items = getPageSlashMenuItems(e, actions, '')
    const titles = items.map((item) => item.title)
    expect(titles.indexOf('Callout')).toBe(titles.indexOf('Quote') + 1)
    expect(items.find((item) => item.title === 'Callout')?.group).toBe(PAGE_ITEMS_GROUP)
    for (const alias of ['callout', 'note', 'tip', 'warning', 'info']) {
      expect(getPageSlashMenuItems(e, actions, alias).map((item) => item.title)).toContain('Callout')
    }
    expect(getPageSlashMenuItems(e, actions, 'call')[0].title).toBe('Callout')

    e.setTextCursorPosition(e.document[0].id)
    getPageSlashMenuItems(e, actions, 'callout')[0].onItemClick()
    expect(e.document[0].type).toBe('callout')
    expect(e.document[0].props).toEqual({ emoji: '💡', backgroundColor: 'gray', textColor: 'default' })
  })

  test('a title match is selected first, before alias-only matches ("/sub" → Sub-page)', () => {
    const items = getPageSlashMenuItems(editor(), actions, 'sub')
    expect(items[0].title).toBe('Sub-page')
    const groupRuns = items.map((item) => item.group).filter((group, index, all) => group !== all[index - 1])
    expect(new Set(groupRuns).size).toBe(groupRuns.length)
  })
})

describe('preferTitleMatches', () => {
  const items = [
    { title: 'Heading 2', group: 'Headings' },
    { title: 'Toggle list', group: 'Basic' },
    { title: 'Table', group: 'Basic' },
    { title: 'Code', group: 'Advanced' },
  ]

  test('moves title-hit groups to the front and title hits to the top of their group, keeping groups whole', () => {
    expect(preferTitleMatches(items, 'ta').map((item) => item.title)).toEqual(['Table', 'Toggle list', 'Heading 2', 'Code'])
  })

  test('keeps the order for an empty query', () => {
    expect(preferTitleMatches(items, ' ')).toEqual(items)
  })
})
