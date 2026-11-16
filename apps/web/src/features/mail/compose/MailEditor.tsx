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
  /** The body as plain text without code blocks (for the pre-send checks). */
  text: () => string
  /** Whether the user wrote anything: text other than this signature at the end. */
  hasWriting: (signatureHtml: string | null) => boolean
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
  /** The block at a point of the screen (a drop), else the cursor's block: where dropped images go. */
  blockAt: (point?: { x: number; y: number }) => string | null
  /** Puts an uploaded image in the text after `afterBlockId` (or at the end); it is sent as `cid:{contentId}`. */
  insertImage: (contentId: string, name: string, afterBlockId: string | null) => void
}

export interface MailEditorProps {
  /** The draft's body, read once. */
  initialHtml: string
  /** Put under two empty lines when the body is blank (a new draft). */
  signatureHtml: string | null
  snippets: readonly MailSnippet[]
  /** The text changed; `html` exports it (the caller decides when: export is not free). */
  onChange: (html: () => string) => void
  /** Files pasted into the text (screenshots), with the block they were pasted in: images go inline there. */
  onPasteFiles: (files: File[], blockId: string | null) => void
  /** The URL to show for an image's URL (`cid:` previews). Read once. */
  resolveImage: (url: string) => Promise<string>
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

/** The body as HTML; `''` while it is blank. */
const exportHtml = (editor: MailEditorInstance) => (isBlankDocument(editor.document) ? '' : editor.blocksToHTMLLossy(editor.document))

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

export function MailEditor({ initialHtml, signatureHtml, snippets, onChange, onPasteFiles, resolveImage, autoFocus, editable, className, ref }: MailEditorProps) {
  const { theme } = useTheme()
  const onFilesRef = useRef(onPasteFiles)
  const onChangeRef = useRef(onChange)
  const snippetsRef = useRef(snippets)
  useEffect(() => {
    onFilesRef.current = onPasteFiles
    onChangeRef.current = onChange
    snippetsRef.current = snippets
  }, [onPasteFiles, onChange, snippets])
  // read once: later draft changes come from this editor
  const [content] = useState(() => initialBlocks(initialHtml, signatureHtml))

  const editor = useCreateBlockNote({
    schema: mailEditorSchema,
    initialContent: content as never,
    links: { isValidLink: isSafeLinkHref },
    // Pasted files (a screenshot: files, no HTML) go to the composer; copied text and web images go in as they are.
    pasteHandler: ({ event, editor: target, defaultPasteHandler }) => {
      const data = event.clipboardData
      const files = data ? [...data.files] : []
      if (files.length > 0 && !data?.types.includes('text/html')) {
        onFilesRef.current(files, target.getTextCursorPosition().block.id)
        return true
      }
      return defaultPasteHandler()
    },
    // the text keeps `cid:` URLs (what the message sends); the editor shows previews
    resolveFileUrl: (url: string) => resolveImage(url),
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
      html: () => exportHtml(editor),
      text: () => mailText(editor.document, { skipCode: true }),
      hasWriting: (signatureHtml) => {
        const document = editor.document
        const lines = document.map(blockText)
        const start = signatureHtml ? signatureStart(lines, htmlToMailBlocks(signatureHtml).map(blockText)) : null
        return !isBlankDocument(start === null ? document : document.slice(0, start))
      },
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
      blockAt: (point) => {
        const view = editor.prosemirrorView
        if (point && view) {
          const found = view.posAtCoords({ left: point.x, top: point.y })
          if (found) view.dispatch(view.state.tr.setSelection(TextSelection.near(view.state.doc.resolve(found.pos))))
        }
        return editor.getTextCursorPosition().block.id ?? null
      },
      insertImage: (contentId, name, afterBlockId) => {
        const anchor = (afterBlockId ? editor.getBlock(afterBlockId) : undefined) ?? editor.document.at(-1)
        if (!anchor) return
        const image = { type: 'image', props: { url: `cid:${contentId}`, name, showPreview: true } } as const
        // an empty line becomes the image; text stays, and the image goes after it
        const blank = anchor.type === 'paragraph' && blockText(anchor).trim() === ''
        const [inserted] = blank ? editor.replaceBlocks([anchor], [image]).insertedBlocks : editor.insertBlocks([image], anchor, 'after')
        if (!inserted) return
        // a line after the image to go on writing
        const next = editor.getNextBlock(inserted.id) ?? editor.insertBlocks([{ type: 'paragraph' }], inserted.id, 'after')[0]
        if (next) editor.setTextCursorPosition(next, 'start')
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
      onChange={() => onChangeRef.current(() => exportHtml(editor))}
    >
      <FormattingToolbarController formattingToolbar={MailFormattingToolbar} />
      {editable ? <TabCompletesSuggestion /> : null}
      {editable ? <SuggestionMenuController triggerCharacter=";" shouldOpen={mentionTriggerAllowed} getItems={getSnippetItems} /> : null}
    </ThemedBlockNoteView>
  )
}
