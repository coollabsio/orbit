import { useSyncExternalStore } from 'react'
import type { AppState } from './types'
import { seedState } from './seed'

let state: AppState = seedState()
const listeners = new Set<() => void>()

export function getState(): AppState {
  return state
}

/** Immutably update the app state and notify subscribers. */
export function updateState(updater: (prev: AppState) => AppState): void {
  state = updater(state)
  for (const listener of listeners) listener()
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

/** Subscribe a component to the whole app state. Select inside the component. */
export function useAppState(): AppState {
  return useSyncExternalStore(subscribe, getState)
}

let idCounter = 1000

export function nextId(prefix: string): string {
  idCounter += 1
  return `${prefix}_${idCounter}`
}
