// Full-screen PDF preview, like the ImageViewer. pdf.js draws each page into a canvas when it comes near the view:
// no text layer, no links and no PDF scripts, so a file from anyone is safe to show. This module is loaded on demand.
import { useEffect, useRef, useState } from 'react'
import { Download, Xmark as X } from 'reicon-react'
import { getDocument, GlobalWorkerOptions, version, type PDFDocumentProxy, type RenderTask } from 'pdfjs-dist'
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'
import { cn } from 'cn'
import { pdfRenderScale, type Attachment } from '@/lib/attachmentLib'
import { buttonVariants } from '@/components/ui/button'
import { Dialog, DialogContent } from '@/components/ui/dialog'
import { Spinner } from '@/components/ui/spinner'
import { Tip } from '@/components/common/Tip'
import { ViewerBarButton } from './ImageViewer'

GlobalWorkerOptions.workerSrc = workerUrl

/** The image decoders' JavaScript builds that `vite.config.ts` serves (no WebAssembly under the CSP). */
const WASM_URL = `/assets/pdfjs-${version}/wasm/`
/** A longer file shows its first pages; the rest is in the download. */
const PAGE_LIMIT = 100
const MAX_PAGE_WIDTH = 900
/** A page's height for its width before pdf.js reports its size (A4). */
const A4_RATIO = 297 / 210

type Loaded = { status: 'loading' } | { status: 'failed' } | { status: 'ready'; pdf: PDFDocumentProxy }

export function PdfViewer({ attachment, onClose }: { attachment: Attachment; onClose: () => void }) {
  // Closing plays the dialog's exit first; the caller unmounts the viewer once it has finished.
  const [open, setOpen] = useState(true)
  const [loaded, setLoaded] = useState<Loaded>({ status: 'loading' })
  const [width, setWidth] = useState(0)
  const resizeObserver = useRef<ResizeObserver | null>(null)

  useEffect(() => {
    const controller = new AbortController()
    let destroy: (() => Promise<void>) | null = null
    void (async () => {
      try {
        const response = await fetch(attachment.url, { credentials: 'same-origin', signal: controller.signal })
        if (!response.ok) throw new Error(`HTTP ${response.status}`)
        const task = getDocument({ data: new Uint8Array(await response.arrayBuffer()), useWasm: false, wasmUrl: WASM_URL, enableXfa: false })
        destroy = () => task.destroy()
        const pdf = await task.promise
        if (!controller.signal.aborted) setLoaded({ status: 'ready', pdf })
      } catch {
        if (!controller.signal.aborted) setLoaded({ status: 'failed' })
      }
    })()
    return () => {
      controller.abort()
      void destroy?.()
    }
  }, [attachment.url])

  // the pages follow the width of the scrolling area
  const measure = (element: HTMLDivElement | null) => {
    resizeObserver.current?.disconnect()
    if (!element) return
    resizeObserver.current = new ResizeObserver(([entry]) => setWidth(Math.min(entry.contentRect.width, MAX_PAGE_WIDTH)))
    resizeObserver.current.observe(element)
  }

  const pageCount = loaded.status === 'ready' ? loaded.pdf.numPages : 0
  const download = (
    <Tip label="Download PDF" side="bottom">
      <a
        data-slot="image-viewer-button"
        href={attachment.url}
        download={attachment.fileName}
        className={cn(buttonVariants({ variant: 'ghost', size: 'icon-lg' }), 'bg-white/10 text-white hover:bg-white/15 hover:text-white dark:hover:bg-white/15')}
        aria-label="Download PDF"
      >
        <Download className="size-4" />
      </a>
    </Tip>
  )

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) setOpen(false)
      }}
      onOpenChangeComplete={(next) => {
        if (!next) onClose()
      }}
    >
      <DialogContent
        showCloseButton={false}
        aria-label={attachment.fileName}
        className="top-0 right-0 left-0 z-[100] flex h-[var(--app-height,100svh)] max-h-[var(--app-height,100svh)] w-full max-w-none translate-x-0 translate-y-0 flex-col gap-0 overflow-hidden rounded-none bg-black/90 p-0 pt-[env(safe-area-inset-top,0px)] pr-[env(safe-area-inset-right,0px)] pb-[env(safe-area-inset-bottom,0px)] pl-[env(safe-area-inset-left,0px)] text-white ring-0 backdrop-blur-sm duration-150 motion-reduce:animate-none sm:max-w-none"
      >
        <div className="flex h-14 shrink-0 items-center justify-between gap-3 border-b border-white/10 px-4">
          <div className="min-w-0">
            <div className="truncate text-sm font-bold">{attachment.fileName}</div>
            <div className="text-xs font-semibold text-white/55 tabular-nums">
              {pageCount > 0 ? `${pageCount} ${pageCount === 1 ? 'page' : 'pages'}` : 'PDF'}
            </div>
          </div>
          <div className="flex items-center gap-2">
            {download}
            <ViewerBarButton title="Close PDF viewer" aria-label="Close PDF viewer" onClick={() => setOpen(false)}>
              <X className="size-4" />
            </ViewerBarButton>
          </div>
        </div>
        <div ref={measure} className="flex min-h-0 flex-1 flex-col items-center gap-4 overflow-y-auto overscroll-contain p-2 sm:p-6">
          {loaded.status === 'loading' ? (
            <Spinner className="m-auto text-white/70" aria-label="Loading the PDF" />
          ) : loaded.status === 'failed' ? (
            <div className="m-auto flex flex-col items-center gap-3 text-center text-sm text-white/80">
              <p>This PDF cannot be shown here. Download it to open it.</p>
              {download}
            </div>
          ) : (
            <>
              {Array.from({ length: Math.min(pageCount, PAGE_LIMIT) }, (_, index) => (
                <PdfPage key={index} pdf={loaded.pdf} number={index + 1} width={width} />
              ))}
              {pageCount > PAGE_LIMIT ? (
                <p className="pb-4 text-sm text-white/70">
                  The first {PAGE_LIMIT} of {pageCount} pages are shown. Download the PDF to see all of them.
                </p>
              ) : null}
            </>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}

/** One page: its place is kept from its size, and pdf.js draws it once it is near the view. */
function PdfPage({ pdf, number, width }: { pdf: PDFDocumentProxy; number: number; width: number }) {
  const canvas = useRef<HTMLCanvasElement>(null)
  const [pageSize, setPageSize] = useState<{ width: number; height: number } | null>(null)
  const [near, setNear] = useState(false)

  useEffect(() => {
    let cancelled = false
    void pdf.getPage(number).then((page) => {
      const viewport = page.getViewport({ scale: 1 })
      if (!cancelled) setPageSize({ width: viewport.width, height: viewport.height })
    })
    return () => {
      cancelled = true
    }
  }, [pdf, number])

  useEffect(() => {
    const element = canvas.current
    if (!element) return
    const observer = new IntersectionObserver(([entry]) => {
      if (entry.isIntersecting) {
        setNear(true)
        observer.disconnect()
      }
    }, { rootMargin: '200% 0px' })
    observer.observe(element)
    return () => observer.disconnect()
  }, [])

  useEffect(() => {
    if (!near || !pageSize || width <= 0) return
    let task: RenderTask | null = null
    let cancelled = false
    void pdf.getPage(number).then((page) => {
      const element = canvas.current
      if (cancelled || !element) return
      const viewport = page.getViewport({ scale: pdfRenderScale(width, pageSize.width, window.devicePixelRatio) })
      element.width = Math.floor(viewport.width)
      element.height = Math.floor(viewport.height)
      task = page.render({ canvas: element, viewport })
      // a cancelled render (the width changed, the viewer closed) rejects; nothing to show for it
      task.promise.catch(() => undefined)
    })
    return () => {
      cancelled = true
      task?.cancel()
    }
  }, [pdf, number, near, pageSize, width])

  const height = width * (pageSize ? pageSize.height / pageSize.width : A4_RATIO)
  return (
    <canvas
      ref={canvas}
      role="img"
      aria-label={`Page ${number}`}
      className="shrink-0 rounded-sm bg-white shadow-[0_25px_50px_-12px_rgba(0,0,0,0.6)]"
      style={{ width, height }}
    />
  )
}
