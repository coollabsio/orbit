// The files of a draft (§5.9, U14): attachments, files sent as a secure Orbit link, uploads on their way, and the
// offer to send the largest files as links when the message is too large for its send route.
import { useState } from 'react'
import { Danger, DocumentText, Gallery, Link2, Xmark as X } from 'reicon-react'
import { cn } from 'cn'
import type { MailDraftFile } from '@orbit/api/generated/types.gen'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { formatBytes } from '@/lib/formatBytes'
import { linkRefusal } from './composeLib'

/** A file on its way up. */
export interface PendingUpload {
  key: string
  name: string
  size: number
}

/** The message was refused as too large: its size, the limit it was likely over, and the files to link. */
export interface TooLarge {
  size: number
  limit: number
  suggestion: string[]
}

export function ComposerFiles({
  files,
  pending,
  busy,
  tooLarge,
  onRemove,
  onToggleLink,
  onLinkSuggested,
}: {
  files: readonly MailDraftFile[]
  pending: readonly PendingUpload[]
  /** Files with a change on its way (remove, link). */
  busy: ReadonlySet<string>
  tooLarge: TooLarge | null
  onRemove: (file: MailDraftFile) => void
  onToggleLink: (file: MailDraftFile) => void
  onLinkSuggested: () => void
}) {
  // files already there when the composer opened show at once; added ones fade in
  const [initial] = useState(() => new Set(files.map((file) => file.id)))
  if (files.length === 0 && pending.length === 0 && !tooLarge) return null
  const total = files.reduce((sum, file) => sum + file.size, 0)
  const links = files.filter((file) => file.as_link).length
  return (
    <div data-slot="composer-files" className="flex flex-col gap-2 px-3 pb-2">
      {tooLarge ? (
        <div role="alert" className="flex flex-wrap items-center gap-x-3 gap-y-1.5 rounded-lg border border-amber-500/30 bg-amber-500/8 px-2.5 py-2 text-[0.8125rem]">
          <Danger className="size-4 shrink-0 text-amber-600 dark:text-amber-400" />
          <span className="min-w-0 flex-1">
            The message is too large for its send route (about {formatBytes(tooLarge.size)}).
            {tooLarge.suggestion.length > 0
              ? ` Send ${tooLarge.suggestion.length === 1 ? 'the largest file' : `the ${tooLarge.suggestion.length} largest files`} as a secure link?`
              : ' Remove some files and send again.'}
          </span>
          {tooLarge.suggestion.length > 0 ? (
            <Button size="sm" variant="outline" onClick={onLinkSuggested}>
              <Link2 />
              Send as {tooLarge.suggestion.length === 1 ? 'a link' : 'links'}
            </Button>
          ) : null}
        </div>
      ) : null}
      {files.length > 0 || pending.length > 0 ? (
        <ul className="flex flex-wrap gap-1.5" aria-label="Files">
          {files.map((file) => (
            <FileChip key={file.id} file={file} fresh={!initial.has(file.id)} busy={busy.has(file.id)} onRemove={() => onRemove(file)} onToggleLink={() => onToggleLink(file)} />
          ))}
          {pending.map((upload) => (
            <li
              key={upload.key}
              className="flex h-8 max-w-60 items-center gap-2 rounded-lg border border-dashed bg-muted/30 px-2.5 text-[0.8125rem] text-muted-foreground"
              aria-busy="true"
            >
              <Spinner className="size-3.5" />
              <span className="min-w-0 truncate">{upload.name}</span>
              <span className="shrink-0 text-xs tabular-nums">{formatBytes(upload.size)}</span>
            </li>
          ))}
        </ul>
      ) : null}
      {files.length > 1 ? (
        <p className="text-xs text-muted-foreground tabular-nums">
          {files.length} files · {formatBytes(total)}
          {links > 0 ? ` · ${links} as ${links === 1 ? 'a link' : 'links'}` : ''}
        </p>
      ) : null}
    </div>
  )
}

function FileChip({ file, fresh, busy, onRemove, onToggleLink }: { file: MailDraftFile; fresh: boolean; busy: boolean; onRemove: () => void; onToggleLink: () => void }) {
  const refusal = linkRefusal(file)
  const Icon = file.as_link ? Link2 : file.content_type.startsWith('image/') ? Gallery : DocumentText
  return (
    <li
      data-slot="composer-file"
      data-link={file.as_link || undefined}
      className={cn(
        'group/file flex h-8 max-w-72 items-center gap-2 rounded-lg border bg-muted/30 pr-1 pl-2.5 text-[0.8125rem] transition-opacity duration-150',
        fresh && 'animate-in fade-in-0 zoom-in-[0.97] duration-150 ease-out motion-reduce:animate-none',
        file.as_link && 'border-primary/30 bg-primary/5',
        busy && 'opacity-60',
      )}
    >
      <Icon className={cn('size-4 shrink-0', file.as_link ? 'text-primary' : 'text-muted-foreground')} />
      <span className="min-w-0 truncate text-foreground">{file.filename}</span>
      <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{file.as_link ? 'Link' : formatBytes(file.size)}</span>
      <Tip label={refusal ?? (file.as_link ? 'Attach the file instead' : 'Send as a secure link')}>
        <button
          type="button"
          aria-label={file.as_link ? `Attach ${file.filename} instead` : `Send ${file.filename} as a link`}
          aria-pressed={file.as_link}
          aria-disabled={Boolean(refusal) || busy || undefined}
          className={cn(
            'flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors duration-100 outline-none hover:bg-foreground/8 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50',
            file.as_link && 'text-primary hover:text-primary',
            (refusal || busy) && 'cursor-not-allowed opacity-40 hover:bg-transparent',
          )}
          onClick={() => {
            if (!refusal && !busy) onToggleLink()
          }}
        >
          <Link2 className="size-3.5" />
        </button>
      </Tip>
      <Tip label="Remove">
        <button
          type="button"
          aria-label={`Remove ${file.filename}`}
          disabled={busy}
          className="flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors duration-100 outline-none hover:bg-foreground/8 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 disabled:opacity-40"
          onClick={onRemove}
        >
          <X className="size-3.5" />
        </button>
      </Tip>
    </li>
  )
}
