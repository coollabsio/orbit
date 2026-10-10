import { useShortcutTitle } from '@/shortcuts/shortcutText'
import { lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react'
import { ArrowUp, Paperclip2 as Paperclip, Xmark as X } from 'reicon-react'
import { cn } from 'cn'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Textarea } from '@/components/ui/textarea'
import type { User } from '@/features/workspaces/models'

// BlockNote is large: it loads when a task with a rich composer shows
const MarkdownEditor = lazy(() => import('@/components/common/markdownEditor/MarkdownEditor'))

/**
 * The field of a new comment or reply. With `rich` it is the rich editor (Mod+Enter sends, "/" and "@" open their
 * menus), and the "Markdown" button changes to the plain field, from which a comment can always be sent. The text is
 * markdown in both.
 */
export function TaskCommentComposer({ placeholder, pending, progress, error, members = [], compact, rich = false, onUploadImage, onSend }: {
  placeholder: string
  pending: boolean
  progress?: number
  error?: string
  members?: User[]
  compact?: boolean
  rich?: boolean
  /** Uploads an image that was pasted or dropped into the rich editor; it goes into the text. */
  onUploadImage?: (file: File) => Promise<string>
  onSend: (body: string, files: File[]) => Promise<unknown>
}) {
  const [body, setBody] = useState('')
  const [plain, setPlain] = useState(!rich)
  // A reply field is in each thread: its editor mounts when the person goes to it, not with the page.
  const [active, setActive] = useState(!compact)
  // a new editor for each comment: the editor reads its text one time
  const [editorKey, setEditorKey] = useState(0)
  const [files, setFiles] = useState<File[]>([])
  const input = useRef<HTMLInputElement>(null)
  // `pending` arrives a render late; this blocks a second click before it does
  const sending = useRef(false)
  const mentionQuery = useMemo(() => {
    const match = body.match(/(?:^|\s)@([^\s@]*)$/)
    return match ? match[1].toLowerCase() : null
  }, [body])
  // the rich editor has its own "@" menu
  const suggestions = mentionQuery === null || !plain
    ? []
    : members.filter((member) => member.name.toLowerCase().includes(mentionQuery) || member.handle.toLowerCase().includes(mentionQuery)).slice(0, 8)
  useEffect(() => {
    if (files.length === 0 && input.current) input.current.value = ''
  }, [files.length])
  const sendTitle = useShortcutTitle('detail.sendComment', 'Send')
  const send = async () => {
    if (sending.current || pending || (!body.trim() && files.length === 0)) return
    sending.current = true
    try {
      await onSend(body, files)
      setBody((current) => current === body ? '' : current)
      setFiles((current) => current === files ? [] : current)
      setEditorKey((key) => key + 1)
    } finally {
      sending.current = false
    }
  }
  const insertMention = (member: User) => {
    // the server reads the mention from the text: the name, or the handle when another member has the same name
    const sameName = members.some((other) => other.id !== member.id && [other.name, other.handle].some((label) => label.toLowerCase() === member.name.toLowerCase()))
    setBody((current) => current.replace(/@([^\s@]*)$/, `@${sameName ? member.handle : member.name} `))
  }
  const textarea = (
      <Textarea
      className={cn(
        'block max-h-60 min-h-0 resize-none overflow-y-auto rounded-none border-0 bg-transparent text-[13px] leading-5 focus-visible:ring-0 md:text-[13px] dark:bg-transparent',
        compact ? 'w-auto min-w-0 flex-1 px-0 py-1' : 'min-h-12 basis-full px-3 pt-2.5 pb-0',
      )}
      rows={1}
      value={body}
      placeholder={placeholder}
      onChange={(event) => setBody(event.target.value)}
      onKeyDown={(event) => {
        if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
          event.preventDefault()
          void send()
        }
      }}
      onPaste={(event) => {
        const pasted = Array.from(event.clipboardData.files)
        if (pasted.length > 0) setFiles((current) => [...current, ...pasted])
      }}
    />
  )
  return (
    // one box holds the field and its actions; a reply (`compact`) is a single line inside its thread card
    <div
      className={cn('relative flex flex-wrap items-end', compact ? 'gap-x-1' : 'rounded-[10px] border bg-card transition-colors focus-within:border-ring')}
      onDrop={(event) => { event.preventDefault(); setFiles((current) => [...current, ...event.dataTransfer.files]) }}
      onDragOver={(event) => event.preventDefault()}
    >
      {suggestions.length > 0 ? (
        <div className="absolute bottom-full right-0 left-0 z-[5] mb-1.5 flex max-h-[200px] flex-col gap-px overflow-auto overscroll-contain rounded-lg border bg-popover p-1 shadow-lg" role="listbox" aria-label="Mention member">
          {suggestions.map((member) => (
            <Button variant="ghost" key={member.id} type="button" className="w-full justify-start font-normal" onMouseDown={(event) => { event.preventDefault(); insertMention(member) }}>
              {/* decorative: the initials must not go into the button's name */}
              <span className="flex" aria-hidden>
                <UserAvatar user={member} size={20} />
              </span>
              @{member.name}
            </Button>
          ))}
        </div>
      ) : null}
      {plain ? textarea : !active ? (
        <div
          role="textbox"
          tabIndex={0}
          aria-label={placeholder}
          className="min-w-0 flex-1 cursor-text py-1 text-[13px] leading-5 text-muted-foreground outline-none"
          onFocus={() => setActive(true)}
          onClick={() => setActive(true)}
        >
          {placeholder}
        </div>
      ) : (
        <Suspense fallback={<div className={cn('min-h-8', compact ? 'flex-1' : 'basis-full')} />}>
          {/* BlockNote's own placeholder needs an injected <style>, which the production CSP blocks */}
          {body ? null : <span aria-hidden="true" className={cn('pointer-events-none absolute text-[13px] leading-5 text-muted-foreground', compact ? 'top-[7px] left-0' : 'top-[13px] left-3')}>{placeholder}</span>}
          <MarkdownEditor
            key={editorKey}
            value={body}
            members={members}
            ariaLabel={placeholder}
            className={cn('max-h-60 min-w-0 overflow-y-auto', compact ? 'flex-1 py-1' : 'min-h-12 basis-full px-3 pt-2.5')}
            compact
            autoFocus={compact}
            fallback={textarea}
            onChange={setBody}
            onSubmit={() => void send()}
            onFiles={(dropped) => setFiles((current) => [...current, ...dropped])}
            uploadImage={onUploadImage}
          />
        </Suspense>
      )}
      {files.length > 0 ? <div className={cn('flex basis-full flex-wrap items-center gap-1.5', compact ? 'py-1' : 'px-3 pt-2')}>{files.map((file, index) => (
        <Badge variant="outline" className="pr-0.5" key={`${file.name}-${index}`}>
          {file.name}
          <Button type="button" variant="ghost" size="icon-xs" className="size-4 rounded-full text-muted-foreground" aria-label={`Remove ${file.name}`} onClick={() => setFiles((current) => current.filter((_, itemIndex) => itemIndex !== index))}>
            <X />
          </Button>
        </Badge>
      ))}</div> : null}
      {error ? <p role="alert" className={cn('basis-full text-xs text-destructive', !compact && 'px-3 pt-2')}>{error}</p> : null}
      <div className={cn('ml-auto flex items-center gap-1', !compact && 'p-1.5')}>
        <input ref={input} type="file" multiple hidden aria-label="Attach comment files" onChange={(event) => setFiles(Array.from(event.target.files ?? []))} />
        {progress !== undefined && (pending || error) ? <span role="status" aria-live="polite" className="text-xs text-muted-foreground/70 tabular-nums">Uploading {progress}%</span> : null}
        {rich ? (
          <Tip label={plain ? 'Use the rich editor' : 'Write plain markdown'}>
            <Button variant="ghost" size="sm" type="button" className="text-xs text-muted-foreground" aria-pressed={plain} onClick={() => setPlain((current) => !current)}>Markdown</Button>
          </Tip>
        ) : null}
        <Tip label="Attach files"><Button variant="ghost" size="icon-sm" type="button" className="text-muted-foreground" aria-label="Attach" onClick={() => input.current?.click()}><Paperclip /></Button></Tip>
        {error ? (
          <Button type="button" size="sm" disabled={pending} onClick={() => void send()}>Retry</Button>
        ) : (
          <Tip label={sendTitle}><Button type="button" size="icon-sm" className="rounded-full" aria-label={pending ? 'Sending…' : 'Send'} disabled={pending || (!body.trim() && files.length === 0)} onClick={() => void send()}><ArrowUp /></Button></Tip>
        )}
      </div>
    </div>
  )
}
