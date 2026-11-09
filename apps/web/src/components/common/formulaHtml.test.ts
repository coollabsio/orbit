import { describe, expect, test } from 'bun:test'
import { formulaHtml } from './formulaHtml'

describe('formulaHtml', () => {
  test('draws a display formula', () => {
    const formula = formulaHtml('e^{i\\pi} + 1 = 0')
    expect(formula).toHaveProperty('html')
    if (!('html' in formula)) return
    expect(formula.html).toContain('katex-display')
    expect(formula.html).not.toContain('katex-error')
    const inline = formulaHtml('e^{i\\pi} + 1 = 0', false)
    expect(inline).toHaveProperty('html')
    if ('html' in inline) expect(inline.html).not.toContain('katex-display')
  })

  test('joins lines into one formula and ignores blank source', () => {
    const formula = formulaHtml('a\n+\nb')
    expect(formula).toEqual(formulaHtml('a + b'))
    expect(formulaHtml('  \n')).toEqual({ html: '' })
  })

  test('a bad formula is a short error', () => {
    const formula = formulaHtml('\\frac')
    expect(formula).toHaveProperty('error')
    if (!('error' in formula)) return
    expect(formula.error.length).toBeGreaterThan(0)
    expect(formula.error.startsWith('KaTeX')).toBe(false)
  })
})
