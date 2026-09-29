import { useEffect, useMemo, useRef, useState } from 'react'
import { Paperclip2 as Paperclip, Xmark as X } from 'reicon-react'
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
    <div className="relative grid gap-2" onDrop={(event) => { event.preventDefault(); setFiles((current) => [...current, ...event.dataTransfer.files]) }} onDragOver={(event) => event.preventDefault()}>
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
        className={cn('field-sizing-fixed resize-y', compact ? 'min-h-11 text-[13px] leading-[18px] md:text-[13px]' : 'min-h-20')}
        rows={compact ? 1 : 2}
        value={body}
        placeholder={placeholder}
        onChange={(event) => setBody(event.target.value)}
        onPaste={(event) => {
          const pasted = Array.from(event.clipboardData.files)
          if (pasted.length > 0) setFiles((current) => [...current, ...pasted])
        }}
      />
      {files.length > 0 ? <div className="flex flex-wrap items-center gap-2">{files.map((file, index) => (
        <Badge variant="outline" className="pr-0.5" key={`${file.name}-${index}`}>
          {file.name}
          <Button type="button" variant="ghost" size="icon-xs" className="size-4 rounded-full text-muted-foreground" aria-label={`Remove ${file.name}`} onClick={() => setFiles((current) => current.filter((_, itemIndex) => itemIndex !== index))}>
            <X />
          </Button>
        </Badge>
      ))}</div> : null}
      {error ? <p role="alert" className="text-xs text-destructive">{error}</p> : null}
      <div className="flex flex-wrap items-center gap-2">
        <input ref={input} type="file" multiple hidden aria-label="Attach comment files" onChange={(event) => setFiles(Array.from(event.target.files ?? []))} />
        <Button variant="ghost" type="button" onClick={() => input.current?.click()}><Paperclip className="size-3.5" />Attach</Button>
        {progress !== undefined && (pending || error) ? <span role="status" aria-live="polite" className="text-xs text-muted-foreground/70 tabular-nums">Uploading {progress}%</span> : null}
        <span className="flex-1" />
        <Button type="button" disabled={pending || (!body.trim() && files.length === 0)} onClick={() => void send()}>{pending ? 'Sending…' : error ? 'Retry' : 'Send'}</Button>
      </div>
    </div>
  )
}
