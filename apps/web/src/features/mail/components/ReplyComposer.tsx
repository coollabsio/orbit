import type { RefObject } from 'react'
import { useRef, useState } from 'react'
import { Paperclip2, Xmark } from 'reicon-react'
import { sendReply } from '../../../mock/actions'
import type { Attachment } from '../../../mock/types'
import { clipboardFiles, fileToAttachment } from '../../chat/attachmentLib'
import { Attachments } from '../../chat/components/Attachments'

interface ReplyComposerProps {
  threadId: string
  replyToName: string
  textareaRef?: RefObject<HTMLTextAreaElement | null>
}

export function ReplyComposer({ threadId, replyToName, textareaRef }: ReplyComposerProps) {
  const [body, setBody] = useState('')
  const [attachments, setAttachments] = useState<Attachment[]>([])
  const [dropOver, setDropOver] = useState(false)
  const fileInput = useRef<HTMLInputElement>(null)
  const canSend = body.trim() !== '' || attachments.length > 0

  const attach = (files: FileList | File[] | null) => {
    if (!files || files.length === 0) return
    setAttachments((current) => [...current, ...Array.from(files).map(fileToAttachment)])
  }

  const send = () => {
    if (!canSend) return
    sendReply(threadId, body.trim(), attachments)
    setBody('')
    setAttachments([])
  }

  return (
    <div
      className="mail-reply"
      data-drop-over={dropOver || undefined}
      onDragOver={(event) => {
        if (!event.dataTransfer.types.includes('Files')) return
        event.preventDefault()
        setDropOver(true)
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropOver(false)
      }}
      onDrop={(event) => {
        if (!event.dataTransfer.types.includes('Files')) return
        event.preventDefault()
        setDropOver(false)
        attach(event.dataTransfer.files)
      }}
    >
      <div className="mail-reply-label">Reply to {replyToName}</div>
      <textarea
        ref={textareaRef}
        className="mail-reply-textarea"
        placeholder="Write a reply…"
        value={body}
        onChange={(e) => setBody(e.target.value)}
        onPaste={(event) => {
          const files = clipboardFiles(event)
          if (files.length === 0) return
          event.preventDefault()
          attach(files)
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
            e.preventDefault()
            send()
          }
        }}
      />
      {attachments.length > 0 ? (
        <Attachments
          attachments={attachments}
          hasTextContent={!!body.trim()}
          onRemove={(id) => setAttachments((current) => current.filter((attachment) => attachment.id !== id))}
        />
      ) : null}
      <div className="mail-reply-footer">
        <input ref={fileInput} type="file" multiple hidden onChange={(event) => attach(event.target.files)} />
        <button className="button button-primary" disabled={!canSend} onClick={send}>
          Send
        </button>
        <button type="button" className="icon-button" aria-label="Attach files" onClick={() => fileInput.current?.click()}>
          <Paperclip2 size={16} />
        </button>
        <span className="text-xs text-faint">⌘⏎ to send</span>
        {body || attachments.length > 0 ? (
          <button
            type="button"
            className="icon-button mail-reply-discard"
            aria-label="Discard reply"
            onClick={() => {
              setBody('')
              setAttachments([])
            }}
          >
            <Xmark size={16} />
          </button>
        ) : null}
      </div>
    </div>
  )
}
