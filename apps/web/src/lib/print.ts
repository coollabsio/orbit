/**
 * On `<html>` while the browser prints: `@media print` rules in index.css show only the `[data-print-root]` content
 * (a docs page, a mail conversation).
 */
export const PRINT_ATTR = 'data-print-doc'

/**
 * Prints the `[data-print-root]` content (the browser's "Save as PDF" makes the PDF). The print attribute switches the print stylesheet to
 * page-only, light, full-width output; the document title becomes `title` (the default PDF file name). Both
 * are restored after printing.
 */
export function printPage(title: string, win: Window = window): void {
  const root = win.document.documentElement
  const previousTitle = win.document.title
  let restored = false
  const restore = () => {
    if (restored) return
    restored = true
    root.removeAttribute(PRINT_ATTR)
    win.document.title = previousTitle
    win.removeEventListener('afterprint', restore)
  }
  root.setAttribute(PRINT_ATTR, '')
  win.document.title = title.trim() || 'Untitled'
  win.addEventListener('afterprint', restore)
  try {
    win.print()
  } catch {
    restore()
  }
}
