import { useEffect, useMemo, useRef, useState } from 'react'
import { Paperclip, X } from 'lucide-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import type { User } from '../api/models'

const OPTION =
  'group flex w-full min-h-8 items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-foreground transition-colors hover:bg-accent'
const PILL = 'inline-flex h-[22px] items-center gap-1.5 rounded-full border border-border bg-muted px-2.5 text-xs font-medium leading-none whitespace-nowrap text-foreground'

export function TaskCommentComposer({ placeholder, pending, progress, error, members = [], compact, onSend }: { placeholder: string; pending: boolean; progress?: number; error?: string; members?: User[]; compact?: boolean; onSend: (body: string, files: File[], mentionedUserIds: string[]) => Promise<unknown> }) {
  const [body, setBody] = useState('')
  const [files, setFiles] = useState<File[]>([])
  const [mentionedUserIds, setMentionedUserIds] = useState<string[]>([])
  const input = useRef<HTMLInputElement>(null)
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
    if (!body.trim() && files.length === 0) return
    await onSend(body, files, mentionedUserIds)
    setBody((current) => current === body ? '' : current)
    setFiles((current) => current === files ? [] : current)
    setMentionedUserIds((current) => current === mentionedUserIds ? [] : current)
  }
  const insertMention = (member: User) => {
    setBody((current) => current.replace(/@([^\s@]*)$/, `@${member.name} `))
    setMentionedUserIds((current) => current.includes(member.id) ? current : [...current, member.id])
  }
  return (
    <div className="relative grid gap-2" onDrop={(event) => { event.preventDefault(); setFiles((current) => [...current, ...event.dataTransfer.files]) }} onDragOver={(event) => event.preventDefault()}>
      {suggestions.length > 0 ? (
        <div className="absolute bottom-full right-0 left-0 z-[5] mb-1.5 flex max-h-[200px] flex-col gap-px overflow-auto rounded-lg border border-border bg-popover p-1 shadow-lg" role="listbox" aria-label="Mention member">
          {suggestions.map((member) => (
            <button key={member.id} type="button" className={OPTION} onMouseDown={(event) => { event.preventDefault(); insertMention(member) }}>
              @{member.name}
            </button>
          ))}
        </div>
      ) : null}
      <textarea
        className={cn(
          'w-full resize-y rounded-lg border border-input bg-transparent px-3 py-2 text-sm leading-5 text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 dark:bg-input/30',
          compact ? 'min-h-11 text-[13px] leading-[18px]' : 'min-h-20',
        )}
        rows={compact ? 1 : 2}
        value={body}
        placeholder={placeholder}
        onChange={(event) => setBody(event.target.value)}
        onPaste={(event) => {
          const pasted = Array.from(event.clipboardData.files)
          if (pasted.length > 0) setFiles((current) => [...current, ...pasted])
        }}
        onKeyDown={(event) => { if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); void send() } }}
      />
      {files.length > 0 ? <div className="flex flex-wrap items-center gap-2">{files.map((file, index) => <span className={PILL} key={`${file.name}-${index}`}>{file.name}<button type="button" className="ml-1 inline-flex text-inherit" aria-label={`Remove ${file.name}`} onClick={() => setFiles((current) => current.filter((_, itemIndex) => itemIndex !== index))}><X className="size-3" /></button></span>)}</div> : null}
      {error ? <p role="alert" className="text-xs text-destructive">{error}</p> : null}
      <div className="flex flex-wrap items-center gap-2">
        <input ref={input} type="file" multiple hidden aria-label="Attach comment files" onChange={(event) => setFiles(Array.from(event.target.files ?? []))} />
        <Button variant="ghost" type="button" onClick={() => input.current?.click()}><Paperclip className="size-3.5" />Attach</Button>
        {progress !== undefined && (pending || error) ? <span role="status" aria-live="polite" className="text-xs text-muted-foreground/70">Uploading {progress}%</span> : null}
        <span className="flex-1" />
        <Button type="button" disabled={pending || (!body.trim() && files.length === 0)} onClick={() => void send()}>{pending ? 'Sending…' : error ? 'Retry' : 'Send'}</Button>
      </div>
    </div>
  )
}
