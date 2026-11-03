import { describe, expect, test } from 'bun:test'
import { internalPageLinkId, isSafeLinkHref, normalizeLinkHref } from './links'

const NUL = String.fromCharCode(0)
const TAB = String.fromCharCode(9)

describe('link policy', () => {
  test('only absolute http(s) and mailto hrefs are safe', () => {
    expect(isSafeLinkHref('https://example.com')).toBe(true)
    expect(isSafeLinkHref('HTTP://example.com/a?b#c')).toBe(true)
    expect(isSafeLinkHref('mailto:a@b.co')).toBe(true)
    for (const href of ['javascript:alert(1)', ` java${TAB}script:alert(1)`, `java${NUL}script:x`, 'data:text/html,x', 'vbscript:x', 'ftp://x', 'tel:1', '/docs/1', '#top', 'https:/x', '', null, 3]) {
      expect(isSafeLinkHref(href)).toBe(false)
    }
  })

  test('exact in-app page links /docs/<uuid> are safe; other relative paths are not', () => {
    const id = '0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b'
    expect(isSafeLinkHref(`/docs/${id}`)).toBe(true)
    expect(normalizeLinkHref(`/docs/${id}`)).toBe(`/docs/${id}`)
    expect(internalPageLinkId(`/docs/${id.toUpperCase()}`)).toBe(id)
    for (const href of [
      `/docs/${id}/x`,
      `/docs/${id}?a=1`,
      `/docs/${id}#h`,
      `//docs/${id}`,
      `/docs//${id}`,
      `/docs/${id} `,
      `${TAB}/docs/${id}`,
      `/docs/${id.slice(1)}`,
      '/docs/trash',
      '/docs/import',
      `/tasks/${id}`,
      `javascript:/docs/${id}`,
      `https:/docs/${id}`,
    ]) {
      expect(isSafeLinkHref(href)).toBe(false)
      expect(normalizeLinkHref(href)).toBeNull()
      expect(internalPageLinkId(href)).toBeNull()
    }
  })

  test('normalizeLinkHref upgrades bare hosts and rejects everything else', () => {
    expect(normalizeLinkHref(' https://a.io ')).toBe('https://a.io')
    expect(normalizeLinkHref('example.com/path')).toBe('https://example.com/path')
    expect(normalizeLinkHref('example.com:8080/x')).toBe('https://example.com:8080/x')
    expect(normalizeLinkHref('javascript:alert(1)')).toBeNull()
    expect(normalizeLinkHref('JavaScript:1.2')).toBeNull()
    expect(normalizeLinkHref('localhost:3000')).toBeNull()
    expect(normalizeLinkHref('/relative')).toBeNull()
    expect(normalizeLinkHref('#hash')).toBeNull()
    expect(normalizeLinkHref('word')).toBeNull()
    expect(normalizeLinkHref(undefined)).toBeNull()
  })
})
