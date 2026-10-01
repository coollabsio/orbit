import { useHotkeySequences, useHotkeys, type UseHotkeyDefinition, type UseHotkeySequenceDefinition } from '@tanstack/react-hotkeys'
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from 'react'
import { parseKeys, resolveBindings, toEngineStep, type Bindings, type Overrides } from './bindings'
import type { Command } from './commands'
import { BindingsContext, CommandStore, CommandStoreContext, PendingSequenceContext, SuspendContext } from './context'

/** Longest pause between the two keys of a sequence. */
const SEQUENCE_TIMEOUT_MS = 1000

const OPEN_POPUP = '[role="menu"]:not([data-closed]), [role="dialog"]:not([data-closed]), [role="alertdialog"]:not([data-closed])'
/** An open menu or dialog owns the keyboard. A list does so only while the focus is in it: a list can be a
 *  lasting part of a page (the page history), and then it must not switch every shortcut off. */
const popupOpen = () => document.querySelector(OPEN_POPUP) !== null || document.activeElement?.closest('[role="listbox"]') != null
const MODIFIER_KEYS = ['Shift', 'Control', 'Alt', 'Meta', 'AltGraph']

const hasModifier = (step: string) => /(^|\+)(Mod|Ctrl|Alt)\+/.test(step)

export function ShortcutProvider({ overrides, children }: { overrides?: Overrides; children: ReactNode }) {
  const [store] = useState(() => new CommandStore())
  const bindings = useMemo(() => resolveBindings(overrides ?? {}), [overrides])
  const [suspended, setSuspended] = useState(false)
  const [pending, setPending] = useState<string | null>(null)

  return (
    <CommandStoreContext value={store}>
      <BindingsContext value={bindings}>
        <SuspendContext value={setSuspended}>
          <PendingSequenceContext value={pending}>
            {suspended ? null : <Engine store={store} bindings={bindings} onPending={setPending} />}
            {children}
          </PendingSequenceContext>
        </SuspendContext>
      </BindingsContext>
    </CommandStoreContext>
  )
}

/** Registers the keys of the active commands with the key engine. The only place that talks to it. */
function Engine({ store, bindings, onPending }: { store: CommandStore; bindings: Bindings; onPending: (step: string | null) => void }) {
  const active = useSyncExternalStore(store.subscribe, store.getActive)
  // keydown events that are the second key of a sequence: they must not also run a single-key command
  const sequenceEvents = useRef(new WeakSet<Event>())
  const pending = useRef<ReturnType<typeof setTimeout> | null>(null)

  const clearPending = useCallback(() => {
    if (pending.current) clearTimeout(pending.current)
    pending.current = null
    onPending(null)
  }, [onPending])

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // a held first key repeats: it is still the first key
      if (!pending.current || event.repeat || MODIFIER_KEYS.includes(event.key)) return
      // a key with Ctrl, ⌘ or Alt is its own shortcut: it ends the sequence and runs
      if (!event.ctrlKey && !event.metaKey && !event.altKey) sequenceEvents.current.add(event)
      clearPending()
    }
    document.addEventListener('keydown', onKeyDown, true)
    return () => {
      document.removeEventListener('keydown', onKeyDown, true)
      clearPending()
    }
  }, [clearPending])

  const { singles, sequences } = useMemo(() => {
    const blocked = (event: KeyboardEvent, command: Command) =>
      event.defaultPrevented
      // a text field keeps its keys, except for the few commands made to work while typing
      || (isEditable(event.target) && !command.inInputs)
      || (command.context !== 'new-task' && command.id !== 'palette.open' && popupOpen())

    const run = (event: KeyboardEvent, commands: Command[]) => {
      const command = commands.find((item) => !blocked(event, item))
      if (!command) return
      // a held key runs most commands once, but the key stays ours: the browser must not act on the repeats
      if (event.repeat && !command.repeat) event.preventDefault()
      else if (store.run(command.id)) event.preventDefault()
    }

    const bySingle = new Map<string, Command[]>()
    const bySequence = new Map<string, { steps: string[]; commands: Command[] }>()
    const prefixes = new Map<string, string>()
    for (const command of active) {
      const keys = bindings[command.id]
      const steps = keys ? parseKeys(keys) : null
      if (!steps) continue
      const engine = steps.map(toEngineStep)
      if (engine.length === 1) {
        bySingle.set(engine[0], [...(bySingle.get(engine[0]) ?? []), command])
      } else {
        const key = engine.join(' ')
        bySequence.set(key, { steps: engine, commands: [...(bySequence.get(key)?.commands ?? []), command] })
        prefixes.set(engine[0], steps[0])
      }
    }

    const singles: UseHotkeyDefinition[] = [...bySingle].map(([hotkey, commands]) => ({
      hotkey: hotkey as UseHotkeyDefinition['hotkey'],
      callback: (event) => {
        if (!sequenceEvents.current.has(event)) run(event, commands)
      },
      // the engine skips text fields unless one of the commands is made for them and the key has a modifier
      options: { ignoreInputs: !(hasModifier(hotkey) && commands.some((command) => command.inInputs)) },
    }))
    for (const [hotkey, step] of prefixes) {
      if (bySingle.has(hotkey)) continue
      singles.push({
        hotkey: hotkey as UseHotkeyDefinition['hotkey'],
        callback: (event) => {
          // also when it came as a second key (`G G`): it then starts a new sequence
          if (event.defaultPrevented || event.repeat || popupOpen()) return
          if (pending.current) clearTimeout(pending.current)
          onPending(step)
          pending.current = setTimeout(clearPending, SEQUENCE_TIMEOUT_MS)
        },
        options: { ignoreInputs: true },
      })
    }
    const sequences: UseHotkeySequenceDefinition[] = [...bySequence.values()].map(({ steps, commands }) => ({
      sequence: steps as UseHotkeySequenceDefinition['sequence'],
      callback: (event) => run(event, commands),
      options: { ignoreInputs: true },
    }))
    return { singles, sequences }
  }, [active, bindings, clearPending, onPending, store])

  // the engine must not prevent a key that our guards then refuse, so the handler prevents it itself
  useHotkeys(singles, { preventDefault: false, stopPropagation: false, conflictBehavior: 'allow' })
  useHotkeySequences(sequences, { preventDefault: false, stopPropagation: false, conflictBehavior: 'allow', timeout: SEQUENCE_TIMEOUT_MS })
  return null
}

function isEditable(target: EventTarget | null) {
  return target instanceof HTMLElement && (target.matches('input, textarea, select') || target.isContentEditable)
}
