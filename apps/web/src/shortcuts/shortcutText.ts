import { formatForDisplay } from '@tanstack/react-hotkeys'
import { parseKeys, toEngineStep } from './bindings'
import type { CommandId } from './commands'
import { useBindings } from './useCommand'

/** The key caps of one step: `Mod+Shift+K` → ⌘ ⇧ K on macOS, Ctrl Shift K elsewhere. */
export function stepParts(step: string): string[] {
  // `?` is matched as Shift and the slash key, but reads as one key
  if (step.endsWith('?')) return [...formatForDisplay(`${step.slice(0, -1)}[Slash]`, { parts: true }).slice(0, -1), '?']
  return formatForDisplay(toEngineStep(step), { parts: true })
}

/** Keys as plain text for a `title`: `⌘K`, `Ctrl+K`, or `G then I` for a sequence. */
export function shortcutText(keys: string): string {
  const mac = formatForDisplay('Mod+[KeyK]', { parts: true })[0] === '⌘'
  return (parseKeys(keys) ?? []).map((step) => stepParts(step).join(mac ? '' : '+')).join(' then ')
}

/** A label with the current keys of a command in parentheses: `New task (C)`. Just the label without a shortcut. */
export function useShortcutTitle(id: CommandId, label: string): string {
  const keys = useBindings()[id]
  return keys ? `${label} (${shortcutText(keys)})` : label
}
