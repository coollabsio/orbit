import { createContext, useCallback, useContext, type Dispatch, type SetStateAction } from 'react'
import { useCommand } from './useCommand'

/** The tasks a task command acts on. An open task wins; in a list it is the selection, else the row with the
 *  keyboard focus, else the row under the pointer. Tasks that are no longer in view never count. */
export function resolveTarget({ selectedIds, focusedId, hoveredId, openTaskId, visible }: { selectedIds: string[]; focusedId: string | null; hoveredId: string | null; openTaskId: string | null; visible: Set<string> }): string[] {
  if (openTaskId) return [openTaskId]
  const selection = selectedIds.filter((id) => visible.has(id))
  if (selection.length > 0) return selection
  if (focusedId && visible.has(focusedId)) return [focusedId]
  if (hoveredId && visible.has(hoveredId)) return [hoveredId]
  return []
}

/** The row under the pointer and where the pointer was, kept out of React state: it changes on every move.
 *  `focusedId` is the row with the keyboard focus, remembered while the focus is in a dialog or a menu. */
export interface TaskPointer { hoveredId: string | null; x: number; y: number; focusedId: string | null }

export interface TaskTargetState {
  selectedIds: string[]
  setSelected: Dispatch<SetStateAction<string[]>>
  openTaskId: string | null
  /** The order of the rows when a task was opened, for next/previous in task detail. */
  order: { current: string[] }
}

export const TaskTargetContext = createContext<TaskTargetState>({ selectedIds: [], setSelected: () => {}, openTaskId: null, order: { current: [] } })
/** Apart from the selection, so that a row does not render again each time the selection changes. */
export const TaskPointerContext = createContext<TaskPointer>({ hoveredId: null, x: -1, y: -1, focusedId: null })

const rowOf = (element: Element | null) => element?.closest('[data-task-id]')?.getAttribute('data-task-id') ?? null

/** The ids of the task rows in view, in document order. */
export function visibleTaskIds(): string[] {
  return [...new Set([...document.querySelectorAll('[data-task-id]')].map((element) => element.getAttribute('data-task-id') ?? ''))]
}

/** The task row that holds the keyboard focus. */
export const focusedTaskId = () => rowOf(document.activeElement)

/** Gives a task row the keyboard focus and brings it into view. */
export function focusTaskRow(id: string) {
  const row = document.querySelector<HTMLElement>(`[data-task-id="${CSS.escape(id)}"]`)
  row?.focus({ preventScroll: true })
  // keyboard movement never animates: the row is in view at once
  row?.scrollIntoView?.({ block: 'nearest' })
}

export function useTaskTarget() {
  const state = useContext(TaskTargetContext)
  const { selectedIds, openTaskId } = state
  const pointer = useContext(TaskPointerContext)
  const getTargetIds = useCallback(() => {
    // the command menu takes the focus from the row it was opened on: that row is still the focused one.
    // The menu reads the target when it opens, which is when the row is remembered.
    const focused = focusedTaskId()
    if (focused || !inOverlay(document.activeElement)) pointer.focusedId = focused
    return resolveTarget({ selectedIds, focusedId: pointer.focusedId, hoveredId: pointer.hoveredId, openTaskId, visible: new Set(visibleTaskIds()) })
  }, [openTaskId, pointer, selectedIds])
  return { ...state, pointer, getTargetIds }
}

/** Marks an element as a task row: the keyboard can focus it and the pointer can target it. */
export const taskRowTarget = (id: string) => ({ 'data-task-id': id })

/** Follows the pointer for a tasks page: the row under it is the target of task commands. */
export function trackPointer(pointer: TaskPointer) {
  const onPointerMove = (event: globalThis.PointerEvent) => {
    // a scroll moves rows under a still pointer; only a real movement changes the target
    if (event.clientX === pointer.x && event.clientY === pointer.y) return
    Object.assign(pointer, { x: event.clientX, y: event.clientY })
    const target = event.target instanceof Element ? event.target : null
    const id = rowOf(target)
    if (id) {
      pointer.hoveredId = id
      // the pointer takes over from the keyboard
      const focused = document.activeElement
      if (focused instanceof HTMLElement && rowOf(focused) !== null && rowOf(focused) !== id) focused.blur()
    } else if (!inOverlay(target)) {
      // over a dialog or a menu the row stays the target: the command menu acts on the task it was opened over
      pointer.hoveredId = null
    }
  }
  // a scroll moves the rows under a still pointer: the target is the row that is there now
  const onScroll = () => {
    if (pointer.hoveredId === null || pointer.x < 0) return
    pointer.hoveredId = rowOf(document.elementFromPoint?.(pointer.x, pointer.y) ?? null)
  }
  document.addEventListener('pointermove', onPointerMove)
  document.addEventListener('scroll', onScroll, true)
  return () => {
    document.removeEventListener('pointermove', onPointerMove)
    document.removeEventListener('scroll', onScroll, true)
  }
}

function inOverlay(target: Element | null) {
  const root = document.getElementById('root')
  // overlays render in portals beside the app root
  if (root && target && !root.contains(target)) return true
  return target?.closest('[role="dialog"], [role="menu"], [role="listbox"]') != null
}

/** Board: moves the focus from a card to the nearest card of the next column with cards, left or right. */
export function focusAdjacentColumn(card: HTMLElement, step: 1 | -1) {
  const from = card.getBoundingClientRect()
  const candidates = [...document.querySelectorAll<HTMLElement>('[data-board-card]')]
    .map((element) => ({ element, rect: element.getBoundingClientRect() }))
    .filter(({ rect }) => (step === 1 ? rect.left > from.left + 1 : rect.left < from.left - 1))
  if (candidates.length === 0) return
  const column = candidates.reduce((best, { rect }) => (Math.abs(rect.left - from.left) < Math.abs(best - from.left) ? rect.left : best), candidates[0].rect.left)
  const nearest = candidates
    .filter(({ rect }) => Math.abs(rect.left - column) < 1)
    .reduce((best, item) => (Math.abs(item.rect.top - from.top) < Math.abs(best.rect.top - from.top) ? item : best))
  nearest.element.focus({ preventScroll: true })
  nearest.element.scrollIntoView?.({ block: 'nearest', inline: 'nearest' })
}

/** Mounts the list commands: move the focus, open, and (in the list layout) select rows. */
export function useListNavigation(onOpen: ((id: string) => void) | null, options: { selectable: boolean }) {
  const { setSelected } = useContext(TaskTargetContext)
  const pointer = useContext(TaskPointerContext)
  const enabled = onOpen !== null

  const current = () => {
    const visible = visibleTaskIds()
    const focused = focusedTaskId()
    return { visible, focused, index: focused ? visible.indexOf(focused) : -1 }
  }
  /** Moves the focus by one row and returns the rows it left and entered. Without a focused row it starts at the first. */
  const move = (step: 1 | -1) => {
    const { visible, focused, index } = current()
    if (visible.length === 0) return null
    const next = index === -1 ? visible[0] : visible[Math.min(visible.length - 1, Math.max(0, index + step))]
    focusTaskRow(next)
    return { from: index === -1 ? null : focused, to: next }
  }
  const extend = (step: 1 | -1) => {
    const moved = move(step)
    if (moved) setSelected((selected) => [...new Set([...selected, ...(moved.from ? [moved.from] : []), moved.to])])
  }
  const pointed = () => {
    const { visible, focused, index } = current()
    return index !== -1 ? focused : pointer.hoveredId && visible.includes(pointer.hoveredId) ? pointer.hoveredId : null
  }
  const down = enabled ? () => void move(1) : null
  const up = enabled ? () => void move(-1) : null
  const selectable = enabled && options.selectable

  useCommand('list.down', down)
  useCommand('list.downArrow', down)
  useCommand('list.up', up)
  useCommand('list.upArrow', up)
  useCommand('list.open', enabled ? () => {
    const id = pointed()
    if (id) onOpen(id)
  } : null)
  useCommand('list.select', selectable ? () => {
    const id = pointed()
    if (id) setSelected((selected) => (selected.includes(id) ? selected.filter((item) => item !== id) : [...selected, id]))
  } : null)
  useCommand('list.extendDown', selectable ? () => extend(1) : null)
  useCommand('list.extendUp', selectable ? () => extend(-1) : null)
  useCommand('list.selectAll', selectable ? () => setSelected(visibleTaskIds()) : null)
}
