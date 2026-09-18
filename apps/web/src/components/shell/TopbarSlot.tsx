import { createContext, useCallback, useContext, useState, type ReactNode } from 'react'
import { createPortal } from 'react-dom'

export type TopbarSlotSide = 'left' | 'right'

type SlotTargets = Record<TopbarSlotSide, HTMLElement | null>
type SetSlotTarget = (side: TopbarSlotSide, node: HTMLElement | null) => void

/**
 * Two contexts on purpose. The setter must keep a stable identity, because it is the only
 * dependency of the slot ref callback: if that callback's identity changed with the targets,
 * React would detach and reattach the ref forever (attach -> setState -> new callback -> detach).
 */
const TopbarSlotTargetsContext = createContext<SlotTargets | null>(null)
const TopbarSlotSetterContext = createContext<SetSlotTarget | null>(null)

/** Holds the topbar slot containers so pages can portal their own chrome into them. */
export function TopbarSlotProvider({ children }: { children: ReactNode }) {
  const [targets, setTargets] = useState<SlotTargets>({ left: null, right: null })
  const setTarget = useCallback<SetSlotTarget>((side, node) => {
    setTargets((current) => (current[side] === node ? current : { ...current, [side]: node }))
  }, [])
  return (
    <TopbarSlotSetterContext.Provider value={setTarget}>
      <TopbarSlotTargetsContext.Provider value={targets}>{children}</TopbarSlotTargetsContext.Provider>
    </TopbarSlotSetterContext.Provider>
  )
}

/**
 * Ref callback for a slot container. The target is kept in provider state, not a ref, so that
 * slots mounted before the container re-render once the container attaches.
 */
export function useTopbarSlotTarget(side: TopbarSlotSide) {
  const setTarget = useContext(TopbarSlotSetterContext)
  return useCallback((node: HTMLElement | null) => {
    setTarget?.(side, node)
    return () => setTarget?.(side, null)
  }, [setTarget, side])
}

/** Renders page-owned controls inside the shell topbar, from the page's own React tree. */
export function TopbarSlot({ side, children }: { side: TopbarSlotSide; children: ReactNode }) {
  const targets = useContext(TopbarSlotTargetsContext)
  const target = targets?.[side] ?? null
  if (!target) return null
  return createPortal(children, target)
}
