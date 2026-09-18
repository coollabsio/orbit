import { isEditableTarget } from '../../components/shell/bareKeyShortcut'

/**
 * Escape closes an open task, but only when nothing closer to the user already used it:
 * an open dialog, a menu or editor that handled the key (ProseMirror calls preventDefault),
 * or a text field, which Escape should leave first. So the first Escape exits the editor
 * and the second closes the task, as in Linear.
 */
export function shouldCloseTaskOnKey(event: KeyboardEvent): boolean {
  if (event.key !== 'Escape' || event.defaultPrevented) return false
  if (isEditableTarget(event.target)) return false
  return document.querySelector('[role="dialog"][aria-modal="true"]') === null
}
