// Port of the chat reference FilesView: replaces the message area; search + kind filter + sort,
// Media grid cards and Document rows with download / delete.
import { useMemo, useState } from 'react'
import { ArrowLeft2, Download, Folder, Magnifier, Paperclip2, Trash } from 'reicon-react'
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
    <div className="fc-files">
      <div className="fc-files-bar">
        <button type="button" className="fc-files-back" onClick={onBack}>
          <ArrowLeft2 size={16} />
          Back to messages
        </button>
        <span className="fc-files-count">
          {filtered.length} {filtered.length === 1 ? 'file' : 'files'}
        </span>
      </div>

      <div className="fc-files-body">
        <div className="fc-files-toolbar">
          <div className="fc-threads-search fc-files-search">
            <Magnifier size={14} />
            <input value={query} placeholder="Search files..." onChange={(e) => setQuery(e.target.value)} />
          </div>
          <Listbox<FileFilter>
            className="fc-files-filter"
            aria-label="Filter files"
            value={filter}
            options={[
              { value: 'all', label: 'All Files' },
              { value: 'media', label: 'Media' },
              { value: 'documents', label: 'Documents' },
            ]}
            onChange={setFilter}
          />
          <Listbox<SortOrder>
            className="fc-files-sort"
            aria-label="Sort files"
            value={sortOrder}
            options={[
              { value: 'latest', label: 'Latest first' },
              { value: 'oldest', label: 'Oldest first' },
            ]}
            onChange={setSortOrder}
          />
        </div>

        {filtered.length === 0 ? (
          <div className="fc-threads-empty" style={{ flex: 1, justifyContent: 'center' }}>
            <Folder size={48} style={{ opacity: 0.3 }} />
            <h3>No files found</h3>
            <p>Shared files in this channel will appear here.</p>
          </div>
        ) : (
          <div className="fc-files-sections">
            {media.length > 0 ? (
              <section>
                <h3 className="fc-files-section-title">Media</h3>
                <div className="fc-files-grid">
                  {media.map((file) => (
                    <div key={file.id} className="fc-files-card" title={file.fileName}>
                      <button
                        type="button"
                        className="fc-files-card-open"
                        aria-label={`Open ${file.fileName}`}
                        onClick={() => (isImage(file) ? setViewer(file) : window.open(file.url, '_blank', 'noopener,noreferrer'))}
                      >
                        {isImage(file) ? (
                          <img src={file.url} alt={file.fileName} loading="lazy" />
                        ) : (
                          <span className="fc-files-card-generic">
                            <Paperclip2 size={32} />
                            <span className="truncate">{file.fileName}</span>
                          </span>
                        )}
                      </button>
                      <div className="fc-files-card-actions">
                        <a href={file.url} download={file.fileName} className="fc-files-card-action" title="Download file" aria-label="Download file" onClick={(e) => e.stopPropagation()}>
                          <Download size={14} />
                        </a>
                        <button type="button" className="fc-files-card-action" data-danger="true" title="Delete file" aria-label="Delete file" onClick={() => setDeleteTarget(file)}>
                          <Trash size={14} />
                        </button>
                      </div>
                      <div className="fc-files-card-name">
                        <span className="truncate">{file.fileName}</span>
                      </div>
                    </div>
                  ))}
                </div>
              </section>
            ) : null}

            {documents.length > 0 ? (
              <section>
                <h3 className="fc-files-section-title">Documents</h3>
                <div className="fc-files-rows">
                  {documents.map((file) => (
                    <div key={file.id} className="fc-files-row">
                      <a href={file.url} target="_blank" rel="noreferrer" className="fc-files-row-main">
                        <span className="fc-files-row-ext">{fileExtension(file.fileName)}</span>
                        <span className="fc-threads-row-body">
                          <span className="fc-threads-row-title">{file.fileName}</span>
                          <span className="fc-files-row-meta">
                            {formatSize(file.fileSize)} <span>·</span> {formatDate(file.createdAt)}
                          </span>
                        </span>
                      </a>
                      <span className="fc-files-row-author">
                        <span
                          className="avatar-tile"
                          style={{ width: 24, height: 24, borderRadius: 9999, fontSize: 10, ...(file.authorColor ? { background: `color-mix(in srgb, ${file.authorColor} 22%, transparent)`, color: file.authorColor } : {}) }}
                        >
                          {file.authorName.charAt(0).toUpperCase()}
                        </span>
                        <span className="truncate">{file.authorName}</span>
                      </span>
                      <a href={file.url} download={file.fileName} className="fc-thread-icon-button" title="Download file" aria-label="Download file">
                        <Download size={16} />
                      </a>
                      <button type="button" className="fc-thread-icon-button" data-danger="true" title="Delete file" aria-label="Delete file" onClick={() => setDeleteTarget(file)}>
                        <Trash size={16} />
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
