// Live presence in the collaborative editor: other people's carets carry their user id (the header avatars scroll to
// them), and each block someone else is in gets a margin marker with their initials in their cursor color.
import { describe, expect, test } from 'bun:test'
import { act, render, waitFor } from '@testing-library/react'
import * as Y from 'yjs'
import { COLLAB_FRAGMENT } from './connection'
import { revealCollabCursor } from './cursor'
import { PageEditor } from '@/features/docs/editor/PageEditor'
import { FakeProvider, writeBlocks } from '@/test/fakeCollab'

/** A caret two characters into the paragraph that says `text` (as y-prosemirror publishes it). */
function caretIn(fragment: Y.XmlFragment, text: string) {
  const node = [...fragment.createTreeWalker((item) => item instanceof Y.XmlText && item.toString() === text)][0] as Y.XmlText
  const position = Y.relativePositionToJSON(Y.createRelativePositionFromTypeIndex(node, 2))
  return { anchor: position, head: position }
}

async function renderEditor() {
  const doc = new Y.Doc()
  const fragment = doc.getXmlFragment(COLLAB_FRAGMENT)
  writeBlocks(fragment, [
    { type: 'paragraph', content: 'First' },
    { type: 'paragraph', content: 'Second' },
  ])
  const provider = new FakeProvider(doc)
  let view!: ReturnType<typeof render>
  await act(async () => {
    view = render(
      <PageEditor
        pageId="page-1"
        collab={{ provider, fragment, user: { id: 'ann', name: 'Ann', color: '#737373' } }}
        editable
        resolvePage={() => null}
        onOpenPage={() => {}}
        onCreateSubpage={async () => 'x'}
        onPickPage={async () => null}
      />,
    )
    await new Promise((resolve) => setTimeout(resolve, 20))
  })
  await waitFor(() => expect(view.container.textContent).toContain('Second'))
  return { view, provider, fragment }
}

const markers = (container: HTMLElement) => [...container.querySelectorAll<HTMLElement>('.bn-block-outer[data-collab-presence]')]

describe('presence in the collaborative editor', () => {
  test('marks the block of each other person once, without the own user, and follows leaves', async () => {
    const { view, provider, fragment } = await renderEditor()
    let bob = 0
    await act(async () => {
      bob = provider.addPeer({ id: 'bob', name: 'Bob Stone', color: '#2563eb' }, caretIn(fragment, 'Second'))
      provider.addPeer({ id: 'bob', name: 'Bob Stone', color: '#2563eb' }, caretIn(fragment, 'Second')) // a second tab
      provider.addPeer({ id: 'cat', name: 'Cat', color: '#d97706' }, caretIn(fragment, 'Second'))
      provider.addPeer({ id: 'ann', name: 'Ann', color: '#e11d48' }, caretIn(fragment, 'First')) // the own other tab
      provider.addPeer({ id: 'dan', name: 'Dan', color: '#059669' }) // viewing: no caret
      await new Promise((resolve) => setTimeout(resolve, 20))
    })
    await waitFor(() => expect(markers(view.container)).toHaveLength(1))
    const [marker] = markers(view.container)
    // the second paragraph, where the carets are (their name labels are inside it too)
    expect(marker.querySelector('[data-collab-user="bob"]')).toBeTruthy()
    expect(marker.textContent).not.toContain('First')
    expect(marker.dataset.collabPresence).toBe('BS +1')
    expect(marker.getAttribute('style')).toContain('--collab-color: #2563eb')

    // The caret carries the user id, so the header can scroll to it.
    expect(revealCollabCursor(view.container, 'bob')).toBe(true)
    expect(view.container.querySelector('[data-collab-user="bob"]')!.hasAttribute('data-active')).toBe(true)
    expect(revealCollabCursor(view.container, 'dan')).toBe(false)

    await act(async () => {
      for (const [clientId, state] of provider.awareness.getStates()) {
        if ((state.user as { id?: string } | undefined)?.id === 'bob') provider.awareness.states.delete(clientId)
      }
      provider.awareness.emit('change', [{ added: [], updated: [], removed: [bob] }, 'remote'])
      await new Promise((resolve) => setTimeout(resolve, 20))
    })
    await waitFor(() => expect(markers(view.container)[0]?.dataset.collabPresence).toBe('C'))
    act(() => provider.destroy())
  })
})
