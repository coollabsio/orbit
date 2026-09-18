import { useEffect, useRef, useState } from 'react'
import { Paperclip2, Xmark } from 'reicon-react'
import { RichTextEditor } from '../../../components/editor/RichTextEditor'
import { EMPTY_DOCUMENT, isEmptyDocument, type RichTextDocument } from '../../../components/editor/document'
import type { TaskStatusDef, User } from '../api/models'

/**
 * A comment or reply: rich text with the unified `@` menu, plus attachments that
 * stay outside the document. Cmd/Ctrl+Enter sends; plain Enter is a new line.
 */
export function TaskCommentComposer({
  placeholder,
  pending,
  progress,
  error,
  workspaceId,
  members = [],
  statuses = [],
  compact,
  onSend,
}: {
  placeholder: string
  pending: boolean
  progress?: number
  error?: string
  workspaceId: string
  members?: User[]
  statuses?: TaskStatusDef[]
  compact?: boolean
  onSend: (bodyJson: RichTextDocument, files: File[]) => Promise<unknown>
}) {
  const [body, setBody] = useState<RichTextDocument>(EMPTY_DOCUMENT)
  const [files, setFiles] = useState<File[]>([])
  // Bumped after a send so the editor remounts empty along with the state.
  const [generation, setGeneration] = useState(0)
  const input = useRef<HTMLInputElement>(null)
  const empty = isEmptyDocument(body)

  useEffect(() => {
    if (files.length === 0 && input.current) input.current.value = ''
  }, [files.length])

  const send = async (document: RichTextDocument = body) => {
    if (pending || (isEmptyDocument(document) && files.length === 0)) return
    try {
      await onSend(document, files)
    } catch {
      // The failure is shown through `error`; the draft stays for a retry.
      return
    }
    setBody(EMPTY_DOCUMENT)
    setFiles([])
    setGeneration((current) => current + 1)
  }

  return (
    <div
      className="tasks-native-composer"
      data-compact={compact || undefined}
      onDrop={(event) => {
        event.preventDefault()
        setFiles((current) => [...current, ...Array.from(event.dataTransfer.files)])
      }}
      onDragOver={(event) => event.preventDefault()}
    >
      <RichTextEditor
        key={generation}
        value={EMPTY_DOCUMENT}
        placeholder={placeholder}
        ariaLabel={placeholder}
        compact={compact}
        workspaceId={workspaceId}
        members={members}
        statuses={statuses}
        onChange={setBody}
        onSubmit={(document) => void send(document)}
        onPasteFiles={(pasted) => setFiles((current) => [...current, ...pasted])}
      />
      {files.length > 0 ? (
        <div className="tasks-native-files">
          {files.map((file, index) => (
            <span className="pill" key={`${file.name}-${index}`}>
              {file.name}
              <button
                type="button"
                aria-label={`Remove ${file.name}`}
                onClick={() => setFiles((current) => current.filter((_, itemIndex) => itemIndex !== index))}
              >
                <Xmark size={12} />
              </button>
            </span>
          ))}
        </div>
      ) : null}
      {error ? <p role="alert" className="text-danger text-xs">{error}</p> : null}
      <div className="tasks-native-actions">
        <input
          ref={input}
          type="file"
          multiple
          hidden
          aria-label="Attach comment files"
          onChange={(event) => setFiles(Array.from(event.target.files ?? []))}
        />
        <button className="button button-ghost" type="button" onClick={() => input.current?.click()}>
          <Paperclip2 size={14} />
          Attach
        </button>
        {progress !== undefined && (pending || error) ? (
          <span role="status" aria-live="polite" className="text-faint text-xs">
            Uploading {progress}%
          </span>
        ) : null}
        <span className="spacer" />
        <span className="tasks-composer-hint text-faint text-xs">⌘/Ctrl + enter to send</span>
        <button
          className="button button-primary"
          type="button"
          disabled={pending || (empty && files.length === 0)}
          onClick={() => void send()}
        >
          {pending ? 'Sending…' : error ? 'Retry' : 'Send'}
        </button>
      </div>
    </div>
  )
}
