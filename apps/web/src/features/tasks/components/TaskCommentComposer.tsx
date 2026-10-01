import { useEffect, useMemo, useRef, useState } from 'react'
import { ArrowUp, Paperclip2 as Paperclip, Xmark as X } from 'reicon-react'
import { cn } from 'cn'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import type { User } from '@/features/workspaces/models'

export function TaskCommentComposer({ placeholder, pending, progress, error, members = [], compact, onSend }: { placeholder: string; pending: boolean; progress?: number; error?: string; members?: User[]; compact?: boolean; onSend: (body: string, files: File[], mentionedUserIds: string[]) => Promise<unknown> }) {
  const [body, setBody] = useState('')
  const [files, setFiles] = useState<File[]>([])
  const [mentionedUserIds, setMentionedUserIds] = useState<string[]>([])
  const input = useRef<HTMLInputElement>(null)
  // `pending` arrives a render late; this blocks a second click before it does
  const sending = useRef(false)
  const mentionQuery = useMemo(() => {
    const match = body.match(/(?:^|\s)@([^\s@]*)$/)
    return match ? match[1].toLowerCase() : null
  }, [body])
  const suggestions = mentionQuery === null
    ? []
    : members.filter((member) => member.name.toLowerCase().includes(mentionQuery) || member.handle.toLowerCase().includes(mentionQuery)).slice(0, 8)
  useEffect(() => {
    if (files.length === 0 && input.current) input.current.value = ''
  }, [files.length])
  const send = async () => {
    if (sending.current || pending || (!body.trim() && files.length === 0)) return
    sending.current = true
    try {
      await onSend(body, files, mentionedUserIds)
      setBody((current) => current === body ? '' : current)
      setFiles((current) => current === files ? [] : current)
      setMentionedUserIds((current) => current === mentionedUserIds ? [] : current)
    } finally {
      sending.current = false
    }
  }
  const insertMention = (member: User) => {
    setBody((current) => current.replace(/@([^\s@]*)$/, `@${member.name} `))
    setMentionedUserIds((current) => current.includes(member.id) ? current : [...current, member.id])
  }
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
              @{member.name}
            </Button>
          ))}
        </div>
      ) : null}
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
        <Button variant="ghost" size="icon-sm" type="button" className="text-muted-foreground" aria-label="Attach" title="Attach files" onClick={() => input.current?.click()}><Paperclip /></Button>
        {error ? (
          <Button type="button" size="sm" disabled={pending} onClick={() => void send()}>Retry</Button>
        ) : (
          <Button type="button" size="icon-sm" className="rounded-full" aria-label={pending ? 'Sending…' : 'Send'} title="Send (Ctrl/⌘ + Enter)" disabled={pending || (!body.trim() && files.length === 0)} onClick={() => void send()}><ArrowUp /></Button>
        )}
      </div>
    </div>
  )
}
