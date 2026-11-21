/**
 * iOS opens the on-screen keyboard only for a focus inside the tap itself. A field that appears later (after a
 * network call) gets the focus but no keyboard. Call this in the tap: a hidden field takes the focus at once and opens
 * the keyboard; the real field that focuses next keeps it. The hidden field goes away when it loses the focus, or
 * after `ms` when nothing took it.
 */
export function holdKeyboard(ms = 3000) {
  const proxy = document.createElement('input')
  proxy.setAttribute('aria-hidden', 'true')
  proxy.tabIndex = -1
  // 16px: iOS zooms into a smaller field; fixed at the top so the page does not scroll to it
  proxy.style.cssText = 'position:fixed;top:0;left:0;width:1px;height:1px;opacity:0;font-size:16px;border:0;padding:0'
  document.body.append(proxy)
  proxy.focus({ preventScroll: true })
  const remove = () => proxy.remove()
  proxy.addEventListener('blur', remove, { once: true })
  setTimeout(remove, ms)
}
