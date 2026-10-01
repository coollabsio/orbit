import { formatForDisplay } from '@tanstack/react-hotkeys'
import { cn } from 'cn'
import { Kbd, KbdGroup } from '@/components/ui/kbd'
import { parseKeys, toEngineStep } from './bindings'
import type { CommandId } from './commands'
import { useBindings } from './useCommand'

/** The key caps of one step: `Mod+Shift+K` → ⌘ ⇧ K on macOS, Ctrl Shift K elsewhere. */
function stepParts(step: string): string[] {
  // `?` is matched as Shift and the slash key, but reads as one key
  if (step.endsWith('?')) return [...formatForDisplay(`${step.slice(0, -1)}[Slash]`, { parts: true }).slice(0, -1), '?']
  return formatForDisplay(toEngineStep(step), { parts: true })
}

/** Keys as key caps. The steps of a sequence stand apart. */
export function ShortcutKeys({ keys, className }: { keys: string; className?: string }) {
  const steps = parseKeys(keys)
  if (!steps) return null
  return (
    <KbdGroup data-slot="shortcut" className={cn('gap-1.5', className)}>
      {steps.map((step, index) => (
        <span key={index} className="inline-flex items-center gap-0.5">
          {stepParts(step).map((part) => <Kbd key={part}>{part}</Kbd>)}
        </span>
      ))}
    </KbdGroup>
  )
}

/** The current keys of a command; nothing when it has no shortcut. */
export function Shortcut({ id, className }: { id: CommandId; className?: string }) {
  const keys = useBindings()[id]
  return keys ? <ShortcutKeys keys={keys} className={className} /> : null
}

/** A value for `aria-keyshortcuts`. A sequence has no ARIA form. */
export function shortcutLabel(keys: string | null): string | undefined {
  const steps = keys ? parseKeys(keys) : null
  if (!steps || steps.length !== 1) return undefined
  const step = steps[0].replace('Ctrl+', 'Control+')
  return step.includes('Mod+') ? `${step.replace('Mod+', 'Control+')} ${step.replace('Mod+', 'Meta+')}` : step
}

/** `aria-keyshortcuts` for the current keys of a command. */
export const useShortcutLabel = (id: CommandId) => shortcutLabel(useBindings()[id])
