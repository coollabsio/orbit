// `/docs/<page>?block=<block id>` (an inbox page mention): once the page's editor shows that block, scroll it into view
// and flash it, then drop the parameter so reloads and back navigation do not jump again.
import { useEffect } from 'react'
import { useSearchParams } from 'react-router'

/** Tailwind classes of the brief highlight (spelled out so Tailwind generates them). */
const FLASH_CLASSES = ['rounded-md', 'bg-primary/10']
const TIMEOUT_MS = 8_000

/** The editor's DOM node of a block (BlockNote marks each block container with `data-id`). */
export function findBlockElement(root: ParentNode, blockId: string): HTMLElement | null {
  const escaped = typeof CSS !== 'undefined' && CSS.escape ? CSS.escape(blockId) : blockId.replace(/["\\]/g, '\\$&')
  return root.querySelector<HTMLElement>(`[data-node-type="blockContainer"][data-id="${escaped}"]`)
}

export function useBlockDeepLink(pageId: string) {
  const [searchParams, setSearchParams] = useSearchParams()
  const blockId = searchParams.get('block')
  useEffect(() => {
    if (!blockId) return
    let done = false
    const started = Date.now()
    const clear = () =>
      setSearchParams(
        (current) => {
          const next = new URLSearchParams(current)
          next.delete('block')
          return next
        },
        { replace: true },
      )
    // The editor mounts after the collaborative document syncs, so poll briefly for the block.
    const timer = window.setInterval(() => {
      if (done) return
      const element = findBlockElement(document, blockId)
      if (!element && Date.now() - started < TIMEOUT_MS) return
      done = true
      window.clearInterval(timer)
      if (element) {
        element.scrollIntoView({ block: 'center' })
        element.classList.add(...FLASH_CLASSES)
        window.setTimeout(() => element.classList.remove(...FLASH_CLASSES), 2_000)
      }
      clear()
    }, 100)
    return () => window.clearInterval(timer)
  }, [blockId, pageId, setSearchParams])
}
