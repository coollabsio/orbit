import type { Property } from '../types'

/**
 * A small tokenizer for the formula editor (spec P3 15.3): it highlights and converts property names ↔ ids in
 * `prop("…")`. It does not evaluate; the server parses, checks and runs formulas.
 */
export type FormulaToken = {
  kind: 'prop' | 'function' | 'text' | 'number' | 'keyword' | 'operator' | 'name' | 'space' | 'unknown'
  text: string
  /** UTF-16 offsets, as the server's error `start` and `end`. */
  start: number
  end: number
  /** `prop`: the property it names; `null` when no property has that name or id. */
  property?: Property | null
}

const KEYWORDS = new Set(['true', 'false', 'and', 'or', 'not'])
const OPERATORS = ['==', '!=', '<=', '>=', '&&', '||', '+', '-', '*', '/', '%', '^', '<', '>', '!', '?', ':', '(', ')', '[', ']', ',', '.']

export function tokenize(expression: string, properties: readonly Property[] = []): FormulaToken[] {
  const tokens: FormulaToken[] = []
  let at = 0
  const push = (kind: FormulaToken['kind'], end: number) => {
    tokens.push({ kind, text: expression.slice(at, end), start: at, end })
    at = end
  }
  while (at < expression.length) {
    const rest = expression.slice(at)
    const space = /^\s+/.exec(rest)
    const number = /^\d+(\.\d+)?([eE]-?\d+)?/.exec(rest)
    const name = /^[A-Za-z_][A-Za-z0-9_]*/.exec(rest)
    if (space) push('space', at + space[0].length)
    else if (number) push('number', at + number[0].length)
    else if (rest[0] === '"') push('text', at + stringLength(rest))
    else if (name) {
      const after = expression.slice(at + name[0].length).trimStart()
      push(KEYWORDS.has(name[0]) ? 'keyword' : after.startsWith('(') ? 'function' : 'name', at + name[0].length)
    } else {
      const operator = OPERATORS.find((symbol) => rest.startsWith(symbol))
      push(operator ? 'operator' : 'unknown', at + (operator?.length ?? 1))
    }
  }
  return markProps(tokens, properties)
}

/** The length of the string literal at the start of `rest`, quotes included (to the end when it does not close). */
function stringLength(rest: string): number {
  for (let at = 1; at < rest.length; at++) {
    if (rest[at] === '\\') at++
    else if (rest[at] === '"') return at + 1
  }
  return rest.length
}

/** The text of a string literal token. */
export function stringValue(token: string): string {
  const inner = token.endsWith('"') && token.length > 1 ? token.slice(1, -1) : token.slice(1)
  return inner.replace(/\\(["\\n])/g, (_, char: string) => (char === 'n' ? '\n' : char))
}

export function quote(text: string): string {
  return `"${text.replace(/\\/g, '\\\\').replace(/"/g, '\\"').replace(/\n/g, '\\n')}"`
}

/** The property a `prop("…")` text names: an id first, then a name (as the server resolves it). */
export function findProperty(reference: string, properties: readonly Property[]): Property | null {
  return properties.find((property) => property.id === reference) ?? properties.find((property) => property.name === reference) ?? null
}

/** Marks each string that is the argument of `prop(` as a `prop` token with its property. */
function markProps(tokens: FormulaToken[], properties: readonly Property[]): FormulaToken[] {
  const solid = tokens.filter((token) => token.kind !== 'space')
  for (let index = 2; index < solid.length; index++) {
    const token = solid[index]
    if (token.kind === 'text' && solid[index - 1].text === '(' && solid[index - 2].text === 'prop') {
      token.kind = 'prop'
      token.property = findProperty(stringValue(token.text), properties)
    }
  }
  return tokens
}

const PROPERTY_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

/** A `prop("<id>")` whose property no longer exists (a name that matches nothing is a typo, not a deletion). */
export function isDeletedProperty(token: FormulaToken): boolean {
  return token.kind === 'prop' && token.property === null && PROPERTY_ID.test(stringValue(token.text))
}

/** The expression as the editor shows it: each `prop("<id>")` names its property. A deleted property's id stays (and shows as missing). */
export function toEditable(expression: string, properties: readonly Property[]): string {
  return tokenize(expression, properties)
    .map((token) => (token.kind === 'prop' && token.property ? quote(token.property.name) : token.text))
    .join('')
}
