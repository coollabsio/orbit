import { createExtension } from '@blocknote/core'
import { NodeSelection, Plugin, TextSelection } from '@tiptap/pm/state'
import type { EditorView } from '@tiptap/pm/view'

/**
 * The position of the block content node of a `types` block whose row the point (`x`, `y`) is beside: in the page
 * margin left or right of it, not on it. `null` elsewhere.
 */
export function marginBlockAt(view: EditorView, types: ReadonlySet<string>, x: number, y: number): number | null {
  for (const content of view.dom.querySelectorAll<HTMLElement>('[data-content-type]')) {
    if (!types.has(content.dataset.contentType ?? '')) continue
    const box = content.getBoundingClientRect()
    if (y < box.top || y > box.bottom || (x >= box.left && x <= box.right)) continue
    // A leaf node view's DOM position is the position before the node, or inside it for a node with content.
    const at = view.posAtDOM(content, 0)
    return [at, at - 1].find((pos) => pos >= 0 && types.has(view.state.doc.nodeAt(pos)?.type.name ?? '')) ?? null
  }
  return null
}

/**
 * A press in the page margin beside a block of `types` (blocks whose whole body is their own UI, like a database)
 * selects that block, as Notion does, instead of putting the caret in the nearest paragraph.
 */
export function marginSelectExtension(types: readonly string[]) {
  const set = new Set(types)
  return createExtension({
    key: 'orbitMarginSelect',
    prosemirrorPlugins: [
      new Plugin({
        props: {
          handleDOMEvents: {
            mousedown: (view, event) => {
              if (event.button !== 0 || event.shiftKey || !view.editable) return false
              const pos = marginBlockAt(view, set, event.clientX, event.clientY)
              if (pos === null) return false
              event.preventDefault()
              view.focus()
              view.dispatch(view.state.tr.setSelection(NodeSelection.create(view.state.doc, pos)))
              return true
            },
          },
          // With a block selected, Chrome keeps a hidden caret at the start of the next text block, so a click there
          // changes no DOM selection and ProseMirror's own fix only covers positions up to 2 away from the block.
          handleClick: (view, pos) => {
            if (!(view.state.selection instanceof NodeSelection) || !view.state.doc.resolve(pos).parent.inlineContent) return false
            view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, pos)))
            return true
          },
        },
      }),
    ],
  })
}
