/**
 * Sonner's 4s default is too short to read a toast, notice the mistake and reach Undo; every Undo toast uses this
 * (Undo stays reachable while the pointer is over the toast, as Sonner pauses on hover).
 */
export const UNDO_TOAST_DURATION = 8000

const IDENTIFIER_HYPHEN = /\b([A-Z][A-Z0-9]*)-(?=[0-9A-Z])/g

/**
 * Toast text with a word joiner (U+2060) after each task identifier's hyphen, so a wrapping toast never splits
 * "ORB-" from "0D6F". Toasts stay plain strings, like every other toast in the app.
 */
export function keepIdentifiersTogether(text: string): string {
  return text.replace(IDENTIFIER_HYPHEN, '$1-\u2060')
}
