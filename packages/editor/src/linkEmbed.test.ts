import { describe, expect, test } from 'bun:test'
import { BlockNoteEditor } from '@blocknote/core'
import { onlyLinkHref } from './linkEmbed'
import { mediaEmbed } from './mediaFiles'
import { createEditorSchemas } from './schema'

function editor() {
  const { pageEditorSchema } = createEditorSchemas({
    page: { render: () => null },
    callout: { render: () => null },
    mention: { render: () => null },
    math: { render: () => null },
    codeBlock: { tokenClass: () => undefined },
  })
  return BlockNoteEditor.create({ schema: pageEditorSchema, _tiptapOptions: { injectCSS: false } })
}

function firstParagraph(doc: { descendants: (visitor: (node: Parameters<typeof onlyLinkHref>[0]) => boolean | void) => void }) {
  const nodes: Parameters<typeof onlyLinkHref>[0][] = []
  doc.descendants((node) => {
    if (nodes.length === 0 && node.type.name === 'paragraph') nodes.push(node)
    return nodes.length === 0
  })
  const node = nodes[0]
  if (!node) throw new Error('no paragraph')
  return node
}

describe('onlyLinkHref', () => {
  test('a paragraph that is only a video link is an embed', () => {
    const instance = editor()
    instance.replaceBlocks(instance.document, [
      { type: 'paragraph', content: [{ type: 'link', href: 'https://youtu.be/dQw4w9WgXcQ', content: 'External video' }] },
    ])
    const node = firstParagraph(instance.prosemirrorView.state.doc)
    const href = onlyLinkHref(node)
    expect(href).toBe('https://youtu.be/dQw4w9WgXcQ')
    expect(mediaEmbed(href ?? '', node.textContent)?.src).toBe('https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ')
  })

  test('a sentence that contains a link stays a sentence', () => {
    const instance = editor()
    instance.replaceBlocks(instance.document, [
      {
        type: 'paragraph',
        content: [
          'See ',
          { type: 'link', href: 'https://youtu.be/dQw4w9WgXcQ', content: 'this video' },
        ],
      },
    ])
    const node = firstParagraph(instance.prosemirrorView.state.doc)
    expect(onlyLinkHref(node)).toBeNull()
  })
})
