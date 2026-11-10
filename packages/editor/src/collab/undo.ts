import { createExtension } from '@blocknote/core'
import { Plugin } from '@tiptap/pm/state'
import { redoCommand, undoCommand, ySyncPluginKey, yUndoPlugin, yUndoPluginKey } from 'y-prosemirror'
import type { UndoManager } from 'yjs'

type StackItemEvent = Parameters<Parameters<UndoManager['on']>[1]>[0] & { stackItem: { meta: Map<unknown, unknown> } }

/**
 * BlockNote's `yUndo` extension, with the Yjs UndoManager owned by the editor state instead of the editor view.
 *
 * y-prosemirror creates the UndoManager in the plugin state but destroys it when the view is destroyed. The state
 * outlives the view: React StrictMode (and any remount) destroys the first view and mounts a second one on the same
 * state, which then undoes nothing. Here a view only adds and removes its own listeners; the UndoManager ends with
 * its Y.Doc (it destroys itself on the doc's `destroy`), which the collab session destroys with the editor.
 */
function collabUndoPlugin() {
  const base = yUndoPlugin()
  return new Plugin({
    ...base.spec,
    view: (view) => {
      const undoManager = yUndoPluginKey.getState(view.state)!.undoManager
      const binding = () => ySyncPluginKey.getState(view.state)?.binding
      const onAdded = ({ stackItem }: StackItemEvent) => {
        const current = binding()
        if (current) stackItem.meta.set(current, yUndoPluginKey.getState(view.state)!.prevSel)
      }
      const onPopped = ({ stackItem }: StackItemEvent) => {
        const current = binding()
        if (current) current.beforeTransactionSelection = stackItem.meta.get(current) ?? current.beforeTransactionSelection
      }
      undoManager.on('stack-item-added', onAdded)
      undoManager.on('stack-item-popped', onPopped)
      return {
        destroy: () => {
          undoManager.off('stack-item-added', onAdded)
          undoManager.off('stack-item-popped', onPopped)
        },
      }
    },
  })
}

/** Same key as BlockNote's extension: listed before `withCollaboration` adds that one, it wins (first registration). */
export const collabUndoExtension = createExtension(() => ({
  key: 'yUndo',
  prosemirrorPlugins: [collabUndoPlugin()],
  dependsOn: ['yCursor', 'ySync'],
  undoCommand,
  redoCommand,
}) as const)
