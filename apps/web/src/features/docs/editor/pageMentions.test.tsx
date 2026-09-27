import { beforeAll, describe, expect, mock, test } from 'bun:test'
import { act, render } from '@testing-library/react'
import { createRef } from 'react'
import { BlockNoteEditor } from '@blocknote/core'
import { EditorState, TextSelection } from '@tiptap/pm/state'
import { insertPageMention, mentionTriggerAllowed, pageMentionItems } from './mentionItems'
import { PageMentionChip } from './PageMentionChip'
import { PageEditor, type PageEditorHandle, type PageEditorProps } from './PageEditor'
import {
  filterPageMentionCandidates,
  mentionLabel,
  pageMentionCandidates,
  PageMentionNamesContext,
  PRIVATE_PAGE_HINT,
  UNKNOWN_USER,
  type PageMentionMember,
} from './pageMentions'
import { pageEditorSchema } from './schema'

beforeAll(() => {
  const meta = document.createElement('meta')
  meta.name = 'viewport'
  meta.content = 'width=device-width, initial-scale=1, interactive-widget=resizes-content'
  document.head.append(meta)
})

const ME = '0199a0b0-0000-7000-8000-000000000001'
const ANN = '0199a0b0-0000-7000-8000-0000000000a1'
const BOB = '0199a0b0-0000-7000-8000-0000000000b2'
const GONE = '0199a0b0-0000-7000-8000-0000000000ff'

const members: PageMentionMember[] = [
  { id: ME, name: 'Orbit Owner', handle: 'test', email: 'test@example.com' },
  { id: ANN, name: 'Ann Lee', handle: 'ann', email: 'ann@example.com' },
  { id: BOB, name: 'Bob Stone', handle: 'bstone', email: 'bob@corp.test', suspended: true },
  { id: '0199a0b0-0000-7000-8000-0000000000c3', name: 'Orbit Member', handle: 'member', email: 'member@example.com' },
]

describe('who "@" offers', () => {
  test('a teamspace page offers every active member, yourself last', () => {
    expect(pageMentionCandidates(members, { teamspace_id: 'ts' }, ME).map((member) => member.name)).toEqual([
      'Ann Lee',
      'Orbit Member',
      'Orbit Owner',
    ])
  })

  test('a private page offers only its owner (you)', () => {
    expect(pageMentionCandidates(members, { teamspace_id: null }, ME).map((member) => member.id)).toEqual([ME])
    expect(pageMentionCandidates(members, { teamspace_id: null }, null)).toEqual([])
  })

  test('filters by name, handle or email, case-insensitively, at most 10', () => {
    const candidates = pageMentionCandidates(members, { teamspace_id: 'ts' }, ME)
    expect(filterPageMentionCandidates(candidates, 'orb').map((member) => member.name)).toEqual(['Orbit Member', 'Orbit Owner'])
    expect(filterPageMentionCandidates(candidates, 'ANN').map((member) => member.id)).toEqual([ANN])
    expect(filterPageMentionCandidates(candidates, 'example.com').length).toBe(3)
    expect(filterPageMentionCandidates(candidates, 'bob')).toEqual([])
    const many = Array.from({ length: 15 }, (_, index) => ({ id: String(index), name: `Person ${index}` }))
    expect(filterPageMentionCandidates(many, '')).toHaveLength(10)
  })

  test('picker items carry avatars and the private-page hint', () => {
    const insert = mock(() => {})
    const shared = pageMentionItems({ candidates: members.slice(0, 2), privatePage: false }, 'ann', insert)
    expect(shared.map((item) => [item.title, item.subtext, item.group])).toEqual([['Ann Lee', 'ann', undefined]])
    expect(shared[0].icon).toBeTruthy()
    shared[0].onItemClick()
    expect(insert).toHaveBeenCalledWith(members[1])
    const own = pageMentionItems({ candidates: [members[0]], privatePage: true }, '', insert)
    expect(own.map((item) => [item.title, item.group])).toEqual([['Orbit Owner', PRIVATE_PAGE_HINT]])
  })
})

describe('mention chips', () => {
  test('show the live name, the stored name while members load, and "Unknown user" once gone', () => {
    const names = new Map([[ANN, 'Ann Renamed']])
    expect(mentionLabel(ANN, 'Ann Lee', names)).toBe('Ann Renamed')
    expect(mentionLabel(GONE, 'Former', names)).toBe(UNKNOWN_USER)
    expect(mentionLabel(ANN, 'Ann Lee', null)).toBe('Ann Lee')
    expect(mentionLabel(ANN, '  ', null)).toBe(UNKNOWN_USER)
    const view = render(
      <PageMentionNamesContext value={names}>
        <PageMentionChip userId={ANN} name="Ann Lee" />
      </PageMentionNamesContext>,
    )
    const chip = view.container.querySelector('.orbit-page-mention')!
    expect(chip.textContent).toBe('@Ann Renamed')
    expect(chip.getAttribute('data-mention-user')).toBe(ANN)
    expect(chip.getAttribute('contenteditable')).toBe('false')
    expect(chip.className).toContain('text-primary')
  })
})

describe('schema', () => {
  test('has a `mention` inline content with userId and name and no content', () => {
    const spec = pageEditorSchema.inlineContentSchema.mention
    expect(spec.content).toBe('none')
    expect(Object.keys(spec.propSchema).sort()).toEqual(['name', 'userId'])
    expect(spec.propSchema.userId.default).toBe('')
  })

  test('inserting a mention stores { userId, name } and a space', () => {
    const editor = BlockNoteEditor.create({ schema: pageEditorSchema, _tiptapOptions: { injectCSS: false } })
    editor.replaceBlocks(editor.document, [{ id: 'p', type: 'paragraph', content: 'Hi ' }])
    editor.setTextCursorPosition('p', 'end')
    insertPageMention(editor, members[1])
    expect(editor.document[0].content).toEqual([
      { type: 'text', text: 'Hi ', styles: {} },
      { type: 'mention', props: { userId: ANN, name: 'Ann Lee' } },
      { type: 'text', text: ' ', styles: {} },
    ] as never)
  })

  test('"@" opens the picker at a word start only, never in code', () => {
    const editor = BlockNoteEditor.create({ schema: pageEditorSchema, _tiptapOptions: { injectCSS: false } })
    editor.replaceBlocks(editor.document, [
      { id: 'p', type: 'paragraph', content: 'mail me' },
      { id: 'c', type: 'codeBlock', content: 'x ' },
    ])
    const doc = editor.prosemirrorState.doc
    const at = (search: string) => {
      let found = -1
      doc.descendants((node, pos) => {
        if (found < 0 && node.isText && node.text!.includes(search)) found = pos + node.text!.indexOf(search) + search.length
        return found < 0
      })
      return EditorState.create({ doc, selection: TextSelection.create(doc, found) }).tr
    }
    expect(mentionTriggerAllowed(at('mail '))).toBe(true)
    expect(mentionTriggerAllowed(at('mai'))).toBe(false)
    expect(mentionTriggerAllowed(at('x '))).toBe(false)
  })
})

describe('PageEditor with mentions', () => {
  async function renderEditor(overrides: Partial<PageEditorProps> = {}) {
    const ref = createRef<PageEditorHandle>()
    const props: PageEditorProps = {
      pageId: 'page-1',
      initialContent: [],
      editable: true,
      resolvePage: () => null,
      onOpenPage: () => {},
      onCreateSubpage: async () => 'new',
      onPickPage: async () => null,
      ...overrides,
    }
    let view!: ReturnType<typeof render>
    await act(async () => {
      view = render(<PageEditor ref={ref} {...props} />)
      await new Promise((resolve) => setTimeout(resolve, 0))
    })
    return { view, ref }
  }

  test('renders stored mentions as chips with live names and keeps them in the content', async () => {
    const content = [
      {
        id: 'p',
        type: 'paragraph',
        content: [
          { type: 'text', text: 'Ping ', styles: {} },
          { type: 'mention', props: { userId: ANN, name: 'Ann Lee' } },
          { type: 'mention', props: { userId: GONE, name: 'Old Name' } },
        ],
      },
    ]
    const { view, ref } = await renderEditor({
      initialContent: content,
      mentions: { members: [{ id: ANN, name: 'Ann Renamed' }], candidates: [], privatePage: false },
    })
    const chips = [...view.container.querySelectorAll('.orbit-page-mention')].map((chip) => chip.textContent)
    expect(chips).toEqual(['@Ann Renamed', `@${UNKNOWN_USER}`])
    const stored = ref.current!.getContent() as { content: unknown[] }[]
    expect(stored[0].content[1]).toEqual({ type: 'mention', props: { userId: ANN, name: 'Ann Lee' } })
  })

  test('without a member list chips show their stored names', async () => {
    const { view } = await renderEditor({
      initialContent: [{ id: 'p', type: 'paragraph', content: [{ type: 'mention', props: { userId: ANN, name: 'Ann Lee' } }] }],
    })
    expect(view.container.querySelector('.orbit-page-mention')?.textContent).toBe('@Ann Lee')
  })
})
