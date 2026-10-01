import { useCallback, useContext, useEffect, useRef, useSyncExternalStore } from 'react'
import { shortcutLabel, type Bindings } from './bindings'
import type { Command, CommandId } from './commands'
import { BindingsContext, CommandStoreContext, PendingSequenceContext, SuspendContext } from './context'

/** Gives a command its action while the calling component is mounted. `null` or `enabled: false` leaves it inactive:
 *  its key does nothing and the command menu does not list it. */
export function useCommand(id: CommandId, handler: (() => void) | null, options?: { enabled?: boolean }) {
  const store = useContext(CommandStoreContext)
  const latest = useRef(handler)
  useEffect(() => {
    latest.current = handler
  })
  const active = handler !== null && options?.enabled !== false
  useEffect(() => {
    if (!active) return
    return store.add(id, { run: () => latest.current?.() })
  }, [active, id, store])
}

/** The commands that have a mounted handler, in registry order. */
export function useActiveCommands(): Command[] {
  const store = useContext(CommandStoreContext)
  return useSyncExternalStore(store.subscribe, store.getActive)
}

/** Runs a command by id, as the command menu does. Does nothing when the command is inactive. */
export function useRunCommand(): (id: CommandId) => void {
  const store = useContext(CommandStoreContext)
  return useCallback((id) => void store.run(id), [store])
}

/** The keys in effect for every command. */
export const useBindings = (): Bindings => useContext(BindingsContext)

/** The first key of a sequence that waits for its second key, or null. */
export const usePendingSequence = (): string | null => useContext(PendingSequenceContext)

/** Turns every shortcut off while the calling component is mounted, e.g. while a key recorder listens. */
export function useSuspendShortcuts() {
  const suspend = useContext(SuspendContext)
  useEffect(() => {
    suspend(true)
    return () => suspend(false)
  }, [suspend])
}

/** `aria-keyshortcuts` for the current keys of a command. */
export const useShortcutLabel = (id: CommandId) => shortcutLabel(useBindings()[id])
