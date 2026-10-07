import { createExtension, defaultBlockSpecs } from '@blocknote/core'
import { Decoration } from '@tiptap/pm/view'
import { createHighlightPlugin } from 'prosemirror-highlight'
import { useState, useSyncExternalStore } from 'react'
import { createRoot } from 'react-dom/client'
import { codeTokenVariants } from '@/components/common/codeToken'
import { CopyCodeButton } from '@/components/common/CopyCodeButton'
import { MermaidDiagram } from '@/components/common/MermaidDiagram'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { CODE_LANGUAGES, codeLines, languageId, MERMAID_LANGUAGE } from '@/lib/codeLanguages'

const defaultCodeBlock = defaultBlockSpecs.codeBlock

/** The part of the editor that a code block uses. */
interface CodeBlockEditor {
  isEditable: boolean
  isFocused(): boolean
  onChange(callback: () => void): () => void
  getBlock(id: string): { content?: unknown } | undefined
  getTextCursorPosition(): { block: { id: string } }
  updateBlock(id: string, update: { props: { language: string } }): unknown
}

/**
 * Syntax colors for the code of every code block: the same tokens and colors as a code block in a task or a chat
 * message. They are decorations, so the document holds plain text only.
 */
const highlightExtension = createExtension({
  key: 'codeHighlight',
  prosemirrorPlugins: [
    createHighlightPlugin({
      nodeTypes: ['codeBlock'],
      languageExtractor: (node) => node.attrs.language,
      parser: ({ content, language, pos }) => {
        const decorations: Decoration[] = []
        // the text of a node starts one position after the node
        let from = pos + 1
        for (const line of codeLines(content, language ?? '')) {
          for (const token of line) {
            const className = codeTokenVariants({ type: token.type })
            if (className) decorations.push(Decoration.inline(from, from + token.value.length, { class: className }))
            from += token.value.length
          }
          // the line break
          from += 1
        }
        return decorations
      },
    }),
  ],
})

/**
 * The text of a code block, from the document. The DOM of the block is not a source for it: it also holds the name
 * label of each other person whose cursor is in the block.
 */
function blockText(editor: CodeBlockEditor, id: string) {
  const content = editor.getBlock(id)?.content
  if (!Array.isArray(content)) return ''
  return content.map((item: { text?: string }) => item.text ?? '').join('')
}

function cursorInBlock(editor: CodeBlockEditor, id: string) {
  try {
    return editor.isFocused() && editor.getTextCursorPosition().block.id === id
  } catch {
    // the selection is not in text
    return false
  }
}

/**
 * The block that a person has just changed to `mermaid` with the language selector. The change makes a new node view,
 * and the focus is in the selector then, so this is how the new view knows to start in "Edit".
 */
let editNext: string | undefined

/**
 * The controls in the top-right corner of a code block, shown on hover: the language (a change of it makes a new node
 * view), the tabs of a `mermaid` block, and the copy button.
 */
function CodeTools({ editor, id, language, children }: { editor: CodeBlockEditor; id: string; language: string; children?: React.ReactNode }) {
  return (
    <div className="absolute top-2 right-2 flex items-center gap-1">
      {editor.isEditable ? (
        <Select
          items={CODE_LANGUAGES}
          value={languageId(language)}
          onValueChange={(value) => {
            // The page can lock while it is open; the node view is not made again for that.
            if (!editor.isEditable) return
            if (value === MERMAID_LANGUAGE) editNext = id
            editor.updateBlock(id, { props: { language: value as string } })
          }}
        >
          <SelectTrigger
            size="sm"
            aria-label="Language"
            className="w-auto bg-background/90 text-xs transition-opacity duration-150 focus-visible:opacity-100 data-popup-open:opacity-100 hover-fine:opacity-0 hover-fine:group-hover:opacity-100 dark:bg-background/90"
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {CODE_LANGUAGES.map((item) => (
              <SelectItem key={item.value} value={item.value}>
                {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : null}
      {children}
      <CopyCodeButton getText={() => blockText(editor, id)} />
    </div>
  )
}

/**
 * A `mermaid` code block. "View" shows only the diagram; "Edit" shows the code with the diagram below it, and the
 * diagram follows the code while the person types. A block that the person has just inserted, or changed to `mermaid`,
 * starts in "Edit".
 */
function MermaidBlock({ editor, id }: { editor: CodeBlockEditor; id: string }) {
  const text = useSyncExternalStore(
    (onChange) => editor.onChange(onChange),
    () => blockText(editor, id),
  )
  const [chosen, setChosen] = useState(() => {
    const edit = editNext === id || cursorInBlock(editor, id)
    editNext = undefined
    return edit ? 'edit' : 'view'
  })
  const mode = editor.isEditable ? chosen : 'view'

  return (
    <div data-slot="mermaid-block" data-mode={mode} className="group/mermaid">
      <CodeTools editor={editor} id={id} language={MERMAID_LANGUAGE}>
        {editor.isEditable ? (
          <Tabs
            value={mode}
            onValueChange={(value) => setChosen(value)}
            className="transition-opacity duration-150 focus-within:opacity-100 hover-fine:opacity-0 hover-fine:group-hover:opacity-100"
          >
            <TabsList className="group-data-horizontal/tabs:h-7">
              <TabsTrigger value="edit" className="px-2 text-[11px]">
                Edit
              </TabsTrigger>
              <TabsTrigger value="view" className="px-2 text-[11px]">
                View
              </TabsTrigger>
            </TabsList>
          </Tabs>
        ) : null}
      </CodeTools>
      {/* the page surface: the code surface is dark in both themes */}
      <div className="bg-background p-4 group-data-[mode=edit]/mermaid:rounded-b-lg group-data-[mode=edit]/mermaid:border-t group-data-[mode=view]/mermaid:rounded-lg group-data-[mode=view]/mermaid:border group-data-[mode=view]/mermaid:pt-11">
        <MermaidDiagram code={text} />
      </div>
    </div>
  )
}

/**
 * BlockNote's code block with syntax colors and, in its top-right corner, a language selector and a copy button. A
 * `mermaid` block also has its diagram and the "Edit" / "View" tabs. These are a small React root outside the block's
 * content DOM, so ProseMirror must ignore its DOM changes and events.
 */
export const codeBlockSpec: typeof defaultCodeBlock = {
  ...defaultCodeBlock,
  extensions: [...(defaultCodeBlock.extensions ?? []), highlightExtension],
  implementation: {
    ...defaultCodeBlock.implementation,
    render(block, editor) {
      const view = defaultCodeBlock.implementation.render.call(this, block, editor)
      // `dom` render (HTML export, clipboard) must stay plain code.
      if (this.renderType !== 'nodeView' || !(view.dom instanceof HTMLElement) || !view.contentDOM) return view

      const mermaid = languageId(block.props.language) === MERMAID_LANGUAGE
      const container = document.createElement('div')
      container.contentEditable = 'false'
      // The block content is a flex row (BlockNote) and `position: relative`: the controls pin to its top-right corner.
      container.className = 'basis-full'
      // The code surface is dark in both themes, so the token colors are the dark ones.
      view.contentDOM.parentElement?.classList.add('dark')
      view.dom.classList.add('group')
      // The diagram wraps to a line of its own. In "View" the code is hidden and the block has no code surface.
      if (mermaid) view.dom.classList.add('flex-wrap', 'has-data-[mode=view]:bg-transparent!', 'has-data-[mode=view]:p-0!', 'has-data-[mode=view]:[&>pre]:hidden!')
      view.dom.appendChild(container)
      const root = createRoot(container)
      root.render(mermaid ? <MermaidBlock editor={editor} id={block.id} /> : <CodeTools editor={editor} id={block.id} language={block.props.language} />)

      const ours = (target: EventTarget | null) => target instanceof Node && container.contains(target)
      const destroy = view.destroy
      return {
        ...view,
        ignoreMutation: (mutation) => ours(mutation.target),
        stopEvent: (event: Event) => ours(event.target),
        destroy: () => {
          destroy?.()
          // Unmounting synchronously while React renders the editor logs a warning.
          queueMicrotask(() => root.unmount())
        },
      }
    },
  },
}
