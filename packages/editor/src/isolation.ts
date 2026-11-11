// Custom blocks that hold their own UI (inputs, menus, drag and drop) inside the editor. ProseMirror reads every
// key, paste and drag that happens under its root, so a key typed in a block's input could also edit or delete the
// document. Elements marked with `ISOLATED_ATTR` keep their events to themselves.

import { createExtension } from '@blocknote/core'
import type { DropCursorOptions } from '@blocknote/core/extensions'
import { Plugin } from '@tiptap/pm/state'

/** Put on the root element of a block's own UI (with `contentEditable={false}`); the editor ignores its events. */
export const ISOLATED_ATTR = 'data-editor-isolated'

/** Marks a drag that starts in a block's own UI (a table row, a board card): its data is not editor content. */
const ISOLATED_DRAG_TYPE = 'application/x-orbit-isolated'

function isIsolatedDrag(event: DragEvent): boolean {
  return !!event.dataTransfer?.types.includes(ISOLATED_DRAG_TYPE)
}

/** The editor shows no drop cursor and drops nothing while such a drag passes over the page. */
export function isolatedDragsExtension() {
  return createExtension({
    key: 'orbitIsolatedDrags',
    mount({ root, signal }) {
      const mark = (event: Event) => {
        if (event instanceof DragEvent && event.target instanceof Element && event.target.closest(`[${ISOLATED_ATTR}]`)) event.dataTransfer?.setData(ISOLATED_DRAG_TYPE, '')
      }
      root.addEventListener('dragstart', mark, { capture: true, signal })
    },
    prosemirrorPlugins: [new Plugin({ props: { handleDrop: (_view, event) => isIsolatedDrag(event) } })],
  })
}

/** The editor's `dropCursor` option: no drop cursor for a drag from a block's own UI. */
export const isolatedDropCursor: DropCursorOptions = {
  hooks: { computeDropPosition: ({ event, defaultPosition }) => (isIsolatedDrag(event) ? null : defaultPosition) },
}

interface NodeViewEvents {
  dom: HTMLElement
  stopEvent?: (event: Event) => boolean
  /** Set while the editor drags its own content (text, or a block from the side menu). */
  view?: { dragging: unknown }
}

/** Whether ProseMirror must ignore `event` in a node view whose DOM is `dom`. */
export function isIsolatedEvent(dom: HTMLElement, event: Event, editorDragging: boolean): boolean {
  const target = event.target
  const element = target instanceof Element ? target : target instanceof Node ? target.parentElement : null
  const root = element?.closest(`[${ISOLATED_ATTR}]`)
  if (!root || !dom.contains(root)) return false
  // A block dragged from the side menu (or dragged text) may still be dropped onto this block.
  const editorDrag = editorDragging || (event instanceof DragEvent && !!event.dataTransfer?.types.includes('blocknote/html'))
  return !(event.type.startsWith('drag') || event.type === 'drop') || !editorDrag
}

interface SpecWithRender {
  implementation: { render: (...args: never[]) => unknown }
}

/** Makes the block's node view ignore events inside its `ISOLATED_ATTR` elements (see `isIsolatedEvent`). */
export function isolateEvents<Spec extends SpecWithRender>(spec: Spec): Spec {
  const render = spec.implementation.render as (this: { renderType?: string }, ...args: unknown[]) => unknown
  const isolated = function (this: { renderType?: string }, ...args: unknown[]) {
    const nodeView = render.apply(this, args)
    if (this.renderType !== 'nodeView') return nodeView
    const view = nodeView as NodeViewEvents
    const stopEvent = view.stopEvent?.bind(view)
    view.stopEvent = (event) => isIsolatedEvent(view.dom, event, !!view.view?.dragging) || (stopEvent?.(event) ?? false)
    return nodeView
  }
  return { ...spec, implementation: { ...spec.implementation, render: isolated } }
}
