import { useEffect } from 'react'

export function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  if (target.isContentEditable) return true
  const tag = target.tagName
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT'
}

/**
 * Bare printable keys are the app's global shortcuts (Linear uses `[` and `c`). They only count
 * outside text entry and without modifiers, so they never steal a character the user meant to type.
 */
export function isBareKeyShortcut(event: KeyboardEvent, key: string): boolean {
  if (event.key !== key) return false
  if (event.metaKey || event.ctrlKey || event.altKey) return false
  if (event.defaultPrevented) return false
  return !isEditableTarget(event.target)
}

/** Document-level handler for one bare printable key. */
export function useBareKeyShortcut(key: string, onFire: () => void) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isBareKeyShortcut(event, key)) return
      event.preventDefault()
      onFire()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [key, onFire])
}
