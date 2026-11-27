import { expect, test } from 'bun:test'
import { BlockNoteEditor } from '@blocknote/core'
import { htmlToMailBlocks, mailBlocksToHtml, mailEditorSchema, type MailBlock } from './mailEditorSchema'

test('an inline image is sent as its cid: URL, not as the preview URL the editor shows', () => {
  const editor = BlockNoteEditor.create({
    schema: mailEditorSchema,
    resolveFileUrl: async () => 'blob:preview',
    _tiptapOptions: { injectCSS: false },
  })
  const blocks = htmlToMailBlocks('<p>Hi</p><img src="cid:img-1@orbit" alt="shot.png" width="300">')
  expect(blocks.map((block) => block.type)).toEqual(['paragraph', 'image'])
  const html = editor.blocksToHTMLLossy(blocks)
  expect(html).toContain('src="cid:img-1@orbit"')
  expect(html).toContain('width="300"')
  expect(html).not.toContain('blob:')
})

test('an empty line is sent as a blank line, not as the editor placeholder character, and reads back as an empty line', () => {
  const editor = BlockNoteEditor.create({ schema: mailEditorSchema, _tiptapOptions: { injectCSS: false } })
  const blocks = [{ type: 'paragraph', content: 'Hi' }, { type: 'paragraph' }, { type: 'paragraph', content: 'Best regards' }]
  const html = mailBlocksToHtml(editor, blocks as MailBlock[])
  expect(html).toBe('<p>Hi</p><p><br></p><p>Best regards</p>')
  expect(htmlToMailBlocks(html).map((block) => block.content)).toEqual([
    [{ type: 'text', text: 'Hi', styles: {} }],
    [],
    [{ type: 'text', text: 'Best regards', styles: {} }],
  ])
})
