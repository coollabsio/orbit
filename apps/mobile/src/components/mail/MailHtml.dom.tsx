'use dom'

// A message's HTML in the same sandbox as the web's `MailHtmlFrame`: an iframe without `allow-same-origin`, so the mail
// has an opaque origin and cannot reach this page or the app. The native side fetched the document and moved its
// policy into a `<meta>` (`src/data/mail-frame.ts`); here it is framed as `srcdoc`. Only posts from the frame's own
// window count: its size and theme (the server's frame script) and the links it asks to open.
import type { DOMProps } from 'expo/dom'
import { useEffect, useRef, useState } from 'react'

import { FRAME_MIN_HEIGHT, frameEvent } from '../../data/mail-frame'

export interface MailHtmlProps {
  document: string
  theme: 'light' | 'dark'
  /** The pane colour behind a plain mail, which the frame paints itself. */
  background: string
  /** The page of mail with its own colours: white, as the web. */
  paper: string
  onSize: (size: { height: number; plain: boolean; blocked: number | null }) => Promise<void>
  onOpenLink: (url: string) => Promise<void>
  dom?: DOMProps
}

const postFrameTheme = (element: HTMLIFrameElement | null, theme: 'light' | 'dark', background: string) =>
  element?.contentWindow?.postMessage({ orbitMailTheme: theme, orbitMailBackground: background }, '*')

export default function MailHtml({ document: html, theme, background, paper, onSize, onOpenLink }: MailHtmlProps) {
  const frame = useRef<HTMLIFrameElement>(null)
  const [plain, setPlain] = useState(false)
  // a plain mail shows once it has the app theme, so its first paint (white) never shows; any other mail at once
  const [ready, setReady] = useState(false)
  const latest = useRef({ theme, onSize, onOpenLink })
  latest.current = { theme, onSize, onOpenLink }

  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      if (!frame.current?.contentWindow || event.source !== frame.current.contentWindow) return
      const posted = frameEvent(event.data)
      if (!posted) return
      if (posted.kind === 'open') {
        void latest.current.onOpenLink(posted.url)
        return
      }
      setPlain(posted.plain)
      if (!posted.plain || posted.theme === latest.current.theme) setReady(true)
      void latest.current.onSize({ height: posted.height, plain: posted.plain, blocked: posted.blocked })
    }
    window.addEventListener('message', onMessage)
    return () => window.removeEventListener('message', onMessage)
  }, [])

  // write only, to `*`: the frame's origin is opaque, and nothing is read back
  const postTheme = () => postFrameTheme(frame.current, theme, background)
  useEffect(() => {
    if (plain) postFrameTheme(frame.current, theme, background)
  }, [plain, theme, background])
  // a frame that never says (an old server) shows after a moment all the same
  useEffect(() => {
    const timer = setTimeout(() => setReady(true), 1000)
    return () => clearTimeout(timer)
  }, [html])

  return (
    // the web view's #root is a flex row: the root must grow (see .ai/lessons.md)
    <div style={{ flex: 1, margin: 0, background: plain ? background : paper }}>
      <iframe
        ref={frame}
        title="Message"
        srcDoc={html}
        sandbox="allow-scripts allow-popups allow-popups-to-escape-sandbox"
        referrerPolicy="no-referrer"
        onLoad={postTheme}
        style={{
          display: 'block',
          width: '100%',
          minHeight: FRAME_MIN_HEIGHT,
          height: '100vh',
          border: 0,
          background: plain ? 'transparent' : paper,
          colorScheme: plain ? theme : 'light',
          visibility: ready ? 'visible' : 'hidden',
        }}
      />
    </div>
  )
}
