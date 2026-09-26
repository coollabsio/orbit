import { toast } from 'sonner'
import { apiClient, type createApiClient } from '@/api/client'
import { exportPage } from '@/api/generated/sdk.gen'
import { ApiProblem } from '@/api/problem'

type ApiClient = ReturnType<typeof createApiClient>

/** On `<html>` while the browser prints a page: `@media print` rules in index.css show only the page content. */
export const PRINT_CLASS = 'orbit-print-doc'

/**
 * The download name from a Content-Disposition header: the RFC 5987 `filename*` (UTF-8) when present, else the plain
 * `filename`, else `fallback`. Path separators are never kept.
 */
export function dispositionFileName(header: string | null, fallback: string): string {
  if (header) {
    const extended = /filename\*\s*=\s*UTF-8''([^;]+)/i.exec(header)
    if (extended) {
      try {
        return safeName(decodeURIComponent(extended[1].trim()), fallback)
      } catch {
        // Malformed escapes: use the plain name.
      }
    }
    const plain = /filename\s*=\s*"([^"]*)"/i.exec(header) ?? /filename\s*=\s*([^;]+)/i.exec(header)
    if (plain) return safeName(plain[1].trim(), fallback)
  }
  return fallback
}

function safeName(name: string, fallback: string): string {
  const last = name.split(/[\\/]/).pop()?.trim() ?? ''
  return last || fallback
}

/** Saves a blob through a temporary object URL and a download link. */
export function saveBlob(blob: Blob, fileName: string, doc: Document = document): void {
  const url = URL.createObjectURL(blob)
  const link = doc.createElement('a')
  link.href = url
  link.download = fileName
  link.rel = 'noopener'
  link.style.display = 'none'
  doc.body.append(link)
  link.click()
  link.remove()
  // Give the browser a moment to start the download before the URL goes away.
  setTimeout(() => URL.revokeObjectURL(url), 10_000)
}

function exportErrorMessage(error: unknown, name: string): string {
  if (error instanceof ApiProblem) {
    if (error.status === 413) return error.detail
    if (error.status === 404) return `“${name}” is no longer available.`
  }
  return `Could not export “${name}”. Try again.`
}

interface ExportOptions {
  workspaceId: string
  pageId: string
  /** Display title for toasts and the fallback file name. */
  title: string
  includeChildren: boolean
  client?: ApiClient
  save?: (blob: Blob, fileName: string) => void
}

/**
 * Downloads the page (with its sub-pages when asked) as a ZIP of Markdown files. Shows a progress toast that turns
 * into a success or error toast; resolves to whether it worked.
 */
export async function downloadPageMarkdown({
  workspaceId,
  pageId,
  title,
  includeChildren,
  client = apiClient,
  save = saveBlob,
}: ExportOptions): Promise<boolean> {
  const name = title.trim() || 'Untitled'
  const toastId = `page-export-${pageId}`
  toast.loading(includeChildren ? `Exporting “${name}” with sub-pages…` : `Exporting “${name}”…`, { id: toastId })
  try {
    const { data, response } = await exportPage({
      client,
      path: { workspace_id: workspaceId, page_id: pageId },
      query: { format: 'markdown', children: includeChildren },
      parseAs: 'blob',
      throwOnError: true,
    })
    if (!(data instanceof Blob)) throw new Error('Export response was empty.')
    save(data, dispositionFileName(response.headers.get('content-disposition'), `${name}.zip`))
    toast.success(`Exported “${name}”.`, { id: toastId })
    return true
  } catch (error) {
    toast.error(exportErrorMessage(error, name), { id: toastId })
    return false
  }
}

/**
 * Prints the open page (the browser's "Save as PDF" makes the PDF). The print class switches the print stylesheet to
 * page-only, light, full-width output; the document title becomes the page title (the default PDF file name). Both
 * are restored after printing.
 */
export function printPage(title: string, win: Window = window): void {
  const root = win.document.documentElement
  const previousTitle = win.document.title
  let restored = false
  const restore = () => {
    if (restored) return
    restored = true
    root.classList.remove(PRINT_CLASS)
    win.document.title = previousTitle
    win.removeEventListener('afterprint', restore)
  }
  root.classList.add(PRINT_CLASS)
  win.document.title = title.trim() || 'Untitled'
  win.addEventListener('afterprint', restore)
  try {
    win.print()
  } catch {
    restore()
  }
}
