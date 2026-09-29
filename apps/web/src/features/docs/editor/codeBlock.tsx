import { defaultBlockSpecs } from '@blocknote/core'
import { createRoot } from 'react-dom/client'
import { CopyCodeButton } from '@/components/common/CopyCodeButton'

const defaultCodeBlock = defaultBlockSpecs.codeBlock

/**
 * BlockNote's code block with a copy button in the top-right corner, shown on hover. The button is a small React
 * root outside the block's content DOM, so ProseMirror must ignore its DOM changes and events.
 */
export const codeBlockSpec: typeof defaultCodeBlock = {
  ...defaultCodeBlock,
  implementation: {
    ...defaultCodeBlock.implementation,
    render(block, editor) {
      const view = defaultCodeBlock.implementation.render.call(this, block, editor)
      // `dom` render (HTML export, clipboard) must stay plain code.
      if (this.renderType !== 'nodeView' || !(view.dom instanceof HTMLElement) || !view.contentDOM) return view

      const code = view.contentDOM
      const container = document.createElement('div')
      container.contentEditable = 'false'
      // The block content is `position: relative` (BlockNote), so this pins the button to its top-right corner.
      container.className = 'absolute top-2 right-2'
      view.dom.classList.add('group')
      view.dom.appendChild(container)
      const root = createRoot(container)
      root.render(<CopyCodeButton getText={() => code.textContent ?? ''} />)

      const destroy = view.destroy
      return {
        ...view,
        ignoreMutation: (mutation) => container.contains(mutation.target),
        stopEvent: (event: Event) => event.target instanceof Node && container.contains(event.target),
        destroy: () => {
          destroy?.()
          // Unmounting synchronously while React renders the editor logs a warning.
          queueMicrotask(() => root.unmount())
        },
      }
    },
  },
}
