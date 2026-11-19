/**
 * Writes `text` to the clipboard. Browsers give the Clipboard API only to secure pages (HTTPS or localhost); on a
 * page served over plain HTTP, for example `http://10.0.0.5:8080` on a local network, this copies through a hidden
 * text area and `execCommand('copy')`. Call it in the click handler: both ways need the user's click.
 */
export async function writeClipboard(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text)
      return
    } catch {
      // refused (no permission, or the page lost focus): try the old way
    }
  }
  copyWithTextArea(text)
}

function copyWithTextArea(text: string) {
  const focused = document.activeElement instanceof HTMLElement ? document.activeElement : null
  const area = document.createElement('textarea')
  area.value = text
  area.setAttribute('readonly', '')
  area.style.position = 'fixed'
  area.style.top = '0'
  area.style.left = '0'
  area.style.opacity = '0'
  area.style.pointerEvents = 'none'
  // inside an open dialog, so the dialog's focus trap does not take the selection away
  ;(focused?.closest('[role="dialog"]') ?? document.body).appendChild(area)
  area.focus({ preventScroll: true })
  area.select()
  area.setSelectionRange(0, text.length)
  const copied = document.execCommand('copy')
  area.remove()
  focused?.focus({ preventScroll: true })
  if (!copied) throw new Error('The browser refused to copy.')
}
