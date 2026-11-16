// The mail body editor (§9.1 U8): BlockNote with the mail schema, Markdown shortcuts, `;` for snippets, pasted
// files handed to the composer. Loaded on demand (`LazyMailEditor` in Composer.tsx): BlockNote is about 1 MB.
import '@blocknote/shadcn/style.css'
import { useEffect, useImperativeHandle, useRef, useState, type Ref } from 'react'
import { SuggestionMenu } from '@blocknote/core'
import { FormattingToolbar, FormattingToolbarController, getFormattingToolbarItems, SuggestionMenuController, useCreateBlockNote, type DefaultReactSuggestionItem } from '@blocknote/react'
import { TextSelection } from '@tiptap/pm/state'
import { cn } from 'cn'
import type { MailSnippet } from '@orbit/api/generated/types.gen'
import { isSafeLinkHref } from '@orbit/editor/links'
import { useTheme } from '@/lib/themeContext'
import { ThemedBlockNoteView } from '@/features/docs/editor/ThemedBlockNoteView'
import { TabCompletesSuggestion } from '@/features/docs/editor/TabCompletesSuggestion'
import { mentionTriggerAllowed } from '@/features/docs/editor/mentionItems'
import { signatureStart } from './composeLib'
import { blockText, htmlToMailBlocks, isBlankDocument, mailEditorSchema, mailText, type MailBlock, type MailEditorInstance } from './mailEditorSchema'

export interface MailEditorHandle {
  /** The body as HTML; `''` while it is blank. */
  html: () => string
  /** The body as plain text (for the pre-send checks). */
  text: () => string
  focus: () => void
  /** Opens the snippet menu at the cursor (the toolbar button). */
  openSnippets: () => void
  /** A suggestion menu is open: Escape and Enter are the menu's. */
  menuOpen: () => boolean
  /**
   * From changed: the old signature at the end of the text becomes the new one (or goes). A signature the user
   * changed stays as it is. Returns whether the text changed.
   */
  swapSignature: (previous: string | null, next: string | null) => boolean
}

export interface MailEditorProps {
  /** The draft's body, read once. */
  initialHtml: string
  /** Put under two empty lines when the body is blank (a new draft). */
  signatureHtml: string | null
  snippets: readonly MailSnippet[]
  onChange: () => void
  /** Files pasted into the text (screenshots): the composer attaches them. */
  onFiles: (files: File[]) => void
  autoFocus: boolean
  editable: boolean
  className?: string
  ref?: Ref<MailEditorHandle>
}

// Alignment, colors, files and comments do not survive the server's HTML cleaning, or do not apply to mail.
const HIDDEN_TOOLBAR_ITEMS = /^(textAlign|table|fileCaption|replaceFile|fileRename|fileDownload|filePreview|colorStyle|addComment|addTiptapComment)/

function MailFormattingToolbar() {
  const items = getFormattingToolbarItems().filter((item) => !HIDDEN_TOOLBAR_ITEMS.test(String(item.key)))
  return <FormattingToolbar>{items}</FormattingToolbar>
}

/** The first blocks of an editor: the body, or for a blank body an empty line to type in, a spacer and the signature. */
function initialBlocks(html: string, signatureHtml: string | null): MailBlock[] | undefined {
  const body = htmlToMailBlocks(html)
  if (body.length > 0 && !isBlankDocument(body)) return body
  const signature = signatureHtml ? htmlToMailBlocks(signatureHtml) : []
  if (signature.length === 0) return undefined
  return [{ type: 'paragraph' }, { type: 'paragraph' }, ...signature] as MailBlock[]
}

const snippetPreview = new Map<string, string>()
/** The first line of a snippet, for the menu (cached by id and change time). */
function previewOf(snippet: MailSnippet): string {
  const key = `${snippet.id}:${snippet.updated_at}`
  let preview = snippetPreview.get(key)
  if (preview === undefined) {
    preview = mailText(htmlToMailBlocks(snippet.body_html)).split('\n').find((line) => line.trim() !== '')?.trim() ?? ''
    snippetPreview.set(key, preview)
  }
  return preview
}

/** Selects the first `{placeholder}` after `from`, so typing replaces it. */
function selectPlaceholder(editor: MailEditorInstance, from: number) {
  const view = editor.prosemirrorView
  if (!view) return
  const { doc } = view.state
  let range: [number, number] | null = null
  doc.nodesBetween(from, doc.content.size, (node, pos) => {
    if (range) return false
    if (!node.isText || !node.text) return true
    const match = /\{[^}]+\}/.exec(node.text)
    if (match) range = [pos + match.index, pos + match.index + match[0].length]
    return false
  })
  if (range) view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, range[0], range[1])))
}

/** Puts a snippet at the cursor: one paragraph inline, more as blocks after the current one. */
function insertSnippet(editor: MailEditorInstance, snippet: MailSnippet) {
  const blocks = htmlToMailBlocks(snippet.body_html)
  const from = editor.prosemirrorView?.state.selection.from ?? 0
  const [first] = blocks
  if (!first) return
  if (blocks.length === 1 && first.type === 'paragraph') {
    editor.insertInlineContent(first.content as never)
  } else {
    const current = editor.getTextCursorPosition().block
    if (current.type === 'paragraph' && blockText(current).trim() === '') editor.replaceBlocks([current], blocks)
    else editor.insertBlocks(blocks, current, 'after')
  }
  selectPlaceholder(editor, from)
}

export function MailEditor({ initialHtml, signatureHtml, snippets, onChange, onFiles, autoFocus, editable, className, ref }: MailEditorProps) {
  const { theme } = useTheme()
  const onFilesRef = useRef(onFiles)
  const onChangeRef = useRef(onChange)
  const snippetsRef = useRef(snippets)
  useEffect(() => {
    onFilesRef.current = onFiles
    onChangeRef.current = onChange
    snippetsRef.current = snippets
  }, [onFiles, onChange, snippets])
  // read once: later draft changes come from this editor
  const [content] = useState(() => initialBlocks(initialHtml, signatureHtml))

  const editor = useCreateBlockNote({
    schema: mailEditorSchema,
    initialContent: content as never,
    links: { isValidLink: isSafeLinkHref },
    // A screenshot (files, no HTML) is attached; copied text and web images go in as text and links.
    pasteHandler: ({ event, defaultPasteHandler }) => {
      const data = event.clipboardData
      const files = data ? [...data.files] : []
      if (files.length > 0 && !data?.types.includes('text/html')) {
        onFilesRef.current(files)
        return true
      }
      return defaultPasteHandler()
    },
    // the production CSP blocks Tiptap's runtime <style>; index.css has the rules
    _tiptapOptions: { injectCSS: false },
    domAttributes: { editor: { 'aria-label': 'Message' } },
  }) as unknown as MailEditorInstance

  useEffect(() => {
    if (!autoFocus) return
    const [first] = editor.document
    if (first) editor.setTextCursorPosition(first, 'start')
    editor.focus()
  }, [autoFocus, editor])

  useImperativeHandle(
    ref,
    () => ({
      html: () => (isBlankDocument(editor.document) ? '' : editor.blocksToHTMLLossy(editor.document)),
      text: () => mailText(editor.document),
      focus: () => editor.focus(),
      openSnippets: () => editor.getExtension(SuggestionMenu)?.openSuggestionMenu(';', { deleteTriggerCharacter: true, ignoreQueryLength: true }),
      menuOpen: () => editor.getExtension(SuggestionMenu)?.shown() ?? false,
      swapSignature: (previous, next) => {
        const document = editor.document
        const incoming = next ? htmlToMailBlocks(next) : []
        if (previous) {
          const start = signatureStart(document.map(blockText), htmlToMailBlocks(previous).map(blockText))
          if (start === null) return false
          const old = document.slice(start)
          editor.replaceBlocks(old, incoming.length > 0 ? incoming : start === 0 ? [{ type: 'paragraph' }] : [])
          return true
        }
        const last = document.at(-1)
        if (incoming.length === 0 || !last) return false
        editor.insertBlocks([{ type: 'paragraph' }, ...incoming], last, 'after')
        return true
      },
    }),
    [editor],
  )

  const getSnippetItems = async (query: string): Promise<DefaultReactSuggestionItem[]> => {
    const needle = query.trim().toLowerCase()
    return snippetsRef.current
      .filter((snippet) => !needle || snippet.name.toLowerCase().includes(needle))
      .slice(0, 8)
      .map((snippet) => ({
        title: snippet.name,
        subtext: previewOf(snippet),
        onItemClick: () => insertSnippet(editor, snippet),
      }))
  }

  return (
    <ThemedBlockNoteView
      editor={editor}
      editable={editable}
      theme={theme}
      data-slot="mail-editor"
      className={cn(
        // Mail text: the app's font at 14px, no BlockNote padding or fill.
        '[&_.bn-editor]:bg-transparent! [&_.bn-editor]:px-0! [&_.bn-editor]:py-0! [&_.bn-editor]:[font-family:inherit]! [&_.bn-editor]:text-sm/[1.6]! [&_.bn-editor]:text-foreground!',
        '[&_.bn-block-content]:py-0.5! [&_.bn-editor]:min-h-full [&_.bn-editor]:outline-none',
        '[&_.bn-editor_a[data-inline-content-type=link]]:text-primary! [&_.bn-editor_a[data-inline-content-type=link]]:underline! [&_.bn-editor_a[data-inline-content-type=link]]:underline-offset-2!',
        // Static placeholders (BlockNote's come through a runtime <style>, which the production CSP blocks).
        '[&_.bn-editor[contenteditable=true]_.bn-block-content[data-is-empty-and-focused]:not([data-is-only-empty-block]):has(.ProseMirror-trailingBreak:only-child)]:after:content-none!',
        '[&_.bn-editor[contenteditable=true]_.bn-block-content[data-is-only-empty-block]:has(.ProseMirror-trailingBreak:only-child)]:after:text-muted-foreground/70! [&_.bn-editor[contenteditable=true]_.bn-block-content[data-is-only-empty-block]:has(.ProseMirror-trailingBreak:only-child)]:after:not-italic!',
        "[&_.bn-editor[contenteditable=true]_.bn-block-content[data-is-only-empty-block]:has(.ProseMirror-trailingBreak:only-child)]:after:content-['Write_your_message…_Type_;_for_snippets']!",
        className,
      )}
      sideMenu={false}
      slashMenu={false}
      formattingToolbar={false}
      filePanel={false}
      tableHandles={false}
      emojiPicker={false}
      onChange={() => onChangeRef.current()}
    >
      <FormattingToolbarController formattingToolbar={MailFormattingToolbar} />
      {editable ? <TabCompletesSuggestion /> : null}
      {editable ? <SuggestionMenuController triggerCharacter=";" shouldOpen={mentionTriggerAllowed} getItems={getSnippetItems} /> : null}
    </ThemedBlockNoteView>
  )
}
