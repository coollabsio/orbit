import { useHotkeySequences, useHotkeys, type UseHotkeyDefinition, type UseHotkeySequenceDefinition } from '@tanstack/react-hotkeys'
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from 'react'
import { toast } from 'sonner'
import { parseKeys, resolveBindings, toEngineStep, type Bindings, type Overrides } from './bindings'
import { COMMANDS, type Command } from './commands'

/** Longest pause between the two keys of a sequence. */
export const SEQUENCE_TIMEOUT_MS = 1000

const OPEN_POPUP = '[role="menu"]:not([data-closed]), [role="dialog"]:not([data-closed]), [role="alertdialog"]:not([data-closed]), [role="listbox"]:not([data-closed])'
const MODIFIER_KEYS = ['Shift', 'Control', 'Alt', 'Meta', 'AltGraph']
const ALL_COMMANDS: readonly Command[] = COMMANDS

type Handler = { run: () => void }

/** The handlers that features mounted with `useCommand`. The last mounted handler of a command runs. */
export class CommandStore {
  private handlers = new Map<string, Handler[]>()
  private listeners = new Set<() => void>()
  private active: Command[] = []

  add(id: string, handler: Handler) {
    this.handlers.set(id, [...(this.handlers.get(id) ?? []), handler])
    this.changed()
    return () => {
      this.handlers.set(id, (this.handlers.get(id) ?? []).filter((item) => item !== handler))
      this.changed()
    }
  }

  private changed() {
    this.active = ALL_COMMANDS.filter((command) => this.handlers.get(command.id)?.length)
    for (const listener of this.listeners) listener()
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener)
    return () => void this.listeners.delete(listener)
  }

  getActive = () => this.active

  /** Runs the command; false when no handler is mounted. An error in a handler is reported and does not spread. */
  run(id: string): boolean {
    const handler = this.handlers.get(id)?.at(-1)
    if (!handler) return false
    try {
      handler.run()
    } catch (error) {
      console.error(error)
      toast.error(`Could not run "${ALL_COMMANDS.find((command) => command.id === id)?.title ?? id}".`)
    }
    return true
  }
}

const fallbackStore = new CommandStore()
export const CommandStoreContext = createContext<CommandStore>(fallbackStore)
export const BindingsContext = createContext<Bindings>(resolveBindings({}))
/** The first key of a sequence that waits for its second key, in registry notation. */
export const PendingSequenceContext = createContext<string | null>(null)
const SuspendContext = createContext<(suspended: boolean) => void>(() => {})

/** Turns every shortcut off while the calling component is mounted, e.g. while a key recorder listens. */
export function useSuspendShortcuts() {
  const suspend = useContext(SuspendContext)
  useEffect(() => {
    suspend(true)
    return () => suspend(false)
  }, [suspend])
}

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
      if (!pending.current || MODIFIER_KEYS.includes(event.key)) return
      sequenceEvents.current.add(event)
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
      || (command.context !== 'new-task' && command.id !== 'palette.open' && document.querySelector(OPEN_POPUP) !== null)

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
          if (sequenceEvents.current.has(event) || event.defaultPrevented || event.repeat || document.querySelector(OPEN_POPUP)) return
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
