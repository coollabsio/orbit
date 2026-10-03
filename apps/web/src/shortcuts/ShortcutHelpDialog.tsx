import { useState } from 'react'
import { Link } from 'react-router'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import type { Bindings } from './bindings'
import { COMMANDS, type Command, type Group } from './commands'
import { ShortcutKeys } from './Shortcut'
import { useAvailableCommands, useBindings } from './useCommand'

const GROUPS: Group[] = ['General', 'Navigation', 'List', 'Task', 'Docs', 'Chat']

interface Row { title: string; group: Group; keys: string[]; ids: string[] }

/** One row per action: commands that do the same thing (J and the down arrow) share a row. */
function shortcutRows(bindings: Bindings): Row[] {
  const rows = new Map<string, Row>()
  for (const command of COMMANDS as readonly Command[]) {
    const keys = bindings[command.id]
    if (!keys) continue
    const key = `${command.group}:${command.context}:${command.title}`
    const row = rows.get(key) ?? { title: command.title, group: command.group, keys: [], ids: [] }
    rows.set(key, { ...row, keys: [...row.keys, keys], ids: [...row.ids, command.id] })
  }
  return [...rows.values()]
}

function Rows({ label, rows }: { label: string; rows: Row[] }) {
  return (
    <section role="group" aria-label={label} className="mb-5 break-inside-avoid">
      <h3 className="mb-1 text-xs font-medium text-muted-foreground">{label}</h3>
      <ul>
        {rows.map((row) => (
          <li key={row.ids.join()} className="flex min-h-8 items-center justify-between gap-4 text-[13px]">
            <span className="truncate">{row.title}</span>
            <span className="flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground/70">
              {row.keys.map((keys, index) => (
                <span key={keys} className="contents">
                  {index > 0 ? 'or' : null}
                  <ShortcutKeys keys={keys} />
                </span>
              ))}
            </span>
          </li>
        ))}
      </ul>
    </section>
  )
}

/** Every shortcut with its current keys. A rare visit, so it opens with the dialog's normal motion. */
export function ShortcutHelpDialog({ onClose }: { onClose: () => void }) {
  const bindings = useBindings()
  const [query, setQuery] = useState('')
  // what the page in view can do, read once: the dialog itself must not change the list
  const available = useAvailableCommands()
  const [onPage] = useState(() => new Set(available().filter((command) => command.context !== 'global').map((command) => command.id)))

  const needle = query.trim().toLowerCase()
  const rows = shortcutRows(bindings).filter((row) => !needle || row.title.toLowerCase().includes(needle) || row.keys.some((keys) => keys.toLowerCase().includes(needle)))
  const pageRows = rows.filter((row) => row.ids.some((id) => onPage.has(id)))

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="flex max-h-[80vh] flex-col gap-0 p-0 sm:max-w-3xl">
        <DialogHeader className="flex-row items-center gap-4 border-b py-3 pr-12 pl-4">
          <DialogTitle>Keyboard shortcuts</DialogTitle>
          <DialogDescription className="sr-only">All keyboard shortcuts and their current keys.</DialogDescription>
          <Input type="search" aria-label="Search shortcuts" placeholder="Search shortcuts…" autoFocus value={query} onChange={(event) => setQuery(event.target.value)} className="ml-auto h-7 w-56 max-sm:w-40" />
        </DialogHeader>
        <div className="min-h-0 flex-1 overflow-y-auto px-4 pt-4">
          {rows.length === 0 ? <p className="py-10 text-center text-[13px] text-muted-foreground">No shortcut matches "{query.trim()}".</p> : null}
          <div className="gap-x-10 sm:columns-2">
            {pageRows.length > 0 ? <Rows label="On this page" rows={pageRows} /> : null}
            {GROUPS.map((group) => {
              const groupRows = rows.filter((row) => row.group === group && !pageRows.includes(row))
              return groupRows.length > 0 ? <Rows key={group} label={group} rows={groupRows} /> : null
            })}
          </div>
        </div>
        <div className="border-t px-4 py-2.5 text-[13px]">
          <Link to="/profile/shortcuts" className="text-muted-foreground underline-offset-4 hover:text-foreground hover:underline" onClick={onClose}>Customize shortcuts</Link>
        </div>
      </DialogContent>
    </Dialog>
  )
}
