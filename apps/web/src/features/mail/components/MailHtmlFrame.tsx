import { useCallback, useEffect, useRef, useState } from 'react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { useViewUrl } from '@/features/mail/api/queries'
import { backgroundBehind, FRAME_MIN_HEIGHT, frameMessage, frameThemeMessage } from '@/features/mail/mailLib'
import { useTheme } from '@/lib/themeContext'

/**
 * A message's HTML in a sandboxed frame (spec §6.5). Without `allow-same-origin` the frame has an opaque origin: it
 * cannot read Orbit's cookies, storage or this page. Its one script posts the content height and whether the mail is
 * plain; only a finite number from this frame's own window sets the height (80–20,000 px). A plain mail (no colours
 * of its own) sits on the app background and gets the app theme by `postMessage`; any other mail keeps a white page.
 */
/** The longest wait for a frame to say it is ready. */
const FRAME_SHOW_MS = 1000

export function MailHtmlFrame({
  messageId,
  loadImages,
  title,
  onBlockedImages,
}: {
  messageId: string
  loadImages: boolean
  title: string
  /** How many remote images the view removed, once the frame says. */
  onBlockedImages?: (count: number) => void
}) {
  const view = useViewUrl(messageId, loadImages)
  const frame = useRef<HTMLIFrameElement>(null)
  const [height, setHeight] = useState(FRAME_MIN_HEIGHT)
  const [plain, setPlain] = useState(false)
  /**
   * The frame may show: mail with its own colours at once, a plain one once it shows the app theme. Until then it is
   * hidden under the skeleton, so its first paint (a white page) never shows. After FRAME_SHOW_MS it shows anyway.
   */
  const [ready, setReady] = useState(false)
  const { theme } = useTheme()
  const themeRef = useRef(theme)
  useEffect(() => {
    themeRef.current = theme
  })
  // a frame that never says (an old server, a slow page) shows after a moment all the same
  useEffect(() => {
    if (ready || !view.data) return
    const timer = setTimeout(() => setReady(true), FRAME_SHOW_MS)
    return () => clearTimeout(timer)
  }, [ready, view.data])
  const reportBlocked = useRef(onBlockedImages)
  useEffect(() => {
    reportBlocked.current = onBlockedImages
  })

  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      const message = frameMessage(event, frame.current?.contentWindow)
      if (message === null) return
      setHeight(message.height)
      setPlain(message.plain)
      if (!message.plain || message.theme === themeRef.current) setReady(true)
      if (message.blocked !== null) reportBlocked.current?.(message.blocked)
    }
    window.addEventListener('message', onMessage)
    return () => window.removeEventListener('message', onMessage)
  }, [])

  // write only: the frame's origin is opaque, so the target is `*`, and nothing is read back
  const postTheme = useCallback(() => {
    const element = frame.current
    if (!element) return
    // the frame paints the pane's colour itself (read when the theme is sent, after the theme change took effect)
    const background = backgroundBehind(element.parentElement, (node) => getComputedStyle(node).backgroundColor)
    element.contentWindow?.postMessage(frameThemeMessage(theme, background), '*')
  }, [theme])
  useEffect(() => {
    if (plain) postTheme()
  }, [plain, postTheme])

  if (view.isError) {
    return (
      <div role="alert" className="flex items-center gap-3 rounded-md border px-3 py-2 text-[0.8125rem] text-muted-foreground">
        The message could not be shown.
        <Button variant="outline" size="sm" onClick={() => void view.refetch()}>
          Try again
        </Button>
      </div>
    )
  }
  if (!view.data) return <Skeleton className="h-20 w-full" />
  return (
    <div className="relative">
      {ready ? null : <Skeleton aria-hidden className="absolute inset-x-0 top-0 h-20" />}
      <iframe
      ref={frame}
      data-slot="mail-frame"
      src={view.data.url}
      title={title}
      sandbox="allow-scripts allow-popups allow-popups-to-escape-sandbox"
      referrerPolicy="no-referrer"
      onLoad={postTheme}
      // mail with its own colours is written for a white page, also in the dark theme
      className={cn('block w-full rounded-md border-0', plain ? 'bg-transparent' : 'bg-white [color-scheme:light]', !ready && 'invisible')}
      style={{ height }}
      />
    </div>
  )
}
