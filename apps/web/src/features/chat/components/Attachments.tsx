// Port of the chat reference Attachments: image grid (1 / 2 / 3-hero / 4+N) that opens the ImageViewer,
// plus file cards for non-images (paperclip, name, size, download on hover).
import { useState } from 'react'
import { Download, Paperclip2 } from 'reicon-react'
import type { Attachment } from '../../../mock/types'
import { formatSize, isImage } from '../attachmentLib'
import { ImageViewer } from './ImageViewer'

export function Attachments({ attachments, hasTextContent = true }: { attachments: Attachment[]; hasTextContent?: boolean }) {
  const [viewerImage, setViewerImage] = useState<Attachment | null>(null)
  if (attachments.length === 0) return null

  const images = attachments.filter(isImage)
  const otherFiles = attachments.filter((att) => !isImage(att))

  return (
    <div className="fc-attachments" data-no-text={hasTextContent ? undefined : 'true'}>
      {images.length > 0 ? (
        <div className="fc-attachment-grid" data-count={Math.min(images.length, 4)}>
          {images.slice(0, 4).map((att, index) => (
            <div
              key={att.id}
              className="fc-attachment-image"
              data-single={images.length === 1 || undefined}
              data-hero={images.length === 3 && index === 0 ? 'true' : undefined}
            >
              <button type="button" aria-label={`Open image ${att.fileName}`} onClick={() => setViewerImage(att)}>
                <img src={att.url} alt={att.fileName} loading={att.url.startsWith('data:') ? 'eager' : 'lazy'} />
              </button>
              {images.length > 4 && index === 3 ? <div className="fc-attachment-more">+{images.length - 4}</div> : null}
            </div>
          ))}
        </div>
      ) : null}

      {otherFiles.length > 0 ? (
        <div className="fc-file-cards">
          {otherFiles.map((att) => (
            <a key={att.id} href={att.url} download={att.fileName} className="fc-file-card">
              <Paperclip2 size={40} />
              <span className="fc-file-card-text">
                <span className="fc-file-card-name">{att.fileName}</span>
                <span className="fc-file-card-size">{formatSize(att.fileSize)}</span>
              </span>
              <span className="fc-file-card-download">
                <Download size={20} />
              </span>
            </a>
          ))}
        </div>
      ) : null}

      {viewerImage ? <ImageViewer attachment={viewerImage} onClose={() => setViewerImage(null)} /> : null}
    </div>
  )
}
