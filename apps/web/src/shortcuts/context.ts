import { createContext } from 'react'
import { toast } from 'sonner'
import { resolveBindings, type Bindings } from './bindings'
import { COMMANDS, type Command } from './commands'

const ALL_COMMANDS: readonly Command[] = COMMANDS

type Handler = { run: () => void; available?: () => boolean }

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

  /** Whether the command can act at this moment, e.g. a task command with a task to act on. */
  isAvailable(id: string): boolean {
    const handler = this.handlers.get(id)?.at(-1)
    return handler !== undefined && (handler.available?.() ?? true)
  }

  /** Runs the command; false when it is not available. An error in a handler is reported and does not spread. */
  run(id: string): boolean {
    const handler = this.handlers.get(id)?.at(-1)
    if (!handler || !this.isAvailable(id)) return false
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
export const SuspendContext = createContext<(suspended: boolean) => void>(() => {})
