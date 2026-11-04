import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { useViewUrl } from '@/features/mail/api/queries'
import { FRAME_MIN_HEIGHT, frameHeight } from '@/features/mail/mailLib'

/**
 * A message's HTML in a sandboxed frame (spec §6.5). Without `allow-same-origin` the frame has an opaque origin: it
 * cannot read Orbit's cookies, storage or this page. Its one script posts the content height; only a finite number
 * from this frame's own window sets the height (80–20,000 px).
 */
export function MailHtmlFrame({ messageId, loadImages, title }: { messageId: string; loadImages: boolean; title: string }) {
  const view = useViewUrl(messageId, loadImages)
  const frame = useRef<HTMLIFrameElement>(null)
  const [height, setHeight] = useState(FRAME_MIN_HEIGHT)

  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      const next = frameHeight(event, frame.current?.contentWindow)
      if (next !== null) setHeight(next)
    }
    window.addEventListener('message', onMessage)
    return () => window.removeEventListener('message', onMessage)
  }, [])

  if (view.isError) {
    return (
      <div role="alert" className="flex items-center gap-3 rounded-md border px-3 py-2 text-[13px] text-muted-foreground">
        The message could not be shown.
        <Button variant="outline" size="sm" onClick={() => void view.refetch()}>
          Try again
        </Button>
      </div>
    )
  }
  if (!view.data) return <Skeleton className="h-20 w-full" />
  return (
    <iframe
      ref={frame}
      data-slot="mail-frame"
      src={view.data.url}
      title={title}
      sandbox="allow-scripts allow-popups allow-popups-to-escape-sandbox"
      referrerPolicy="no-referrer"
      // mail is written for a white page, also in the dark theme
      className="block w-full rounded-md border-0 bg-white [color-scheme:light]"
      style={{ height }}
    />
  )
}
