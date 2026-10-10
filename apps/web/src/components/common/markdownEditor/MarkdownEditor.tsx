import '@blocknote/shadcn/style.css'

import { useEffect, useMemo, useRef, type ReactNode } from 'react'
import { BlockNoteEditor } from '@blocknote/core'
import { filterSuggestionItems } from '@blocknote/core/extensions'
import { getDefaultReactSlashMenuItems, SuggestionMenuController, useCreateBlockNote, type DefaultReactSuggestionItem } from '@blocknote/react'
import { UserAvatar } from '@/components/common/UserAvatar'
import { mentionTriggerAllowed } from '@/features/docs/editor/mentionItems'
import { mermaidSlashItem } from '@/features/docs/editor/slashMenu'
import { ThemedBlockNoteView } from '@/features/docs/editor/ThemedBlockNoteView'
import { clipboardFiles } from '@/lib/attachmentLib'
import { buildMentionTokens, mentionedUserId, type MentionPerson } from '@/lib/mentions'
import { useTheme } from '@/lib/themeContext'
import { blocksToMarkdown, markdownToBlocks, roundTrips } from './convert'
import { markdownEditorSchema, type MarkdownEditorInstance } from './schema'

export interface MarkdownEditorProps {
  /** The markdown the editor opens with. Read once: remount the editor (a `key`) for other text. */
  value: string
  /** Who "@" offers, and who a `@Name` in the text is. */
  members: MentionPerson[]
  ariaLabel: string
  className?: string
  /** A comment: no headings and no tables in the slash menu. */
  compact?: boolean
  autoFocus?: boolean
  /**
   * Shown in place of the editor for text that it would change (`roundTrips`): the caller's plain markdown field.
   * The editor never opens on such text.
   */
  fallback: ReactNode
  /** The markdown after each change that the person made. Never called for the text the editor opened with. */
  onChange: (markdown: string) => void
  /** Mod+Enter. */
  onSubmit?: () => void
  /** Escape, when no menu of the editor took it. */
  onCancel?: () => void
  /** The focus left the editor and its menus. */
  onBlur?: () => void
  /** Files that were pasted or dropped, and are not images that `uploadImage` takes. */
  onFiles?: (files: File[]) => void
  /**
   * Uploads a pasted or dropped image and gives its source: the download path of a task attachment
   * (`taskAttachmentPath`). With it an image goes into the text; without it images go to `onFiles`.
   */
  uploadImage?: (file: File) => Promise<string>
}

/** The slash menu: the blocks of the schema, and a Mermaid diagram after the code block. */
function slashItems(editor: MarkdownEditorInstance, compact: boolean, query: string): DefaultReactSuggestionItem[] {
  const items = getDefaultReactSlashMenuItems(editor).flatMap((item) => {
    const key = (item as { key?: string }).key ?? ''
    if (compact && (key.includes('heading') || key === 'table')) return []
    return key === 'code_block' ? [item, mermaidSlashItem(editor as never)] : [item]
  })
  return filterSuggestionItems(items, query)
}

/**
 * The rich editor of task descriptions and comments: the Docs editor's blocks, with markdown in and out. It is in its
 * own chunk (load it with `lazy`), so the task list does not carry BlockNote.
 */
export default function MarkdownEditor({ value, members, ariaLabel, className, compact = false, autoFocus, fallback, onChange, onSubmit, onCancel, onBlur, onFiles, uploadImage }: MarkdownEditorProps) {
  const { theme } = useTheme()
  const tokens = useMemo(() => buildMentionTokens(members), [members])
  // Parsed once, with an editor that is never shown: the visible editor then starts with the blocks, and its first
  // change event is a change by the person.
  const initial = useRef<{ supported: boolean; blocks: ReturnType<typeof markdownToBlocks> } | null>(null)
  if (initial.current === null) {
    const parser = BlockNoteEditor.create({ schema: markdownEditorSchema, _tiptapOptions: { injectCSS: false } })
    const supported = roundTrips(parser, value, tokens)
    initial.current = { supported, blocks: supported ? markdownToBlocks(parser, value, tokens) : [] }
  }
  const { supported, blocks } = initial.current
  // Whether uploads exist is fixed per editor; the latest callback is read through a ref.
  const upload = useRef(uploadImage)
  useEffect(() => { upload.current = uploadImage }, [uploadImage])
  const editor: MarkdownEditorInstance = useCreateBlockNote({
    schema: markdownEditorSchema,
    initialContent: blocks.length > 0 ? blocks : undefined,
    uploadFile: uploadImage ? (file: File) => upload.current!(file) : undefined,
    // Tiptap would inject a <style> tag, which the production CSP (`style-src 'self'`) blocks; the rules are in index.css.
    _tiptapOptions: { injectCSS: false },
  })
  const root = useRef<HTMLDivElement>(null)
  // the caret goes to the end of the text, as in the plain field
  useEffect(() => {
    if (!autoFocus || !supported) return
    const last = editor.document.at(-1)
    if (last) editor.setTextCursorPosition(last, 'end')
    editor.focus()
  }, [autoFocus, editor, supported])
  const inlineImages = (files: File[]) => uploadImage !== undefined && files.every((file) => file.type.startsWith('image/'))

  if (!supported) return fallback

  return (
    <div
      ref={root}
      data-slot="markdown-editor"
      className={className}
      onKeyDown={(event) => {
        if (event.key === 'Enter' && (event.metaKey || event.ctrlKey) && onSubmit) {
          event.preventDefault()
          onSubmit()
        }
        // a menu of the editor that used Escape has prevented the default
        if (event.key === 'Escape' && !event.defaultPrevented && onCancel) {
          event.stopPropagation()
          onCancel()
        }
      }}
      onBlur={(event) => {
        const next = event.relatedTarget as Element | null
        // the editor's menus and toolbars are outside this element
        if (next && (root.current?.contains(next) || next.closest('.bn-root, [role=menu], [role=listbox], [role=dialog]'))) return
        onBlur?.()
      }}
      onPasteCapture={(event) => {
        const files = clipboardFiles(event)
        // images alone go on to the editor, which uploads them and puts them into the text
        if (files.length === 0 || !onFiles || inlineImages(files)) return
        event.preventDefault()
        event.stopPropagation()
        onFiles(files)
      }}
      onDropCapture={(event) => {
        const files = Array.from(event.dataTransfer.files)
        if (files.length === 0 || !onFiles || inlineImages(files)) return
        event.preventDefault()
        event.stopPropagation()
        onFiles(files)
      }}
    >
      <ThemedBlockNoteView
        editor={editor}
        theme={theme}
        aria-label={ariaLabel}
        slashMenu={false}
        sideMenu={false}
        // the text sits flush with the title and the read view: no gutter for a side menu
        // BlockNote's stylesheet is unlayered, so the `!` is what beats its own font (Inter, 16px)
        className="[&_.bn-editor]:px-0! [&_.bn-default-styles]:[font-family:inherit]! [&_.bn-default-styles]:text-[13px]! [&_.bn-default-styles]:leading-5!"
        onChange={() => onChange(blocksToMarkdown(editor))}
      >
        <SuggestionMenuController triggerCharacter="/" getItems={async (query) => slashItems(editor, compact, query)} />
        <SuggestionMenuController
          triggerCharacter="@"
          shouldOpen={mentionTriggerAllowed}
          getItems={async (query) => {
            const needle = query.trim().toLowerCase()
            return members
              .filter((member) => member.color !== undefined && (!needle || member.name.toLowerCase().includes(needle) || member.handle.toLowerCase().includes(needle)))
              .slice(0, 10)
              .map((member): DefaultReactSuggestionItem => ({
                title: member.name,
                subtext: member.handle,
                icon: <UserAvatar user={{ name: member.name, color: member.color ?? 'var(--muted-foreground)', avatarUrl: member.avatarUrl }} size={24} />,
                onItemClick: () => {
                  // the name, or the handle when another member has the same name: the text must name one person
                  const name = mentionedUserId(member.name, tokens) === member.id ? member.name : member.handle
                  editor.insertInlineContent([{ type: 'mention', props: { userId: member.id, name } }, ' '])
                },
              }))
          }}
        />
      </ThemedBlockNoteView>
    </div>
  )
}
