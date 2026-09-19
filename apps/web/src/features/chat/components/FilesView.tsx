// Port of the chat reference FilesView: replaces the message area; search + kind filter + sort,
// Media grid cards and Document rows with download / delete.
import { useMemo, useState } from 'react'
import { ArrowLeft, Download, Folder, Paperclip, Search, Trash2 } from 'lucide-react'
import { Listbox } from '../../../components/ui/Listbox'
import { deleteAttachment } from '../../../mock/actions'
import type { AppState, Attachment, Channel } from '../../../mock/types'
import { fileExtension, formatSize, isImage, isMedia } from '../attachmentLib'
import { authorUser, displayName } from '../chatLib'
import { ConfirmDeleteModal } from './ChannelModals'
import { ImageViewer } from './ImageViewer'

type FileFilter = 'all' | 'media' | 'documents'
type SortOrder = 'latest' | 'oldest'

interface ChannelFile extends Attachment {
  messageId: string
  createdAt: string
  authorName: string
  authorColor?: string
}

const rowIconBtn =
  'inline-flex size-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground data-[danger=true]:hover:bg-destructive/10 data-[danger=true]:hover:text-destructive'

function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString([], { month: 'numeric', day: 'numeric', year: 'numeric' })
}

export function FilesView({ state, channel, onBack }: { state: AppState; channel: Channel; onBack: () => void }) {
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<FileFilter>('all')
  const [sortOrder, setSortOrder] = useState<SortOrder>('latest')
  const [viewer, setViewer] = useState<ChannelFile | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<ChannelFile | null>(null)

  const files = useMemo<ChannelFile[]>(
    () =>
      state.chatMessages
        .filter((m) => m.channelId === channel.id && m.attachments?.length)
        .flatMap((m) =>
          (m.attachments ?? []).map((att) => ({
            ...att,
            messageId: m.id,
            createdAt: m.createdAt,
            authorName: displayName(state, m),
            authorColor: authorUser(state, m)?.color,
          })),
        ),
    [state, channel.id],
  )

  const normalized = query.trim().toLowerCase()
  const filtered = files
    .filter((file) => {
      const kindMatch = filter === 'all' || (filter === 'media' && isMedia(file)) || (filter === 'documents' && !isMedia(file))
      if (!kindMatch) return false
      if (!normalized) return true
      return (
        file.fileName.toLowerCase().includes(normalized) ||
        file.authorName.toLowerCase().includes(normalized) ||
        file.mimeType.toLowerCase().includes(normalized)
      )
    })
    .sort((a, b) => (sortOrder === 'latest' ? b.createdAt.localeCompare(a.createdAt) : a.createdAt.localeCompare(b.createdAt)))
  const media = filtered.filter(isMedia)
  const documents = filtered.filter((f) => !isMedia(f))

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-background">
      <div className="flex shrink-0 items-center justify-between border-b border-border px-4 py-3 max-[899px]:px-2.5 max-[899px]:py-2">
        <button type="button" className="inline-flex h-8 items-center gap-1.5 rounded-lg px-2.5 text-sm font-semibold text-muted-foreground transition-colors hover:bg-muted hover:text-foreground max-[899px]:h-[30px] max-[899px]:px-1.5 max-[899px]:text-xs" onClick={onBack}>
          <ArrowLeft className="size-4" />
          Back to messages
        </button>
        <span className="text-xs font-semibold text-muted-foreground max-[899px]:text-[10px]">
          {filtered.length} {filtered.length === 1 ? 'file' : 'files'}
        </span>
      </div>

      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto p-4 max-[899px]:p-2.5">
        <div className="mb-6 flex flex-wrap items-center gap-3 max-[899px]:mb-4 max-[899px]:gap-2">
          <div className="flex h-9 w-56 flex-none items-center gap-2 rounded-lg border border-input bg-muted px-2.5 text-muted-foreground/70 max-[899px]:h-8 max-[899px]:w-full">
            <Search className="size-3.5 shrink-0" />
            <input value={query} placeholder="Search files..." onChange={(e) => setQuery(e.target.value)} className="min-w-0 flex-1 border-none bg-transparent text-[13px] text-foreground outline-none" />
          </div>
          <div className="w-40 max-[899px]:w-[calc(50%-4px)]">
            <Listbox<FileFilter>
              aria-label="Filter files"
              value={filter}
              options={[
                { value: 'all', label: 'All Files' },
                { value: 'media', label: 'Media' },
                { value: 'documents', label: 'Documents' },
              ]}
              onChange={setFilter}
            />
          </div>
          <div className="w-36 max-[899px]:w-[calc(50%-4px)]">
            <Listbox<SortOrder>
              aria-label="Sort files"
              value={sortOrder}
              options={[
                { value: 'latest', label: 'Latest first' },
                { value: 'oldest', label: 'Oldest first' },
              ]}
              onChange={setSortOrder}
            />
          </div>
        </div>

        {filtered.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-1 p-10 text-center text-muted-foreground/70">
            <Folder className="size-12 opacity-30" />
            <h3 className="mt-2 text-[15px] font-semibold text-foreground">No files found</h3>
            <p className="text-[13px] text-muted-foreground">Shared files in this channel will appear here.</p>
          </div>
        ) : (
          <div className="flex flex-col gap-7 max-[899px]:gap-5">
            {media.length > 0 ? (
              <section>
                <h3 className="mb-3 text-[11px] font-bold tracking-wider text-muted-foreground uppercase">Media</h3>
                <div className="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-3 max-[899px]:grid-cols-2 max-[899px]:gap-2">
                  {media.map((file) => (
                    <div key={file.id} className="group relative block aspect-[1.42] overflow-hidden rounded-lg border border-border bg-muted/30 shadow-sm transition-colors hover:border-primary/40" title={file.fileName}>
                      <button
                        type="button"
                        className="absolute inset-0 text-left"
                        aria-label={`Open ${file.fileName}`}
                        onClick={() => (isImage(file) ? setViewer(file) : window.open(file.url, '_blank', 'noopener,noreferrer'))}
                      >
                        {isImage(file) ? (
                          <img src={file.url} alt={file.fileName} loading="lazy" className="size-full object-cover" />
                        ) : (
                          <span className="flex size-full flex-col items-center justify-center gap-2 text-xs font-semibold text-muted-foreground">
                            <Paperclip className="size-8" />
                            <span className="max-w-[80%] truncate">{file.fileName}</span>
                          </span>
                        )}
                      </button>
                      <div className="absolute top-2 right-2 flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100">
                        <a href={file.url} download={file.fileName} className="grid size-7 place-items-center rounded-md border border-white/15 bg-black/55 text-white backdrop-blur-sm transition-colors hover:bg-black/75" title="Download file" aria-label="Download file" onClick={(e) => e.stopPropagation()}>
                          <Download className="size-3.5" />
                        </a>
                        <button type="button" className="grid size-7 place-items-center rounded-md border border-white/15 bg-black/55 text-white backdrop-blur-sm transition-colors hover:bg-destructive" title="Delete file" aria-label="Delete file" onClick={() => setDeleteTarget(file)}>
                          <Trash2 className="size-3.5" />
                        </button>
                      </div>
                      <div className="pointer-events-none absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/70 to-transparent p-2 text-xs font-semibold text-white opacity-0 transition-opacity group-hover:opacity-100">
                        <span className="block truncate">{file.fileName}</span>
                      </div>
                    </div>
                  ))}
                </div>
              </section>
            ) : null}

            {documents.length > 0 ? (
              <section>
                <h3 className="mb-3 text-[11px] font-bold tracking-wider text-muted-foreground uppercase">Documents</h3>
                <div className="flex flex-col gap-2">
                  {documents.map((file) => (
                    <div key={file.id} className="flex items-center gap-3 rounded-lg border border-border bg-muted/15 p-3 transition-colors hover:bg-muted/45 max-[899px]:gap-2 max-[899px]:p-2">
                      <a href={file.url} target="_blank" rel="noreferrer" className="flex min-w-0 flex-1 items-center gap-3 max-[899px]:gap-2">
                        <span className="flex size-10 shrink-0 items-center justify-center rounded-lg border border-border bg-background text-[10px] font-black text-destructive uppercase max-[899px]:size-8 max-[899px]:text-[8px]">{fileExtension(file.fileName)}</span>
                        <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                          <span className="truncate text-sm font-bold text-foreground">{file.fileName}</span>
                          <span className="truncate text-xs text-muted-foreground max-[899px]:text-[10px]">
                            {formatSize(file.fileSize)} <span className="mx-1">·</span> {formatDate(file.createdAt)}
                          </span>
                        </span>
                      </a>
                      <span className="flex min-w-0 items-center gap-2 text-xs font-medium text-muted-foreground max-[899px]:hidden">
                        <span
                          className="flex items-center justify-center overflow-hidden font-bold"
                          style={{ width: 24, height: 24, borderRadius: 9999, fontSize: 10, ...(file.authorColor ? { background: `color-mix(in srgb, ${file.authorColor} 22%, transparent)`, color: file.authorColor } : {}) }}
                        >
                          {file.authorName.charAt(0).toUpperCase()}
                        </span>
                        <span className="max-w-28 truncate">{file.authorName}</span>
                      </span>
                      <a href={file.url} download={file.fileName} className={rowIconBtn} title="Download file" aria-label="Download file">
                        <Download className="size-4" />
                      </a>
                      <button type="button" className={rowIconBtn} data-danger="true" title="Delete file" aria-label="Delete file" onClick={() => setDeleteTarget(file)}>
                        <Trash2 className="size-4" />
                      </button>
                    </div>
                  ))}
                </div>
              </section>
            ) : null}
          </div>
        )}
      </div>

      {viewer ? <ImageViewer attachment={viewer} onClose={() => setViewer(null)} /> : null}
      {deleteTarget ? (
        <ConfirmDeleteModal
          title="Delete file?"
          description={`This will permanently delete ${deleteTarget.fileName}.`}
          onClose={() => setDeleteTarget(null)}
          onConfirm={() => {
            deleteAttachment(deleteTarget.messageId, deleteTarget.id)
            setDeleteTarget(null)
          }}
        />
      ) : null}
    </div>
  )
}
