// Custom blocks that hold their own UI (inputs, menus, drag and drop) inside the editor. ProseMirror reads every
// key, paste and drag that happens under its root, so a key typed in a block's input could also edit or delete the
// document. Elements marked with `ISOLATED_ATTR` keep their events to themselves.

/** Put on the root element of a block's own UI (with `contentEditable={false}`); the editor ignores its events. */
export const ISOLATED_ATTR = 'data-editor-isolated'

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
