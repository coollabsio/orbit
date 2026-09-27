// Parity goldens for the server's BlockNote JSON <-> Yjs converter (`apps/server/src/collab/blocknote.rs`).
//
// The server builds a page's collaborative document from `pages.content_json` and projects it back to JSON, so its
// converter must produce exactly what BlockNote + y-prosemirror produce for this editor schema. This test:
// 1. regenerates the schema table the Rust converter reads (`blocknote-schema.json`) from the real `pageEditorSchema`
//    and fails when the checked-in copy drifted (BlockNote upgrade, schema change);
// 2. writes `{ blocks, y_update, expected }` for every Notion golden plus an editor-exercise document:
//    `y_update` = `blocksToYXmlFragment` (what an editor would have), `expected` = `yDocToBlocks` of it. Rust asserts
//    `fragment_to_blocks(y_update) == expected` and that its own `blocks_to_update(blocks)` projects to `expected`;
// 3. loads the Rust-built updates (`rust/<name>.json`, written by `cargo test --test collab_convert`) with
//    `yDocToBlocks` and expects `expected` again.
//
// Regenerate: `UPDATE_COLLAB_GOLDENS=1 bun test src/features/docs/editor/collabParity.test.ts`, then
// `UPDATE_COLLAB_GOLDENS=1 cargo test -p orbit-server --test collab_convert`, then rerun this test.
import { describe, expect, test } from 'bun:test'
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { BlockNoteEditor } from '@blocknote/core'
import { CommentsExtension, DefaultThreadStoreAuth, ThreadStore } from '@blocknote/core/comments'
import { _blocksToProsemirrorNode, blocksToYXmlFragment, yDocToBlocks } from '@blocknote/core/yjs'
import { Transform } from '@tiptap/pm/transform'
import { prosemirrorToYXmlFragment, yXmlFragmentToProseMirrorRootNode } from 'y-prosemirror'
import * as Y from 'yjs'
import { toEditorContent } from './content'
import { EDITOR_BLOCK_TYPES, pageEditorSchema } from './schema'

const SERVER = join(import.meta.dir, '../../../../../server')
const SCHEMA_FILE = join(SERVER, 'src/collab/blocknote-schema.json')
const NOTION_GOLDENS = join(SERVER, 'tests/fixtures/notion/expected')
const COLLAB_GOLDENS = join(SERVER, 'tests/fixtures/collab')
/** Documents with editor-only marks (comments): the server must read them and keep the marks, never write them. */
const MARK_GOLDENS = join(COLLAB_GOLDENS, 'marks')
const FRAGMENT = 'prosemirror'
const UPDATE = process.env.UPDATE_COLLAB_GOLDENS === '1'

type Json = Record<string, unknown>

function editor() {
  return BlockNoteEditor.create({ schema: pageEditorSchema, _tiptapOptions: { injectCSS: false } })
}

/** Schema facts the Rust converter needs: PM attr order and defaults per block, table cell attrs, style kinds. */
function schemaTable() {
  const instance = editor()
  const pm = instance.pmSchema
  const blocks: Record<string, unknown> = {}
  const blockSchema = instance.schema.blockSchema as Record<string, { content: string; propSchema: Json }>
  for (const type of Object.keys(blockSchema).sort()) {
    const spec = blockSchema[type]
    const node = pm.nodes[type]
    if (!node) continue
    const created = node.create()
    const attrs = Object.keys(node.spec.attrs ?? {})
    blocks[type] = {
      content: node.spec.code ? 'plain' : spec.content,
      attrs: attrs.map((name) => ({
        name,
        prop: name in spec.propSchema,
        missing: created.attrs[name] === undefined ? { undefined: true } : { value: created.attrs[name] },
      })),
    }
  }
  const cell = pm.nodes.tableCell.create()
  const styles: Record<string, string> = {}
  const styleSchema = instance.schema.styleSchema as Record<string, { propSchema: string }>
  for (const name of Object.keys(styleSchema).sort()) styles[name] = styleSchema[name].propSchema
  // Custom inline content (mentions): PM attr order and defaults; `text` and `link` are built in.
  const inline: Record<string, unknown> = {}
  const inlineSchema = instance.schema.inlineContentSchema as Record<string, string | { content: string; propSchema: Json }>
  for (const type of Object.keys(inlineSchema).sort()) {
    const spec = inlineSchema[type]
    const node = pm.nodes[type]
    if (typeof spec === 'string' || !node) continue
    const created = node.create()
    inline[type] = {
      content: spec.content,
      attrs: Object.keys(node.spec.attrs ?? {}).map((name) => ({
        name,
        prop: name in spec.propSchema,
        missing: created.attrs[name] === undefined ? { undefined: true } : { value: created.attrs[name] },
      })),
    }
  }
  return {
    blocks,
    inline,
    cell: { attrs: Object.keys(pm.nodes.tableCell.spec.attrs ?? {}), defaults: cell.attrs },
    styles,
    container_attrs: Object.keys(pm.nodes.blockContainer.spec.attrs ?? {}),
  }
}

const EXERCISE_PAGE = '0199a0b0-0000-7000-8000-000000000001'
const EXERCISE_FILE = `/api/v1/workspaces/0199a0b0-0000-7000-8000-00000000000a/pages/${EXERCISE_PAGE}/files/0199a0b0-0000-7000-8000-00000000000f`

/** Everything the editor can produce: styles, links, hard breaks, nesting, every block type, tables with spans. */
const EDITOR_EXERCISE: Json[] = [
  { id: 'ex-h1', type: 'heading', props: { level: 1 }, content: 'Exercise' },
  { id: 'ex-h2', type: 'heading', props: { level: 2, isToggleable: true, textColor: 'blue' }, content: 'Toggle heading', children: [{ id: 'ex-h2-c', type: 'paragraph', content: 'inside' }] },
  { id: 'ex-h3', type: 'heading', props: { level: 3, textAlignment: 'center' }, content: [] },
  {
    id: 'ex-p1',
    type: 'paragraph',
    props: { backgroundColor: 'yellow' },
    content: [
      { type: 'text', text: 'Plain, ', styles: {} },
      { type: 'text', text: 'bold', styles: { bold: true } },
      { type: 'text', text: ' ', styles: {} },
      { type: 'text', text: 'all', styles: { bold: true, italic: true, underline: true, strike: true, code: true } },
      { type: 'text', text: ' red', styles: { textColor: 'red' } },
      { type: 'text', text: ' on green', styles: { backgroundColor: 'green', italic: true } },
      { type: 'text', text: '\nsecond line 🙂 ünïcode\n', styles: {} },
      { type: 'link', href: 'https://example.com/a?b=c#d', content: [{ type: 'text', text: 'a link', styles: {} }, { type: 'text', text: ' bold', styles: { bold: true } }] },
      { type: 'text', text: ' and ', styles: {} },
      { type: 'link', href: `/docs/${EXERCISE_PAGE}`, content: [{ type: 'text', text: 'a page', styles: {} }] },
      { type: 'link', href: 'mailto:someone@example.com', content: [{ type: 'text', text: 'mail', styles: {} }] },
    ],
  },
  { id: 'ex-empty', type: 'paragraph', content: [] },
  {
    id: 'ex-b1',
    type: 'bulletListItem',
    content: 'Bullet',
    children: [
      { id: 'ex-b1-n', type: 'numberedListItem', props: { start: 3 }, content: 'Numbered from three' },
      { id: 'ex-b1-n2', type: 'numberedListItem', content: 'Next' },
      { id: 'ex-b1-c', type: 'checkListItem', props: { checked: true }, content: 'Done', children: [{ id: 'ex-b1-c-c', type: 'checkListItem', content: 'Open' }] },
    ],
  },
  { id: 'ex-toggle', type: 'toggleListItem', content: 'Toggle', children: [{ id: 'ex-toggle-c', type: 'paragraph', content: 'hidden' }] },
  { id: 'ex-quote', type: 'quote', content: [{ type: 'text', text: 'Quoted', styles: { italic: true } }] },
  { id: 'ex-code', type: 'codeBlock', props: { language: 'rust' }, content: 'fn main() {\n    println!("hi");\n}' },
  { id: 'ex-code-empty', type: 'codeBlock', content: '' },
  { id: 'ex-divider', type: 'divider' },
  { id: 'ex-image', type: 'image', props: { url: EXERCISE_FILE, name: 'a.png', caption: 'Caption', previewWidth: 320, textAlignment: 'center' } },
  { id: 'ex-image-ext', type: 'image', props: { url: 'https://example.com/b.png', showPreview: false } },
  { id: 'ex-file', type: 'file', props: { url: EXERCISE_FILE, name: 'report.pdf' } },
  { id: 'ex-file-empty', type: 'file' },
  { id: 'ex-page', type: 'page', props: { pageId: EXERCISE_PAGE } },
  {
    id: 'ex-callout',
    type: 'callout',
    props: { emoji: '🚀', backgroundColor: 'purple', textColor: 'default' },
    content: [{ type: 'text', text: 'Callout ', styles: {} }, { type: 'text', text: 'text', styles: { bold: true } }],
    children: [{ id: 'ex-callout-c', type: 'bulletListItem', content: 'nested' }],
  },
  {
    id: 'ex-table',
    type: 'table',
    content: {
      type: 'tableContent',
      columnWidths: [120, null, 80],
      headerRows: 1,
      headerCols: 1,
      rows: [
        { cells: ['Head', { type: 'tableCell', props: { colspan: 2, backgroundColor: 'blue' }, content: [{ type: 'text', text: 'Wide', styles: { bold: true } }] }] },
        { cells: ['Side', [{ type: 'text', text: 'a\nb', styles: {} }], { type: 'tableCell', props: { textAlignment: 'right', textColor: 'red' }, content: [{ type: 'link', href: 'https://example.com', content: [{ type: 'text', text: 'x', styles: {} }] }] }] },
        { cells: ['', '', ''] },
      ],
    },
  },
  { id: 'ex-last', type: 'paragraph', props: { textAlignment: 'right', textColor: 'gray' }, content: 'The end' },
]

const MENTION_ANN = '0199a0b0-0000-7000-8000-0000000000a1'
const MENTION_BOB = '0199a0b0-0000-7000-8000-0000000000b2'
const mention = (userId: string, name: string) => ({ type: 'mention', props: { userId, name } })

/** @mentions next to text, styles, links, hard breaks, each other, in lists, callouts and table cells. */
const EDITOR_MENTIONS: Json[] = [
  {
    id: 'mn-p1',
    type: 'paragraph',
    content: [
      { type: 'text', text: 'Hi ', styles: {} },
      mention(MENTION_ANN, 'Ann Lee'),
      { type: 'text', text: ' and ', styles: {} },
      mention(MENTION_BOB, 'Bob Stone'),
      { type: 'text', text: ', see ', styles: { bold: true } },
      { type: 'link', href: 'https://example.com', content: [{ type: 'text', text: 'this', styles: {} }] },
      mention(MENTION_ANN, 'Ann Lee'),
    ],
  },
  { id: 'mn-p2', type: 'paragraph', content: [mention(MENTION_BOB, 'Bob Stone'), mention(MENTION_ANN, 'Ann Lee')] },
  {
    id: 'mn-p3',
    type: 'paragraph',
    content: [mention(MENTION_ANN, 'Ann Lee'), { type: 'text', text: '\nnext line', styles: {} }],
  },
  { id: 'mn-empty-name', type: 'paragraph', content: [mention(MENTION_BOB, '')] },
  { id: 'mn-h', type: 'heading', props: { level: 2 }, content: [{ type: 'text', text: 'Owner: ', styles: {} }, mention(MENTION_BOB, 'Bob Stone')] },
  {
    id: 'mn-list',
    type: 'checkListItem',
    content: [mention(MENTION_ANN, 'Ann Lee'), { type: 'text', text: ' reviews', styles: { italic: true } }],
    children: [{ id: 'mn-list-c', type: 'bulletListItem', content: [{ type: 'text', text: 'ask ', styles: {} }, mention(MENTION_BOB, 'Bob Stone')] }],
  },
  { id: 'mn-callout', type: 'callout', props: { emoji: '📣' }, content: [mention(MENTION_ANN, 'Ann Lee')] },
  {
    id: 'mn-table',
    type: 'table',
    content: {
      type: 'tableContent',
      columnWidths: [null, null],
      rows: [{ cells: [[mention(MENTION_ANN, 'Ann Lee')], [{ type: 'text', text: 'with ', styles: {} }, mention(MENTION_BOB, 'Bob Stone')]] }],
    },
  },
  { id: 'mn-code', type: 'codeBlock', content: '@not a mention' },
]

function base64(bytes: Uint8Array): string {
  return Buffer.from(bytes).toString('base64')
}

function fromBase64(text: string): Uint8Array {
  return new Uint8Array(Buffer.from(text, 'base64'))
}

/** A doc with a fixed client id, so the golden bytes are stable. */
function goldenFor(blocks: Json[]) {
  const doc = new Y.Doc()
  doc.clientID = 1
  blocksToYXmlFragment(editor(), blocks as never, doc.getXmlFragment(FRAGMENT))
  const update = Y.encodeStateAsUpdate(doc)
  return { blocks, y_update: base64(update), expected: yDocToBlocks(editor(), doc, FRAGMENT) as unknown as Json[] }
}

function sources(): { name: string; blocks: Json[] }[] {
  const out = readdirSync(NOTION_GOLDENS)
    .filter((name) => name.endsWith('.json'))
    .sort()
    .map((file) => {
      const golden = JSON.parse(readFileSync(join(NOTION_GOLDENS, file), 'utf8')) as { content: unknown[] }
      // Stored content goes through the same sanitizer the server applies before converting.
      return { name: `notion_${file.replace(/\.json$/, '')}`, blocks: (toEditorContent(golden.content, EDITOR_BLOCK_TYPES) ?? []) as Json[] }
    })
  out.push({ name: 'editor_exercise', blocks: EDITOR_EXERCISE })
  out.push({ name: 'editor_mentions', blocks: EDITOR_MENTIONS })
  return out
}

function writeJson(path: string, value: unknown) {
  writeFileSync(path, `${JSON.stringify(value, null, 1)}\n`)
}

describe('collaborative converter parity (server Rust port vs BlockNote + y-prosemirror)', () => {
  test('the schema table matches the editor schema', () => {
    const table = schemaTable()
    if (UPDATE) writeJson(SCHEMA_FILE, table)
    expect(JSON.parse(readFileSync(SCHEMA_FILE, 'utf8'))).toEqual(JSON.parse(JSON.stringify(table)))
  })

  test('the editor exercise uses every block type of the schema', () => {
    const used = new Set<string>()
    const walk = (blocks: Json[]) => {
      for (const block of blocks) {
        used.add(String(block.type))
        if (Array.isArray(block.children)) walk(block.children as Json[])
      }
    }
    walk(EDITOR_EXERCISE)
    expect([...EDITOR_BLOCK_TYPES].filter((type) => !used.has(type))).toEqual([])
  })

  for (const { name, blocks } of sources()) {
    test(`golden ${name}`, () => {
      const golden = goldenFor(blocks)
      const path = join(COLLAB_GOLDENS, `${name}.json`)
      if (UPDATE) {
        mkdirSync(COLLAB_GOLDENS, { recursive: true })
        writeJson(path, golden)
      }
      const stored = JSON.parse(readFileSync(path, 'utf8')) as { blocks: Json[]; y_update: string; expected: Json[] }
      expect(stored.blocks).toEqual(JSON.parse(JSON.stringify(golden.blocks)))
      expect(stored.expected).toEqual(JSON.parse(JSON.stringify(golden.expected)))
      // The stored update still decodes to the expected document with this BlockNote version.
      const doc = new Y.Doc()
      Y.applyUpdate(doc, fromBase64(stored.y_update))
      expect(JSON.parse(JSON.stringify(yDocToBlocks(editor(), doc, FRAGMENT)))).toEqual(stored.expected)

      // The Rust converter's update for the same blocks projects to the same document in BlockNote.
      const rustPath = join(COLLAB_GOLDENS, 'rust', `${name}.json`)
      if (existsSync(rustPath)) {
        const rust = JSON.parse(readFileSync(rustPath, 'utf8')) as { update: string }
        const rustDoc = new Y.Doc()
        Y.applyUpdate(rustDoc, fromBase64(rust.update))
        expect(JSON.parse(JSON.stringify(yDocToBlocks(editor(), rustDoc, FRAGMENT)))).toEqual(stored.expected)
      } else if (!UPDATE) {
        throw new Error(`missing ${rustPath}: run UPDATE_COLLAB_GOLDENS=1 cargo test -p orbit-server --test collab_convert`)
      }
    })
  }
})

/** A thread store that holds nothing: the parity test only needs the comment mark in the schema. */
class EmptyThreadStore extends ThreadStore {
  constructor() {
    super(new DefaultThreadStoreAuth('user', 'comment'))
  }
  addThreadToDocument = undefined
  createThread = async (): Promise<never> => Promise.reject(new Error('unused'))
  addComment = async (): Promise<never> => Promise.reject(new Error('unused'))
  updateComment = async () => {}
  deleteComment = async () => {}
  deleteThread = async () => {}
  resolveThread = async () => {}
  unresolveThread = async () => {}
  addReaction = async () => {}
  deleteReaction = async () => {}
  getThread = (): never => {
    throw new Error('unused')
  }
  getThreads = () => new Map()
  subscribe = () => () => {}
}

function commentEditor() {
  return BlockNoteEditor.create({
    schema: pageEditorSchema,
    extensions: [CommentsExtension({ threadStore: new EmptyThreadStore(), resolveUsers: async () => [] })],
    _tiptapOptions: { injectCSS: false },
  })
}

const COMMENT_BLOCKS: Json[] = [
  { id: 'cm-p', type: 'paragraph', content: 'Commented text with a second overlapping thread' },
  {
    id: 'cm-h',
    type: 'heading',
    props: { level: 2 },
    content: [
      { type: 'text', text: 'Bold head', styles: { bold: true } },
      { type: 'text', text: ' and tail', styles: {} },
    ],
  },
  { id: 'cm-b', type: 'bulletListItem', content: 'Resolved thread (orphan mark)' },
]

/** Where a block's inline text starts in the PM document (`blockGroup > blockContainer > node`). */
function textStart(doc: import('@tiptap/pm/model').Node, blockId: string): number {
  let found = -1
  doc.descendants((node, pos) => {
    if (found >= 0) return false
    if (node.type.name === 'blockContainer' && node.attrs.id === blockId) {
      found = pos + 2
      return false
    }
    return true
  })
  if (found < 0) throw new Error(`no block ${blockId}`)
  return found
}

describe('comment marks in the collaborative document', () => {
  test('are invisible in the JSON projection and stored as hashed y-prosemirror attributes', () => {
    const editorWithComments = commentEditor()
    const node = _blocksToProsemirrorNode(editorWithComments, COMMENT_BLOCKS as never)
    const mark = (threadId: string, orphan = false) => node.type.schema.marks.comment.create({ threadId, orphan })
    const paragraph = textStart(node, 'cm-p')
    const heading = textStart(node, 'cm-h')
    const bullet = textStart(node, 'cm-b')
    const marked = new Transform(node)
      .addMark(paragraph, paragraph + 14, mark('0199a0b0-0000-7000-8000-0000000000c1'))
      .addMark(paragraph + 10, paragraph + 30, mark('0199a0b0-0000-7000-8000-0000000000c2'))
      .addMark(heading + 5, heading + 13, mark('0199a0b0-0000-7000-8000-0000000000c3'))
      .addMark(bullet, bullet + 8, mark('0199a0b0-0000-7000-8000-0000000000c4', true)).doc
    const doc = new Y.Doc()
    doc.clientID = 1
    prosemirrorToYXmlFragment(marked, doc.getXmlFragment(FRAGMENT))
    const golden = {
      blocks: COMMENT_BLOCKS,
      y_update: base64(Y.encodeStateAsUpdate(doc)),
      expected: yDocToBlocks(editorWithComments, doc, FRAGMENT) as unknown as Json[],
    }
    const path = join(MARK_GOLDENS, 'comment_marks.json')
    if (UPDATE) {
      mkdirSync(MARK_GOLDENS, { recursive: true })
      writeJson(path, golden)
    }
    // Comment marks never reach the blocks: the projection equals the unmarked document's.
    expect(JSON.parse(JSON.stringify(golden.expected))).toEqual(JSON.parse(JSON.stringify(goldenFor(COMMENT_BLOCKS).expected)))
    // Overlapping marks are allowed, so y-prosemirror keys them `comment--<hash>`.
    const keys = new Set<string>()
    const visit = (item: Y.XmlElement | Y.XmlText | Y.XmlFragment) => {
      if (item instanceof Y.XmlText) {
        for (const op of item.toDelta() as { attributes?: Record<string, unknown> }[]) Object.keys(op.attributes ?? {}).forEach((key) => keys.add(key))
        return
      }
      for (const child of item.toArray()) visit(child as Y.XmlElement)
    }
    visit(doc.getXmlFragment(FRAGMENT))
    expect([...keys].filter((key) => key.startsWith('comment--')).length).toBeGreaterThanOrEqual(3)

    const stored = JSON.parse(readFileSync(path, 'utf8')) as { blocks: Json[]; y_update: string; expected: Json[] }
    expect(stored.expected).toEqual(JSON.parse(JSON.stringify(golden.expected)))
    // The stored update still holds the marks an editor with comments reads back (thread anchors).
    const storedDoc = new Y.Doc()
    Y.applyUpdate(storedDoc, fromBase64(stored.y_update))
    const pm = yXmlFragmentToProseMirrorRootNode(storedDoc.getXmlFragment(FRAGMENT), commentEditor().pmSchema)
    const threads = new Set<string>()
    pm.descendants((child) => {
      for (const found of child.marks) if (found.type.name === 'comment') threads.add(`${found.attrs.threadId}:${found.attrs.orphan}`)
      return true
    })
    expect([...threads].sort()).toEqual([
      '0199a0b0-0000-7000-8000-0000000000c1:false',
      '0199a0b0-0000-7000-8000-0000000000c2:false',
      '0199a0b0-0000-7000-8000-0000000000c3:false',
      '0199a0b0-0000-7000-8000-0000000000c4:true',
    ])
    expect(pm.textContent).toBe(marked.textContent)
  })
})
