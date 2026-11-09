import { createExtension } from '@blocknote/core'
import type { EditorState, Transaction } from '@tiptap/pm/state'
import { TextSelection } from '@tiptap/pm/state'

/**
 * Cmd/Ctrl+A selects the text of the block that holds the cursor. When that text is already selected (or the block
 * is empty), it returns null, so the editor's own Cmd/Ctrl+A selects the whole page.
 */
export function selectBlockText(state: EditorState): Transaction | null {
  const { $from, $to } = state.selection
  if (!$from.sameParent($to) || !$from.parent.isTextblock) return null
  const start = $from.start()
  const end = $from.end()
  if (start === end || (state.selection.from === start && state.selection.to === end)) return null
  return state.tr.setSelection(TextSelection.create(state.doc, start, end))
}

export function selectBlockExtension() {
  return createExtension({
    key: 'orbitSelectBlock',
    keyboardShortcuts: {
      'Mod-a': ({ editor }) => {
        const view = editor.prosemirrorView
        const tr = view && selectBlockText(view.state)
        if (!tr) return false
        view.dispatch(tr)
        return true
      },
    },
  })
}
