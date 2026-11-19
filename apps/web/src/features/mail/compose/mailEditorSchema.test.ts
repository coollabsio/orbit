import { expect, test } from 'bun:test'
import { BlockNoteEditor } from '@blocknote/core'
import { htmlToMailBlocks, mailEditorSchema } from './mailEditorSchema'

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
