import type { CommandId } from '@/shortcuts/commands'
import { Shortcut, ShortcutKeys } from '@/shortcuts/Shortcut'

const HINTS: { ids: CommandId[]; label: string }[] = [
  { ids: ['pull.nextFile', 'pull.prevFile'], label: 'files' },
  { ids: ['pull.nextThread', 'pull.prevThread'], label: 'threads' },
  { ids: ['pull.toggleViewed'], label: 'viewed' },
  { ids: ['pull.comment'], label: 'comment' },
  { ids: ['pull.review'], label: 'review' },
]

/** The few keys that make a review fast, always in sight on a wide screen. */
export function PullKeyHints() {
  return <footer aria-label="Keyboard shortcuts" className="flex h-8 shrink-0 items-center gap-4 border-t px-3 text-[11px] text-muted-foreground max-[899px]:hidden">
    {HINTS.map((hint) => <span key={hint.label} className="flex items-center gap-1.5">{hint.ids.map((id) => <Shortcut key={id} id={id} />)}{hint.label}</span>)}
    <span className="ml-auto flex items-center gap-1.5"><ShortcutKeys keys="Escape" />close</span>
    <span className="flex items-center gap-1.5"><Shortcut id="help.open" />all shortcuts</span>
  </footer>
}
