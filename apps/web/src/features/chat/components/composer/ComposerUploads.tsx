import { Xmark } from 'reicon-react'
import { Attachments } from '@/components/common/Attachments'
import { Button } from '@/components/ui/button'
import type { Upload } from './useUploads'

/** The files in a composer: uploaded ones as attachments, the others as rows with progress or the failure. */
export function ComposerUploads({
  uploads,
  onRemove,
  onRetry,
}: {
  uploads: Upload[]
  onRemove: (id: string) => void
  onRetry: (id: string) => void
}) {
  if (uploads.length === 0) return null
  const done = uploads.flatMap((upload) => (upload.attachment ? [upload.attachment] : []))
  const pending = uploads.filter((upload) => upload.status !== 'done')

  return (
    <div data-slot="composer-uploads" className="flex shrink-0 flex-col gap-1.5 px-3 pb-1">
      <Attachments
        attachments={done}
        hasTextContent={false}
        onRemove={(attachmentId) => {
          const upload = uploads.find((candidate) => candidate.attachment?.id === attachmentId)
          if (upload) onRemove(upload.id)
        }}
      />
      {pending.map((upload) => (
        <div
          key={upload.id}
          data-slot="composer-upload"
          data-status={upload.status}
          className="flex max-w-md items-center gap-2 rounded-md border border-border py-1 pr-1 pl-2.5 text-xs data-[status=failed]:border-destructive/40"
        >
          <span className="min-w-0 flex-1 truncate font-medium">{upload.file.name || 'Pasted file'}</span>
          {upload.status === 'uploading' ? (
            <span
              role="progressbar"
              aria-label={`Uploading ${upload.file.name}`}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={Math.round(upload.progress * 100)}
              className="h-1 w-24 shrink-0 overflow-hidden rounded-full bg-muted"
            >
              <span className="block h-full bg-primary" style={{ width: `${Math.round(upload.progress * 100)}%` }} />
            </span>
          ) : (
            <span role="alert" className="flex shrink-0 items-center gap-1.5 text-destructive">
              Upload failed.
              <Button variant="link" size="xs" className="h-6 px-0 text-destructive" onClick={() => onRetry(upload.id)}>
                Try again
              </Button>
            </span>
          )}
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={upload.status === 'uploading' ? `Cancel upload of ${upload.file.name}` : `Remove ${upload.file.name}`}
            onClick={() => onRemove(upload.id)}
          >
            <Xmark />
          </Button>
        </div>
      ))}
    </div>
  )
}
