import { cn } from 'cn'
import { Kbd, KbdGroup } from '@/components/ui/kbd'
import { parseKeys } from './bindings'
import type { CommandId } from './commands'
import { stepParts } from './shortcutText'
import { useBindings } from './useCommand'

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
