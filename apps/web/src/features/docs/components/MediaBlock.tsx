import { useState } from 'react'
import { Download, Global, Paperclip2, Xmark } from 'reicon-react'
import { appNavigate } from '../../../lib/navigateBridge'
import type { DocBlock } from '../../../mock/types'
import { formatSize } from '../../chat/attachmentLib'
import { ImageViewer } from '../../chat/components/ImageViewer'

function domainOf(url: string): string {
  try {
    return new URL(url, window.location.origin).hostname || url
  } catch {
    return url
  }
}

/** Embed (bookmark card), image and file blocks; all removable on hover. */
export function MediaBlock({ block, onRemove }: { block: DocBlock; onRemove: () => void }) {
  const [viewerOpen, setViewerOpen] = useState(false)

  const remove = (
    <button
      type="button"
      className="doc-media-remove"
      aria-label="Remove block"
      title="Remove"
      onClick={(e) => {
        e.preventDefault()
        e.stopPropagation()
        onRemove()
      }}
    >
      <Xmark size={12} />
    </button>
  )

  if (block.type === 'image' && block.url) {
    return (
      <div className="doc-block doc-media">
        <button type="button" className="doc-image" aria-label={`Open image ${block.fileName ?? ''}`} onClick={() => setViewerOpen(true)}>
          <img
            src={block.url}
            alt={block.fileName ?? ''}
            loading={block.url.startsWith('data:') ? 'eager' : 'lazy'}
          />
        </button>
        {remove}
        {viewerOpen ? (
          <ImageViewer
            attachment={{
              id: block.id,
              fileName: block.fileName ?? 'image',
              mimeType: block.mimeType ?? 'image/*',
              fileSize: block.fileSize ?? 0,
              url: block.url,
            }}
            onClose={() => setViewerOpen(false)}
          />
        ) : null}
      </div>
    )
  }

  if (block.type === 'file' && block.url) {
    return (
      <div className="doc-block doc-media doc-media-file">
        <a href={block.url} download={block.fileName} className="fc-file-card">
          <Paperclip2 size={22} />
          <span className="fc-file-card-text">
            <span className="fc-file-card-name">{block.fileName}</span>
            <span className="fc-file-card-size">{formatSize(block.fileSize ?? 0)}</span>
          </span>
          <span className="fc-file-card-download">
            <Download size={16} />
          </span>
        </a>
        {remove}
      </div>
    )
  }

  // embed: bookmark card; links into this app navigate in place
  const url = block.text.trim()
  const origin = window.location.origin
  const internal = url.startsWith(`${origin}/`) ? url.slice(origin.length) : url.startsWith('/') ? url : null
  return (
    <div className="doc-block doc-media">
      <a
        className="doc-embed"
        href={internal ?? url}
        target={internal ? undefined : '_blank'}
        rel={internal ? undefined : 'noopener noreferrer'}
        onClick={
          internal
            ? (e) => {
                e.preventDefault()
                appNavigate(internal)
              }
            : undefined
        }
      >
        <span className="doc-embed-icon">
          <Global size={18} />
        </span>
        <span className="doc-embed-text">
          <span className="doc-embed-title">{domainOf(url)}</span>
          <span className="doc-embed-url">{url}</span>
        </span>
      </a>
      {remove}
    </div>
  )
}
