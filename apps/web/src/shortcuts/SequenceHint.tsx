import { useEffect, useState } from 'react'
import { parseKeys } from './bindings'
import { ShortcutKeys } from './Shortcut'
import { useActiveCommands, useBindings, usePendingSequence } from './useCommand'

/** How long the first key of a sequence waits before the hint shows. A user who knows the keys never sees it. */
const HINT_DELAY_MS = 400

/** After the first key of a sequence (`G`): the keys that can follow and what they do. */
export function SequenceHint() {
  const pending = usePendingSequence()
  const active = useActiveCommands()
  const bindings = useBindings()
  // the prefix the hint is shown for; it lags behind `pending` by the delay
  const [shown, setShown] = useState<string | null>(null)
  useEffect(() => {
    if (!pending) return
    const timer = setTimeout(() => setShown(pending), HINT_DELAY_MS)
    return () => {
      clearTimeout(timer)
      setShown(null)
    }
  }, [pending])

  if (!shown || shown !== pending) return null
  const next = active.flatMap((command) => {
    const steps = parseKeys(bindings[command.id] ?? '')
    return steps?.length === 2 && steps[0] === shown ? [{ command, key: steps[1] }] : []
  })
  if (next.length === 0) return null
  return (
    <div
      role="status"
      data-slot="sequence-hint"
      className="fixed right-4 bottom-4 z-50 grid grid-cols-[auto_1fr] items-center gap-x-3 gap-y-1.5 rounded-lg border bg-popover px-3 py-2.5 text-[13px] text-popover-foreground shadow-lg transition-opacity duration-125 ease-out starting:opacity-0"
    >
      {next.map(({ command, key }) => (
        <div key={command.id} className="col-span-full grid grid-cols-subgrid items-center">
          <ShortcutKeys keys={key} />
          <span>{command.title}</span>
        </div>
      ))}
    </div>
  )
}
