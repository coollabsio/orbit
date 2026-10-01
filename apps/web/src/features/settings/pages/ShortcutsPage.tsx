import { useState } from 'react'
import { Danger, RotateLeft } from 'reicon-react'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { applyRebind, findConflict, findConflicts, isReserved, parseKeys, resolveBindings } from '@/shortcuts/bindings'
import { COMMANDS, type Command, type Group } from '@/shortcuts/commands'
import { KeyRecorder } from '@/shortcuts/KeyRecorder'
import { ShortcutKeys } from '@/shortcuts/Shortcut'
import { useShortcutBindings } from '@/shortcuts/useShortcutBindings'

const GROUPS: Group[] = ['General', 'Navigation', 'List', 'Task', 'Docs']
const ALL: readonly Command[] = COMMANDS

/** What the page says under a row after a recording that could not be saved as it is. */
type Notice = { id: string; kind: 'reserved' | 'invalid' } | { id: string; kind: 'conflict'; keys: string; other: Command; more: number }

/** Account view: every shortcut with its keys. A key is a button: press it, then press the new keys. */
export function ShortcutsPage() {
  const { overrides, save } = useShortcutBindings()
  const bindings = resolveBindings(overrides)
  const [query, setQuery] = useState('')
  const [recording, setRecording] = useState<string | null>(null)
  const [notice, setNotice] = useState<Notice | null>(null)

  const needle = query.trim().toLowerCase()
  const shown = ALL.filter((command) => !needle || command.title.toLowerCase().includes(needle) || (bindings[command.id] ?? '').toLowerCase().includes(needle))
  const changed = Object.keys(overrides).length > 0

  const record = (command: Command, keys: string) => {
    setRecording(null)
    if (keys === bindings[command.id]) return
    // a key the app has no name for (the numeric keypad, a special key of one layout)
    if (parseKeys(keys) === null) return setNotice({ id: command.id, kind: 'invalid' })
    if (isReserved(keys)) return setNotice({ id: command.id, kind: 'reserved' })
    const [other, ...rest] = findConflicts(bindings, command.id, keys)
    if (other) return setNotice({ id: command.id, kind: 'conflict', keys, other, more: rest.length })
    save(applyRebind(overrides, command.id, keys, false))
  }
  const start = (id: string) => {
    setNotice(null)
    setRecording(id)
  }

  return (
    <SettingsCard
      title="Keyboard shortcuts"
      description="Select a shortcut, then press the new keys. Press two keys in a row for a sequence."
      actions={
        <>
          <Input type="search" aria-label="Search shortcuts" placeholder="Search shortcuts…" value={query} onChange={(event) => setQuery(event.target.value)} className="h-8 w-52 max-sm:w-36" />
          {changed ? (
            <Button
              type="button"
              variant="outline"
              onClick={async () => {
                if (await confirmAction({ title: 'Reset all shortcuts?', description: 'Every shortcut goes back to its default keys.', confirmLabel: 'Reset all' })) save({})
              }}
            >
              Reset all shortcuts
            </Button>
          ) : null}
        </>
      }
      flush
    >
      {shown.length === 0 ? <p className="px-4 py-10 text-center text-[13px] text-muted-foreground">No shortcut matches "{query.trim()}".</p> : null}
      {GROUPS.map((group) => {
        const commands = shown.filter((command) => command.group === group)
        if (commands.length === 0) return null
        return (
          <section key={group} aria-label={group} className="border-t first:border-t-0">
            <h4 className="px-4 pt-3 pb-1 text-xs font-medium text-muted-foreground">{group}</h4>
            <ul className="divide-y">
              {commands.map((command) => {
                const keys = bindings[command.id]
                const rowNotice = notice?.id === command.id ? notice : null
                // a default that another command's override took over
                const clash = keys && !rowNotice ? findConflict(bindings, command.id, keys) : null
                return (
                  <li key={command.id} aria-label={command.title} className="px-4 py-1.5 text-[13px]">
                    {/* a fixed height, so a row does not move when its recorder or keys change */}
                    <div className="flex h-8 items-center gap-2">
                      <span className="min-w-0 flex-1 truncate">{command.title}</span>
                      {command.id in overrides && !command.fixed ? (
                        <Button type="button" variant="ghost" size="icon-xs" className="animate-in text-muted-foreground/70 duration-150 ease-out fade-in-0" aria-label={`Reset ${command.title}`} title="Reset to the default keys" onClick={() => save(applyRebind(overrides, command.id, command.keys, false))}>
                          <RotateLeft />
                        </Button>
                      ) : null}
                      {command.fixed ? (
                        <span title="This key cannot be changed" className="flex h-7 items-center px-2 opacity-70">{keys ? <ShortcutKeys keys={keys} /> : null}</span>
                      ) : recording === command.id ? (
                        <KeyRecorder onRecord={(next) => record(command, next)} onCancel={() => setRecording(null)} onClear={() => { setRecording(null); save(applyRebind(overrides, command.id, null, false)) }} />
                      ) : (
                        <Button type="button" variant="ghost" className="h-7 min-w-28 justify-end px-2 text-xs font-normal text-muted-foreground active:scale-[0.97] motion-reduce:active:scale-100" aria-label={`Change shortcut for ${command.title}`} onClick={() => start(command.id)}>
                          {keys ? <ShortcutKeys keys={keys} /> : 'No shortcut'}
                        </Button>
                      )}
                    </div>
                    {rowNotice?.kind === 'reserved' ? <p role="alert" className="animate-in pb-1.5 text-xs text-destructive duration-150 ease-out fade-in-0">The browser uses this shortcut. Choose other keys.</p> : null}
                    {rowNotice?.kind === 'invalid' ? <p role="alert" className="animate-in pb-1.5 text-xs text-destructive duration-150 ease-out fade-in-0">These keys cannot be used. Choose other keys.</p> : null}
                    {rowNotice?.kind === 'conflict' ? (
                      <div role="alert" className="flex animate-in flex-wrap items-center gap-2 pb-1.5 text-xs text-muted-foreground duration-150 ease-out fade-in-0">
                        <ShortcutKeys keys={rowNotice.keys} />
                        {/* a swap is an exchange of two shortcuts: not with a fixed one, and not with several at once */}
                        <span>Used by "{rowNotice.other.title}"{rowNotice.more > 0 ? ` and ${rowNotice.more} more` : ''}.{rowNotice.other.fixed ? ' That shortcut cannot be changed.' : ''}</span>
                        {rowNotice.other.fixed || rowNotice.more > 0 ? null : (
                          <Button type="button" size="xs" variant="outline" onClick={() => { save(applyRebind(overrides, command.id, rowNotice.keys, true)); setNotice(null) }}>Swap</Button>
                        )}
                        <Button type="button" size="xs" variant="ghost" onClick={() => setNotice(null)}>Cancel</Button>
                      </div>
                    ) : null}
                    {clash ? (
                      <p className="flex items-center gap-1.5 pb-1.5 text-xs text-muted-foreground">
                        <Danger aria-hidden className="size-3.5 text-destructive" />
                        Also used by "{clash.title}". Change one of them.
                      </p>
                    ) : null}
                  </li>
                )
              })}
            </ul>
          </section>
        )
      })}
    </SettingsCard>
  )
}
