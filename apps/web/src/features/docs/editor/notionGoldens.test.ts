// Validates the server's Notion → BlockNote converter output (golden files written by
// `cargo test -p orbit-server --test notion_convert`) against the real editor schema: every golden must load into
// BlockNote without throwing and come back with the same block tree, props that matter and text.
import { describe, expect, test } from 'bun:test'
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { BlockNoteEditor } from '@blocknote/core'
import { toEditorContent } from './content'
import { EDITOR_BLOCK_TYPES, pageEditorSchema } from './schema'

const GOLDEN_DIR = join(import.meta.dir, '../../../../../server/tests/fixtures/notion/expected')

type Json = Record<string, unknown>

function isRecord(value: unknown): value is Json {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** Block types, nested with `>`: a stable outline of the tree. */
function outline(blocks: unknown[], depth = 0): string[] {
  const out: string[] = []
  for (const block of blocks) {
    if (!isRecord(block)) continue
    out.push(`${'>'.repeat(depth)}${String(block.type)}`)
    if (Array.isArray(block.children)) out.push(...outline(block.children, depth + 1))
  }
  return out
}

/** All inline text of a block tree (paragraph content, links, table cells), in order. */
function text(value: unknown): string {
  if (typeof value === 'string') return value
  if (Array.isArray(value)) return value.map(text).join('')
  if (!isRecord(value)) return ''
  if (value.type === 'text' && typeof value.text === 'string') return value.text
  let out = ''
  if ('content' in value) out += text(value.content)
  if ('rows' in value) out += text(value.rows)
  if ('cells' in value) out += text(value.cells)
  if ('children' in value) out += text(value.children)
  return out
}

/** Props that carry imported data: file URLs, page targets, heading levels, check state, code language, callout emoji, colors. */
function keyProps(blocks: unknown[]): unknown[] {
  const out: unknown[] = []
  for (const block of blocks) {
    if (!isRecord(block) || !isRecord(block.props)) continue
    const props = block.props
    const pick = ['url', 'name', 'pageId', 'level', 'checked', 'language', 'isToggleable', 'emoji', 'textColor', 'backgroundColor']
    out.push(Object.fromEntries(pick.filter((key) => key in props).map((key) => [key, props[key]])))
    if (Array.isArray(block.children)) out.push(...keyProps(block.children))
  }
  return out
}

const goldens = readdirSync(GOLDEN_DIR)
  .filter((name) => name.endsWith('.json'))
  .sort()

describe('Notion converter goldens load into the page editor', () => {
  test('golden files exist', () => {
    expect(goldens.length).toBeGreaterThan(10)
  })

  test('Notion callouts load as callout blocks with their emoji, colors and nested children', () => {
    const golden = JSON.parse(readFileSync(join(GOLDEN_DIR, 'callout.json'), 'utf8')) as { content: unknown[] }
    const editor = BlockNoteEditor.create({ schema: pageEditorSchema, initialContent: golden.content as never })
    const [first, second, , fourth] = editor.document
    expect(first.type).toBe('callout')
    expect(first.props).toEqual({ emoji: '⭐', backgroundColor: 'gray', textColor: 'default' })
    expect(first.children.map((child) => child.type)).toEqual(['paragraph'])
    expect(second.props).toEqual({ emoji: '💡', backgroundColor: 'default', textColor: 'red' })
    expect(fourth.props).toMatchObject({ backgroundColor: 'blue' })
    expect(fourth.children.map((child) => child.type)).toEqual(['bulletListItem'])
  })

  for (const name of goldens) {
    test(name, () => {
      const golden = JSON.parse(readFileSync(join(GOLDEN_DIR, name), 'utf8')) as { content: unknown[] }
      expect(Array.isArray(golden.content)).toBe(true)
      expect(golden.content.length).toBeGreaterThan(0)
      // Every converted block type exists in the editor schema (nothing is dropped on load).
      for (const type of outline(golden.content)) expect(EDITOR_BLOCK_TYPES.has(type.replace(/^>+/, ''))).toBe(true)

      const editor = BlockNoteEditor.create({ schema: pageEditorSchema, initialContent: golden.content as never })
      const document = editor.document as unknown[]
      expect(outline(document)).toEqual(outline(golden.content))
      expect(text(document)).toBe(text(golden.content))
      expect(keyProps(document)).toEqual(keyProps(golden.content))

      // The editor's load path (link and file URL sanitizing) keeps every block.
      const sanitized = toEditorContent(golden.content, EDITOR_BLOCK_TYPES) ?? []
      expect(outline(sanitized)).toEqual(outline(golden.content))
      const loaded = BlockNoteEditor.create({ schema: pageEditorSchema, initialContent: sanitized as never })
      expect(outline(loaded.document as unknown[])).toEqual(outline(golden.content))
    })
  }
})
