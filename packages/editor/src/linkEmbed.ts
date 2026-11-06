// A paragraph whose whole text is one video or audio link shows the player above that link.
// The document stays a paragraph: the player is a decoration, so co-editing does not change the schema.
import { createExtension } from '@blocknote/core'
import type { Node as ProseMirrorNode } from '@tiptap/pm/model'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import { Decoration, DecorationSet } from '@tiptap/pm/view'
import { mediaEmbed, type MediaEmbed } from './mediaFiles'

const linkEmbedKey = new PluginKey<DecorationSet>('orbitLinkEmbed')

/** The href when every text node of the paragraph carries the same link mark. Otherwise null. */
export function onlyLinkHref(node: ProseMirrorNode): string | null {
  if (node.childCount === 0) return null
  let href: string | null = null
  for (let index = 0; index < node.childCount; index += 1) {
    const child = node.child(index)
    if (!child.isText) return null
    const mark = child.marks.find((item) => item.type.name === 'link')
    const next = mark?.attrs.href
    if (typeof next !== 'string' || next === '') return null
    if (href !== null && href !== next) return null
    href = next
  }
  return href
}

function embedElement(embed: MediaEmbed): HTMLElement {
  const box = document.createElement('div')
  box.contentEditable = 'false'
  box.dataset.orbitEmbed = embed.kind
  box.style.margin = '4px 0 8px'
  box.style.maxWidth = '100%'
  if (embed.kind === 'iframe') {
    const frame = document.createElement('iframe')
    frame.src = embed.src
    frame.title = embed.title || 'Video'
    frame.allow = 'accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; fullscreen'
    frame.allowFullscreen = true
    frame.referrerPolicy = 'strict-origin-when-cross-origin'
    frame.style.display = 'block'
    frame.style.width = '100%'
    frame.style.aspectRatio = '16 / 9'
    frame.style.border = '0'
    frame.style.borderRadius = '8px'
    frame.style.background = '#000'
    box.appendChild(frame)
    return box
  }
  const media = document.createElement(embed.kind)
  media.controls = true
  media.preload = 'metadata'
  media.src = embed.src
  media.style.display = 'block'
  media.style.width = '100%'
  if (embed.kind === 'video') {
    media.style.maxHeight = '360px'
    media.style.borderRadius = '8px'
    media.style.background = '#000'
  }
  box.appendChild(media)
  return box
}

function linkEmbedDecorations(doc: ProseMirrorNode): DecorationSet {
  const widgets: Decoration[] = []
  doc.descendants((node, pos) => {
    if (node.type.name !== 'paragraph') return true
    const href = onlyLinkHref(node)
    if (!href) return false
    const embed = mediaEmbed(href, node.textContent)
    if (!embed) return false
    widgets.push(
      Decoration.widget(pos, () => embedElement(embed), {
        side: -1,
        ignoreSelection: true,
        stopEvent: () => true,
        key: `orbit-embed:${embed.kind}:${embed.src}`,
      }),
    )
    return false
  })
  return DecorationSet.create(doc, widgets)
}

/** Shows a player above a paragraph that is only a video or audio link. */
export function linkEmbedExtension() {
  return createExtension({
    key: 'orbitLinkEmbed',
    prosemirrorPlugins: [
      new Plugin<DecorationSet>({
        key: linkEmbedKey,
        state: {
          init: (_, state) => linkEmbedDecorations(state.doc),
          apply(tr, previous) {
            return tr.docChanged ? linkEmbedDecorations(tr.doc) : previous.map(tr.mapping, tr.doc)
          },
        },
        props: { decorations: (state) => linkEmbedKey.getState(state) },
      }),
    ],
  })
}
