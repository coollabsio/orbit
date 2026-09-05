import { useRef, useState } from 'react'
import { Paperclip2, Xmark } from 'reicon-react'

export function TaskCommentComposer({ placeholder, pending, progress, onSend }: { placeholder: string; pending: boolean; progress?: number; onSend: (body: string, files: File[]) => Promise<unknown> }) {
  const [body, setBody] = useState('')
  const [files, setFiles] = useState<File[]>([])
  const input = useRef<HTMLInputElement>(null)
  const send = async () => {
    if (!body.trim() && files.length === 0) return
    await onSend(body, files)
    setBody('')
    setFiles([])
    if (input.current) input.current.value = ''
  }
  return (
    <div className="tasks-native-composer" onDrop={(event) => { event.preventDefault(); setFiles((current) => [...current, ...event.dataTransfer.files]) }} onDragOver={(event) => event.preventDefault()}>
      <textarea className="input" rows={2} value={body} placeholder={placeholder} onChange={(event) => setBody(event.target.value)} onPaste={(event) => {
        const pasted = Array.from(event.clipboardData.files)
        if (pasted.length > 0) setFiles((current) => [...current, ...pasted])
      }} onKeyDown={(event) => { if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); void send() } }} />
      {files.length > 0 ? <div className="tasks-native-files">{files.map((file, index) => <span className="pill" key={`${file.name}-${index}`}>{file.name}<button type="button" aria-label={`Remove ${file.name}`} onClick={() => setFiles((current) => current.filter((_, itemIndex) => itemIndex !== index))}><Xmark size={12} /></button></span>)}</div> : null}
      <div className="tasks-native-actions">
        <input ref={input} type="file" multiple hidden onChange={(event) => setFiles(Array.from(event.target.files ?? []))} />
        <button className="button button-ghost" type="button" onClick={() => input.current?.click()}><Paperclip2 size={14} />Attach</button>
        {pending && progress !== undefined ? <span className="text-faint text-xs">Uploading {progress}%</span> : null}
        <span className="spacer" />
        <button className="button button-primary" type="button" disabled={pending || (!body.trim() && files.length === 0)} onClick={() => void send()}>{pending ? 'Sending…' : 'Send'}</button>
      </div>
    </div>
  )
}
