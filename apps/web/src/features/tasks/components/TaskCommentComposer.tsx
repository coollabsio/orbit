import { useShortcutTitle } from '@/shortcuts/shortcutText'
import { Suspense, useCallback, useMemo, useRef, useState } from 'react'
import { toast } from 'sonner'
import { ArrowUp, Paperclip2 as Paperclip } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import type { PageMentionMember } from '@/features/docs/editor/pageMentions'
import { uploadInlineFile } from '@/features/tasks/api/inlineFiles'
import { fileBlock, isEmptyRichText } from '@/features/tasks/lib/richText'
// Type-only import: erased at build time, so the BlockNote chunk stays lazy.
import type { PageEditorHandle } from '@/features/docs/editor/PageEditor'
import { EditorFallback, PageEditor } from './lazyEditor'

/**
 * A comment or reply field: rich text with @mentions, the slash menu and inline files (paste, drop, paperclip).
 * Cmd/Ctrl+Enter sends. The draft stays while the task is open.
 */
export function TaskCommentComposer({
  workspaceId,
  taskId,
  pending,
  error,
  members,
  compact,
  onSend,
}: {
  workspaceId: string
  taskId: string
  pending: boolean
  error?: string
  members: readonly PageMentionMember[] | null
  compact?: boolean
  onSend: (bodyJson: unknown[]) => Promise<unknown>
}) {
  const editorRef = useRef<PageEditorHandle | null>(null)
  const input = useRef<HTMLInputElement>(null)
  /** A reply field mounts its editor when it is first used (a thread list would mount one per thread). */
  const [active, setActive] = useState(!compact)
  const focusOnMount = useRef(false)
  const setEditor = useCallback((handle: PageEditorHandle | null) => {
    editorRef.current = handle
    if (handle && focusOnMount.current) {
      focusOnMount.current = false
      handle.focus()
    }
  }, [])
  /** Files still uploading: a send now would keep an empty image block. */
  const [uploads, setUploads] = useState(0)
  /** Bumped after a send: a fresh, empty editor. */
  const [draftKey, setDraftKey] = useState(0)
  const [hasContent, setHasContent] = useState(false)
  // `pending` arrives a render late; this blocks a second send before it does
  const sending = useRef(false)
  const sendTitle = useShortcutTitle('detail.sendComment', 'Send')

  const send = async () => {
    const content = editorRef.current?.getContent() ?? []
    if (sending.current || pending || uploads > 0 || isEmptyRichText(content)) return
    sending.current = true
    try {
      await onSend(content)
      setDraftKey((key) => key + 1)
      setHasContent(false)
    } catch {
      // the draft stays; the caller shows the error
    } finally {
      sending.current = false
    }
  }
  const uploadFile = useCallback(
    async (file: File) => {
      setUploads((count) => count + 1)
      try {
        return await uploadInlineFile(workspaceId, taskId, file)
      } catch (failure) {
        toast.error(`Could not upload “${file.name}”.`)
        throw failure
      } finally {
        setUploads((count) => count - 1)
      }
    },
    [workspaceId, taskId],
  )
  const attach = async (files: File[]) => {
    for (const file of files) {
      const url = await uploadFile(file).catch(() => null)
      if (url) editorRef.current?.appendBlocks([fileBlock(file, url)])
    }
  }
  const mentions = useMemo(
    () => ({ members, candidates: (members ?? []).filter((member) => !member.suspended), privatePage: false }),
    [members],
  )
  const fileScope = useMemo(() => ({ workspaceId, taskId }), [workspaceId, taskId])

  if (!active) {
    return (
      <button
        type="button"
        className="w-full cursor-text py-1 text-left text-[13px] text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
        onClick={() => {
          focusOnMount.current = true
          setActive(true)
        }}
      >
        Leave a reply…
      </button>
    )
  }

  return (
    // one box holds the field and its actions; a reply (`compact`) sits inside its thread card
    <div data-realtime-safe="" className={cn('relative flex flex-wrap items-end', compact ? 'gap-x-1' : 'rounded-[10px] border bg-card transition-colors focus-within:border-ring')}>
      <div className={cn('min-w-0', compact ? 'flex-1 py-1' : 'basis-full px-3 pt-2.5')}>
        <Suspense fallback={<EditorFallback />}>
          <PageEditor
            key={draftKey}
            ref={setEditor}
            pageId={`comment:${taskId}:${draftKey}`}
            profile="task"
            fileScope={fileScope}
            placeholder={compact ? 'reply' : 'comment'}
            ariaLabel={compact ? 'Reply' : 'Comment'}
            editable={!pending}
            onChange={(content) => setHasContent(!isEmptyRichText(content))}
            submitShortcut={() => void send()}
            uploadFile={uploadFile}
            mentions={mentions}
            sideMenu={false}
            className="text-[13px] [&_.bn-editor]:min-h-6"
          />
        </Suspense>
      </div>
      {error ? <p role="alert" className={cn('basis-full text-xs text-destructive', !compact && 'px-3 pt-2')}>{error}</p> : null}
      <div className={cn('ml-auto flex items-center gap-1', !compact && 'p-1.5')}>
        <input
          ref={input}
          type="file"
          multiple
          hidden
          aria-label="Attach comment files"
          onChange={(event) => {
            void attach(Array.from(event.target.files ?? []))
            event.target.value = ''
          }}
        />
        <Tip label="Attach files"><Button variant="ghost" size="icon-sm" type="button" className="text-muted-foreground" aria-label="Attach" onClick={() => input.current?.click()}><Paperclip /></Button></Tip>
        {error ? (
          <Button type="button" size="sm" disabled={pending} onClick={() => void send()}>Retry</Button>
        ) : (
          <Tip label={sendTitle}><Button type="button" size="icon-sm" className="rounded-full" aria-label={pending ? 'Sending…' : 'Send'} disabled={pending || !hasContent || uploads > 0} onClick={() => void send()}><ArrowUp /></Button></Tip>
        )}
      </div>
    </div>
  )
}
