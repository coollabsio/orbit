import { useRef, useState, type DragEvent } from 'react'
import { CloseCircle, DocumentUpload, Paperclip } from 'reicon-react'
import { toast } from 'sonner'
import { cn } from 'cn'
import { apiClient } from '@/api/client'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent } from '@/components/ui/popover'
import { pageUploadErrorMessage, uploadPageFileRequest } from '@/features/docs/api/pages'
import { MAX_FILES, rowFileUrl } from '@orbit/database/fields'
import type { CellValue, FileRef, Row } from '@orbit/database/types'
import { useDatabaseEnv } from '../context'
import { usePopoverClose, type CellEditorProps } from './CellEditor'

/** A files cell: one chip per file, each a download link. */
export function FileChips({ row, files, wrap }: { row: Row; files: FileRef[]; wrap: boolean }) {
  const { workspaceId } = useDatabaseEnv()
  return (
    <span className={cn('flex min-w-0 items-center gap-1', wrap ? 'flex-wrap py-1' : 'overflow-hidden')}>
      {files.map((file) => (
        <a
          key={file.file_id}
          href={rowFileUrl(workspaceId, row.id, file.file_id)}
          target="_blank"
          rel="noreferrer"
          title={file.name}
          className="flex max-w-40 min-w-0 shrink-0 items-center gap-1 rounded-sm bg-muted px-1.5 py-0.5 text-xs text-foreground hover:bg-muted/70"
          onClick={(event) => event.stopPropagation()}
        >
          <Paperclip className="size-3 shrink-0 text-muted-foreground" aria-hidden="true" />
          <span className="truncate">{file.name}</span>
        </a>
      ))}
    </span>
  )
}

/** The files popover (spec P3 12.2): upload or drop files onto the row, drag to reorder, remove. */
export function FilesEditor({ property, row, anchor, onCommit, onClose, value }: CellEditorProps & { value: CellValue | null }) {
  const { workspaceId } = useDatabaseEnv()
  const [files, setFiles] = useState<FileRef[]>(value?.kind === 'files' ? value.files : [])
  /** The list as last committed: uploads finish after other changes. */
  const latest = useRef(files)
  const [uploading, setUploading] = useState(0)
  const [dragging, setDragging] = useState<number | null>(null)
  const input = useRef<HTMLInputElement>(null)
  const onOpenChange = usePopoverClose(anchor, onClose)

  const commit = (next: FileRef[]) => {
    latest.current = next
    setFiles(next)
    onCommit(next.length > 0 ? { kind: 'files', files: next } : null)
  }
  const upload = async (picked: File[]) => {
    const room = MAX_FILES - latest.current.length
    if (picked.length > room) toast.error(`A cell holds at most ${MAX_FILES} files.`)
    const batch = picked.slice(0, Math.max(room, 0))
    if (batch.length === 0) return
    setUploading((count) => count + batch.length)
    const added: FileRef[] = []
    for (const file of batch) {
      try {
        const stored = await uploadPageFileRequest(apiClient, workspaceId, row.id, file)
        added.push({ file_id: stored.id, name: stored.file_name })
      } catch (error) {
        toast.error(pageUploadErrorMessage(error))
      } finally {
        setUploading((count) => count - 1)
      }
    }
    if (added.length > 0) commit([...latest.current, ...added].slice(0, MAX_FILES))
  }
  const move = (from: number, to: number) => {
    if (from === to) return
    const next = [...files]
    const [file] = next.splice(from, 1)
    next.splice(to, 0, file!)
    commit(next)
  }
  const dropFiles = (event: DragEvent) => {
    if (event.dataTransfer.files.length === 0) return
    event.preventDefault()
    void upload([...event.dataTransfer.files])
  }

  return (
    <Popover open onOpenChange={onOpenChange}>
      <PopoverContent
        anchor={anchor}
        align="start"
        className="w-72 gap-1 p-1"
        onDragOver={(event) => {
          if (event.dataTransfer.types.includes('Files')) event.preventDefault()
        }}
        onDrop={dropFiles}
      >
        {files.length > 0 ? (
          <ul aria-label={property.name} className="flex flex-col">
            {files.map((file, index) => (
              <li
                key={file.file_id}
                draggable
                onDragStart={(event) => {
                  setDragging(index)
                  event.dataTransfer.effectAllowed = 'move'
                }}
                onDragEnd={() => setDragging(null)}
                onDragOver={(event) => {
                  if (dragging !== null) event.preventDefault()
                }}
                onDrop={(event) => {
                  if (dragging === null) return
                  event.preventDefault()
                  event.stopPropagation()
                  move(dragging, index)
                  setDragging(null)
                }}
                className={cn('group flex cursor-grab items-center gap-2 rounded-md px-2 py-1 text-sm hover:bg-muted', dragging === index && 'opacity-50')}
              >
                <Paperclip className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
                <a
                  href={rowFileUrl(workspaceId, row.id, file.file_id)}
                  target="_blank"
                  rel="noreferrer"
                  className="min-w-0 flex-1 truncate hover:underline"
                >
                  {file.name}
                </a>
                <button
                  type="button"
                  aria-label={`Remove ${file.name}`}
                  className="shrink-0 text-muted-foreground opacity-0 group-hover:opacity-100 hover:text-foreground focus-visible:opacity-100"
                  onClick={() => commit(files.filter((other) => other.file_id !== file.file_id))}
                >
                  <CloseCircle className="size-3.5" />
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="px-2 py-1.5 text-xs text-muted-foreground">Drop files here or upload them.</p>
        )}
        <input
          ref={input}
          type="file"
          multiple
          hidden
          onChange={(event) => {
            const picked = [...(event.target.files ?? [])]
            event.target.value = ''
            void upload(picked)
          }}
        />
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="justify-start text-muted-foreground"
          disabled={uploading > 0 || files.length >= MAX_FILES}
          onClick={() => input.current?.click()}
        >
          <DocumentUpload />
          {uploading > 0 ? 'Uploading…' : 'Upload files'}
        </Button>
      </PopoverContent>
    </Popover>
  )
}
