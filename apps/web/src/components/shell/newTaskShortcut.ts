import { useBareKeyShortcut } from './bareKeyShortcut'

/**
 * Linear creates an issue with a bare `c` from anywhere. This replaces the topbar New dropdown:
 * a many-times-daily action earns a keystroke rather than permanent chrome.
 */
export const NEW_TASK_KEY = 'c'

/** Document-level `c` handler that starts a new task. */
export function useNewTaskShortcut(onNewTask: () => void) {
  useBareKeyShortcut(NEW_TASK_KEY, onNewTask)
}
