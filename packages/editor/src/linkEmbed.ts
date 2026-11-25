// A paragraph whose whole text is one video or audio link shows the player above that link. A YouTube or Vimeo player
// loads only when the reader presses play: before that, the site does not learn who opened the page.
// The document stays a paragraph: the player is a decoration, so co-editing does not change the schema.
import { createExtension } from '@blocknote/core'
import type { Node as ProseMirrorNode } from '@tiptap/pm/model'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import { Decoration, DecorationSet } from '@tiptap/pm/view'
import { mediaEmbed, type MediaEmbed } from './mediaFiles'
import { mediaPreload, proxiedImageUrl } from './outsideUrl'

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

const PLAY_MARK =
  '<svg viewBox="0 0 24 24" width="20" height="20" fill="#fff" aria-hidden="true" style="margin-left:2px"><path d="M8 5.5v13l11-6.5z"/></svg>'

/** The player's URL, started at once: the reader already pressed play. */
export function autoplaySrc(src: string): string {
  const url = new URL(src)
  url.searchParams.set('autoplay', '1')
  return url.toString()
}

function frameElement(embed: MediaEmbed): HTMLIFrameElement {
  const frame = document.createElement('iframe')
  frame.src = autoplaySrc(embed.src)
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
  return frame
}

/** Turns an Orbit URL into one the page can load: the mobile editor's `resolveFileUrl` adds the session. */
type ResolveUrl = (url: string) => Promise<string>

/** The poster (through Orbit) with a play mark; pressing it puts the player in its place, in the same box. */
function playButton(embed: MediaEmbed, resolve: ResolveUrl): HTMLButtonElement {
  const button = document.createElement('button')
  button.type = 'button'
  button.dataset.orbitEmbedPlay = ''
  button.setAttribute('aria-label', `Play ${embed.title || 'video'}`)
  Object.assign(button.style, {
    display: 'block',
    position: 'relative',
    width: '100%',
    aspectRatio: '16 / 9',
    padding: '0',
    border: '0',
    borderRadius: '8px',
    overflow: 'hidden',
    background: '#000',
    cursor: 'pointer',
  })
  if (embed.poster) {
    const poster = document.createElement('img')
    void resolve(proxiedImageUrl(embed.poster, window.location.origin)).then((url) => {
      poster.src = url
    })
    poster.alt = ''
    poster.draggable = false
    Object.assign(poster.style, {
      display: 'block',
      width: '100%',
      height: '100%',
      objectFit: 'cover',
    })
    poster.addEventListener('error', () => poster.remove())
    button.appendChild(poster)
  }
  const mark = document.createElement('span')
  Object.assign(mark.style, {
    position: 'absolute',
    top: '50%',
    left: '50%',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: '44px',
    height: '44px',
    transform: 'translate(-50%, -50%)',
    borderRadius: '9999px',
    background: 'rgba(0, 0, 0, 0.65)',
  })
  mark.innerHTML = PLAY_MARK
  button.appendChild(mark)
  button.addEventListener('click', () => button.replaceWith(frameElement(embed)))
  return button
}

function embedElement(embed: MediaEmbed, resolve: ResolveUrl): HTMLElement {
  const box = document.createElement('div')
  box.contentEditable = 'false'
  box.dataset.orbitEmbed = embed.kind
  box.style.margin = '4px 0 8px'
  box.style.maxWidth = '100%'
  if (embed.kind === 'iframe') {
    box.appendChild(playButton(embed, resolve))
    return box
  }
  const media = document.createElement(embed.kind)
  media.controls = true
  media.preload = mediaPreload(embed.src, window.location.origin)
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

function linkEmbedDecorations(doc: ProseMirrorNode, resolve: ResolveUrl): DecorationSet {
  const widgets: Decoration[] = []
  doc.descendants((node, pos) => {
    if (node.type.name !== 'paragraph') return true
    const href = onlyLinkHref(node)
    if (!href) return false
    const embed = mediaEmbed(href, node.textContent)
    if (!embed) return false
    widgets.push(
      Decoration.widget(pos, () => embedElement(embed, resolve), {
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
  return createExtension(({ editor }) => {
    const resolve: ResolveUrl = (url) => editor.resolveFileUrl?.(url) ?? Promise.resolve(url)
    return {
      key: 'orbitLinkEmbed',
      prosemirrorPlugins: [
        new Plugin<DecorationSet>({
          key: linkEmbedKey,
          state: {
            init: (_, state) => linkEmbedDecorations(state.doc, resolve),
            apply(tr, previous) {
              return tr.docChanged ? linkEmbedDecorations(tr.doc, resolve) : previous.map(tr.mapping, tr.doc)
            },
          },
          props: { decorations: (state) => linkEmbedKey.getState(state) },
        }),
      ],
    }
  })()
}
