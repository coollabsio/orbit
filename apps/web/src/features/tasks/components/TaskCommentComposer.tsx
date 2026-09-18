import { useEffect, useMemo, useRef, useState } from 'react'
import { Paperclip2, Xmark } from 'reicon-react'
import type { User } from '../api/models'
import type { MentionTarget } from '../api/richText'

export function TaskCommentComposer({ placeholder, pending, progress, error, members = [], compact, onSend }: { placeholder: string; pending: boolean; progress?: number; error?: string; members?: User[]; compact?: boolean; onSend: (body: string, files: File[], mentions: MentionTarget[]) => Promise<unknown> }) {
  const [body, setBody] = useState('')
  const [files, setFiles] = useState<File[]>([])
  // Carries the label too: the server derives recipients from mention nodes in the document.
  const [mentions, setMentions] = useState<MentionTarget[]>([])
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
    await onSend(body, files, mentions)
    setBody((current) => current === body ? '' : current)
    setFiles((current) => current === files ? [] : current)
    setMentions((current) => current === mentions ? [] : current)
  }
  const insertMention = (member: User) => {
    setBody((current) => current.replace(/@([^\s@]*)$/, `@${member.name} `))
    setMentions((current) => current.some((mention) => mention.id === member.id) ? current : [...current, { id: member.id, label: member.name }])
  }
  return (
    <div className="tasks-native-composer" data-compact={compact || undefined} onDrop={(event) => { event.preventDefault(); setFiles((current) => [...current, ...event.dataTransfer.files]) }} onDragOver={(event) => event.preventDefault()}>
      {suggestions.length > 0 ? (
        <div className="tasks-mention-list" role="listbox" aria-label="Mention member">
          {suggestions.map((member) => (
            <button key={member.id} type="button" className="popover-option" onMouseDown={(event) => { event.preventDefault(); insertMention(member) }}>
              @{member.name}
            </button>
          ))}
        </div>
      ) : null}
      <textarea className="input" rows={compact ? 1 : 2} value={body} placeholder={placeholder} onChange={(event) => setBody(event.target.value)} onPaste={(event) => {
        const pasted = Array.from(event.clipboardData.files)
        if (pasted.length > 0) setFiles((current) => [...current, ...pasted])
      }} onKeyDown={(event) => { if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); void send() } }} />
      {files.length > 0 ? <div className="tasks-native-files">{files.map((file, index) => <span className="pill" key={`${file.name}-${index}`}>{file.name}<button type="button" aria-label={`Remove ${file.name}`} onClick={() => setFiles((current) => current.filter((_, itemIndex) => itemIndex !== index))}><Xmark size={12} /></button></span>)}</div> : null}
      {error ? <p role="alert" className="text-danger text-xs">{error}</p> : null}
      <div className="tasks-native-actions">
        <input ref={input} type="file" multiple hidden aria-label="Attach comment files" onChange={(event) => setFiles(Array.from(event.target.files ?? []))} />
        <button className="button button-ghost" type="button" onClick={() => input.current?.click()}><Paperclip2 size={14} />Attach</button>
        {progress !== undefined && (pending || error) ? <span role="status" aria-live="polite" className="text-faint text-xs">Uploading {progress}%</span> : null}
        <span className="spacer" />
        <button className="button button-primary" type="button" disabled={pending || (!body.trim() && files.length === 0)} onClick={() => void send()}>{pending ? 'Sending…' : error ? 'Retry' : 'Send'}</button>
      </div>
    </div>
  )
}
