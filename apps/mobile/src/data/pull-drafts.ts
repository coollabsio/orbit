/**
 * Comment drafts for pull requests (the web keeps them in localStorage). The app has no key-value store for UI state,
 * so drafts live in memory: they survive closing a sheet or leaving a screen, not quitting the app. No React.
 */
const drafts = new Map<string, string>()

export function readDraft(key: string): string {
  return drafts.get(key) ?? ''
}

export function writeDraft(key: string, value: string): void {
  if (value) drafts.set(key, value)
  else drafts.delete(key)
}

export function clearDraft(key: string): void {
  drafts.delete(key)
}
