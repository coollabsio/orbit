import { COMMANDS, commandById, type Command, type Context } from './commands'

/** What a user changed: command id → keys, or null for "no shortcut". Stored on the server. */
export type Overrides = Record<string, string | null>
/** The keys in effect for every command. */
export type Bindings = Record<string, string | null>

const MODIFIERS = ['Mod', 'Ctrl', 'Alt', 'Shift']
const NAMED_KEYS = ['Enter', 'Escape', 'Backspace', 'Delete', 'Tab', 'Space', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Home', 'End', 'PageUp', 'PageDown']
/** Keys matched by physical position, so a shortcut survives a non-Latin layout and a held Shift. */
const PUNCTUATION: Record<string, string> = { ',': 'Comma', '.': 'Period', '/': 'Slash', ';': 'Semicolon', "'": 'Quote', '[': 'BracketLeft', ']': 'BracketRight', '\\': 'Backslash', '-': 'Minus', '=': 'Equal', '`': 'Backquote' }
const PUNCTUATION_BY_CODE = Object.fromEntries(Object.entries(PUNCTUATION).map(([key, code]) => [code, key]))

const isKey = (key: string) => /^[A-Z0-9?]$/.test(key) || key in PUNCTUATION || NAMED_KEYS.includes(key) || /^F([1-9]|1[0-2])$/.test(key)

const validStep = (step: string) => {
  const parts = step.split('+')
  const key = parts.pop() ?? ''
  return isKey(key) && parts.every((part) => MODIFIERS.includes(part)) && new Set(parts).size === parts.length
}

/** Splits registry notation into its steps: `G I` → `['G', 'I']`. Null when a step is not a key or there are more than two. */
export function parseKeys(keys: string): string[] | null {
  const steps = keys.split(' ')
  return steps.length <= 2 && steps.every(validStep) ? steps : null
}

/** One step in the notation of the key engine, e.g. `Mod+K` → `Mod+[KeyK]`. */
export function toEngineStep(step: string): string {
  const parts = step.split('+')
  const key = parts.pop() ?? ''
  // `?` has no physical key of its own: it is Shift and the slash key
  if (key === '?') return [...parts.filter((part) => part !== 'Shift'), 'Shift', '[Slash]'].join('+')
  const code = /^[A-Z]$/.test(key) ? `[Key${key}]` : /^[0-9]$/.test(key) ? `[Digit${key}]` : key in PUNCTUATION ? `[${PUNCTUATION[key]}]` : key
  return [...parts, code].join('+')
}

/** Recorder output back to registry notation; a recording longer than two steps keeps the first two.
 *  The main modifier of the platform (⌘ on macOS, Ctrl elsewhere) becomes `Mod`, as in the defaults. */
export function fromRecorded(steps: string[], mac = false): string {
  const modifier = (part: string) => {
    if (part === 'Control' || part === 'Ctrl') return mac ? 'Ctrl' : 'Mod'
    if (part === 'Meta' || part === 'Command' || part === 'Cmd') return mac ? 'Mod' : 'Meta'
    return part
  }
  return steps.slice(0, 2).map((step) => {
    const parts = step.split('+')
    const key = parts.pop() ?? ''
    const code = /^\[(.+)\]$/.exec(key)?.[1]
    const plain = !code ? key : /^Key[A-Z]$/.test(code) ? code.slice(3) : /^Digit[0-9]$/.test(code) ? code.slice(5) : PUNCTUATION_BY_CODE[code] ?? code
    // in the order the registry writes them
    const modifiers = MODIFIERS.filter((name) => parts.map(modifier).includes(name))
    if (plain === '/' && modifiers.includes('Shift')) return [...modifiers.filter((part) => part !== 'Shift'), '?'].join('+')
    return [...modifiers, plain].join('+')
  }).join(' ')
}

/** Defaults with the user's valid overrides on top. Unknown ids and unparsable keys are ignored; fixed commands keep their default. */
export function resolveBindings(overrides: Overrides): Bindings {
  const bindings: Bindings = {}
  for (const command of COMMANDS as readonly Command[]) {
    const override = overrides[command.id]
    const usable = !command.fixed && command.id in overrides && (override === null || parseKeys(override) !== null)
    bindings[command.id] = usable ? override : command.keys
  }
  return bindings
}

const LIST_CONTEXTS: Context[] = ['task-list', 'task-board', 'timeline', 'views']

/** Whether commands of the two contexts can be active together. */
export function contextsOverlap(a: Context, b: Context): boolean {
  if (a === b || a === 'global' || b === 'global') return true
  const inList = (context: Context) => LIST_CONTEXTS.includes(context)
  if (a === 'task-target') return inList(b) || b === 'task-detail'
  if (b === 'task-target') return inList(a) || a === 'task-detail'
  return inList(a) && inList(b)
}

const isPrefix = (short: string[], long: string[]) => short.every((step, index) => long[index] === step)

/** Every command that `keys` would collide with if `id` took them: the same keys, or one a prefix of the other's sequence. */
export function findConflicts(bindings: Bindings, id: string, keys: string): Command[] {
  const command = commandById(id)
  const steps = parseKeys(keys)?.map(toEngineStep)
  if (!command || !steps) return []
  return (COMMANDS as readonly Command[]).filter((other) => {
    const otherKeys = bindings[other.id]
    if (other.id === id || !otherKeys || !contextsOverlap(command.context, other.context)) return false
    const otherSteps = parseKeys(otherKeys)?.map(toEngineStep)
    return otherSteps !== undefined && (isPrefix(steps, otherSteps) || isPrefix(otherSteps, steps))
  })
}

/** The first command that `keys` would collide with, or null. */
export const findConflict = (bindings: Bindings, id: string, keys: string): Command | null => findConflicts(bindings, id, keys)[0] ?? null

/** Overrides after `id` takes `keys`. With `swap`, the command that had those keys takes the previous keys of `id`. */
export function applyRebind(overrides: Overrides, id: string, keys: string | null, swap: boolean): Overrides {
  const bindings = resolveBindings(overrides)
  const next: Overrides = { ...overrides, [id]: keys }
  const conflict = swap && keys ? findConflict(bindings, id, keys) : null
  if (conflict) next[conflict.id] = bindings[id]
  for (const key of Object.keys(next)) if (next[key] === commandById(key)?.keys) delete next[key]
  return next
}

const RESERVED = ['Mod+W', 'Mod+T', 'Mod+N', 'Mod+Q', 'Mod+R', 'Mod+L', 'Mod+Shift+T', 'Mod+Shift+N', 'Mod+Shift+W']

/** Keys the browser keeps for itself, or a step with no key but modifiers. */
export function isReserved(keys: string): boolean {
  return keys.split(' ').some((step) => RESERVED.includes(step) || step.split('+').every((part) => MODIFIERS.includes(part)))
}

/** A value for `aria-keyshortcuts`. A sequence has no ARIA form. */
export function shortcutLabel(keys: string | null): string | undefined {
  const steps = keys ? parseKeys(keys) : null
  if (!steps || steps.length !== 1) return undefined
  const step = steps[0].replace('Ctrl+', 'Control+')
  return step.includes('Mod+') ? `${step.replace('Mod+', 'Control+')} ${step.replace('Mod+', 'Meta+')}` : step
}
