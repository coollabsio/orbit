import type { BlockNoteEditor } from '@blocknote/core'

/**
 * Locks or unlocks a mounted editor in place. BlockNoteView mounts its view again on every change of its `editable`
 * prop and switches the mode while the view is away; that fires a change, and a listener that reads the view (the link
 * toolbar with the cursor on a link) throws and takes down the page (BlockNote 0.55). So editors pass BlockNoteView the
 * first value only and switch later changes here.
 *
 * Tiptap puts back its first view attributes on this switch (role goes) and its tabindex plugin drops the tabindex of
 * a read-only editor, which BlockNote keeps focusable: the mounted attributes stay, tabindex included.
 */
export function setEditableInPlace(editor: BlockNoteEditor<any, any, any>, editable: boolean): void {
  if (editor.isEditable === editable) return
  const view = editor.prosemirrorView
  const attributes = view?.props.attributes
  editor.isEditable = editable
  if (view && attributes && typeof attributes === 'object') view.setProps({ attributes: { ...attributes, tabindex: '0' } })
}
