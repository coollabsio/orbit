import type { RefObject } from 'react'
import { useRef, useState } from 'react'
import { Paperclip, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { sendReply } from '../../../mock/actions'
import type { Attachment } from '../../../mock/types'
import { clipboardFiles, fileToAttachment } from '../../chat/attachmentLib'
import { Attachments } from '../../chat/components/Attachments'

interface ReplyComposerProps {
  threadId: string
  replyToName: string
  textareaRef?: RefObject<HTMLTextAreaElement | null>
  onClose: () => void
}

export function ReplyComposer({ threadId, replyToName, textareaRef, onClose }: ReplyComposerProps) {
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
    onClose()
  }

  return (
    <div
      className="mt-6 border-t border-border pt-5 duration-150 animate-in fade-in slide-in-from-bottom-1 data-[drop-over]:bg-primary/10 data-[drop-over]:ring-1 data-[drop-over]:ring-inset data-[drop-over]:ring-primary/25"
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
      <div className="text-xs font-medium text-muted-foreground">Reply to {replyToName}</div>
      <textarea
        ref={textareaRef}
        className="block min-h-24 w-full resize-y border-none bg-transparent py-2.5 text-[13px] leading-[1.5] text-foreground outline-none placeholder:text-muted-foreground focus-visible:outline-none"
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
      <div className="flex items-center gap-2.5 pt-2">
        <input ref={fileInput} type="file" multiple hidden onChange={(event) => attach(event.target.files)} />
        <Button disabled={!canSend} onClick={send}>
          Send
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="text-muted-foreground/70"
          aria-label="Attach files"
          onClick={() => fileInput.current?.click()}
        >
          <Paperclip className="size-4" />
        </Button>
        <span className="text-xs text-muted-foreground/70">⌘⏎ to send</span>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="ml-auto text-muted-foreground/70"
          aria-label="Close reply"
          onClick={() => {
            setBody('')
            setAttachments([])
            onClose()
          }}
        >
          <X className="size-4" />
        </Button>
      </div>
    </div>
  )
}
