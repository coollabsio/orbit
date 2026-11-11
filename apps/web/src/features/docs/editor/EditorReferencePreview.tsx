// The hover card of the page and comment editors: a mention chip shows the member, a link to a task shows the task.
// Below the text, because BlockNote's link toolbar opens above a link.
import { useContext, useEffect } from 'react'
import { useEditorDOMElement } from '@blocknote/react'
import { PersonPreview, ReferencePreviewCard, TaskPreview } from '@/components/common/ReferencePreview'
import { WorkspaceContext } from '@/features/workspaces/workspaceContext'
import { rectAt, useHoverPreview } from '@/lib/useHoverPreview'
import { editorReference } from './editorReferences'

export function EditorReferencePreview() {
  // members and tasks are read in the workspace: an editor outside one (a test, an embed) shows no previews
  const inWorkspace = useContext(WorkspaceContext) !== null
  const element = useEditorDOMElement()
  const { shown, show, hide } = useHoverPreview<string>()
  useEffect(() => {
    if (!element || !inWorkspace) return
    const move = (event: PointerEvent) => {
      // a touch places the caret or opens the link: only a mouse hovers
      if (event.pointerType !== 'mouse' || !(event.target instanceof Element)) return hide()
      const target = event.target.closest('[data-mention-user], a[href]')
      const reference = target && element.contains(target) ? editorReference(target, window.location.origin) : null
      if (!target || !reference) return hide()
      const rect = rectAt(target, event.clientX, event.clientY) ?? target.getBoundingClientRect()
      show(`${reference}@${Math.round(rect.left)},${Math.round(rect.top)}`, reference, rect)
    }
    element.addEventListener('pointermove', move)
    element.addEventListener('pointerleave', hide)
    element.addEventListener('keydown', hide)
    window.addEventListener('scroll', hide, true)
    return () => {
      element.removeEventListener('pointermove', move)
      element.removeEventListener('pointerleave', hide)
      element.removeEventListener('keydown', hide)
      window.removeEventListener('scroll', hide, true)
    }
  }, [element, inWorkspace, show, hide])
  if (!shown) return null
  const split = shown.value.indexOf(':')
  const id = shown.value.slice(split + 1)
  return (
    <ReferencePreviewCard key={shown.key} rect={shown.rect} placement="below">
      {shown.value.startsWith('user:') ? <PersonPreview userId={id} /> : <TaskPreview taskRef={id} />}
    </ReferencePreviewCard>
  )
}
