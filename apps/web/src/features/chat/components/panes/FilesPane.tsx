import { useState } from 'react'
import { DocumentText, Download, Folder, Message as MessageIcon } from 'reicon-react'
import { ImageViewer } from '@/components/common/ImageViewer'
import { Button, buttonVariants } from '@/components/ui/button'
import { useChatContext } from '@/features/chat/api/chatContext'
import { useFiles } from '@/features/chat/api/queries'
import type { Attachment, Message } from '@/features/chat/api/types'
import { relativeAgo } from '@/features/chat/lib/time'
import { useMembers } from '@/features/workspaces/api'
import { formatSize, isImage } from '@/lib/attachmentLib'
import { RightPane, RightPaneBody, RightPaneEmpty, RightPaneError, RightPaneLoading } from './RightPane'
import { useJumpToMessage } from './useJumpToMessage'

interface SharedFile {
  attachment: Attachment
  message: Message
}

/** Everything shared in a conversation: images as a grid, other files as rows, newest first. */
export function FilesPane({ conversationId }: { conversationId: string }) {
  const { workspaceId } = useChatContext()
  const files = useFiles(conversationId)
  const members = useMembers(workspaceId).data ?? []
  const jump = useJumpToMessage()
  const [viewerImage, setViewerImage] = useState<Attachment | null>(null)

  const shared: SharedFile[] = (files.data ?? []).flatMap((message) => message.attachments.map((attachment) => ({ attachment, message })))
  const images = shared.filter((file) => isImage(file.attachment))
  const others = shared.filter((file) => !isImage(file.attachment))
  const authorName = (message: Message) => members.find((member) => member.id === message.authorId)?.name ?? 'Unknown'

  return (
    <RightPane title="Files">
      <RightPaneBody>
        {files.isPending ? (
          <RightPaneLoading />
        ) : files.isError ? (
          <RightPaneError title="The files could not be loaded." onRetry={() => void files.refetch()} />
        ) : shared.length === 0 ? (
          <RightPaneEmpty icon={Folder} title="No files yet" description="Drop a file in the conversation to share it." />
        ) : (
          <div className="flex flex-col gap-3">
            {images.length > 0 ? (
              <section aria-label="Images">
                <h3 className="px-2 py-1 text-xs text-muted-foreground">Images</h3>
                <ul className="grid grid-cols-3 gap-1 px-2">
                  {images.map(({ attachment, message }) => (
                    <li key={attachment.id} className="group relative aspect-square overflow-hidden rounded-md border">
                      <Button
                        variant="ghost"
                        className="block size-full rounded-none border-0 p-0 active:not-aria-[haspopup]:translate-y-0"
                        aria-label={`Open image ${attachment.fileName}`}
                        title={`${attachment.fileName} · ${authorName(message)} · ${relativeAgo(message.createdAt)}`}
                        onClick={() => setViewerImage(attachment)}
                      >
                        <img src={attachment.url} alt="" loading={attachment.url.startsWith('data:') ? 'eager' : 'lazy'} className="block size-full object-cover" />
                      </Button>
                      <Button
                        variant="secondary"
                        size="icon-xs"
                        className="absolute right-1 bottom-1 focus-visible:opacity-100 hover-fine:opacity-0 hover-fine:group-hover:opacity-100"
                        aria-label={`Go to the message with ${attachment.fileName}`}
                        title="Go to message"
                        onClick={() => jump(message)}
                      >
                        <MessageIcon weight="Filled" />
                      </Button>
                    </li>
                  ))}
                </ul>
              </section>
            ) : null}
            {others.length > 0 ? (
              <section aria-label="Other files">
                <h3 className="px-2 py-1 text-xs text-muted-foreground">Files</h3>
                <ul className="flex flex-col">
                  {others.map(({ attachment, message }) => (
                    <li key={attachment.id} className="flex items-center gap-2 rounded-lg px-2 py-1.5 hover-fine:hover:bg-muted/50">
                      <DocumentText weight="Filled" aria-hidden className="size-5 shrink-0 text-muted-foreground" />
                      <span className="flex min-w-0 flex-1 flex-col">
                        <span className="truncate text-sm font-medium" title={attachment.fileName}>
                          {attachment.fileName}
                        </span>
                        <span className="truncate text-xs text-muted-foreground">
                          {formatSize(attachment.fileSize)} · {authorName(message)} · {relativeAgo(message.createdAt)}
                        </span>
                      </span>
                      <Button variant="ghost" size="icon-sm" aria-label={`Go to the message with ${attachment.fileName}`} title="Go to message" onClick={() => jump(message)}>
                        <MessageIcon weight="Filled" />
                      </Button>
                      <a
                        href={attachment.url}
                        download={attachment.fileName}
                        className={buttonVariants({ variant: 'ghost', size: 'icon-sm' })}
                        aria-label={`Download ${attachment.fileName}`}
                        title="Download"
                      >
                        <Download weight="Filled" />
                      </a>
                    </li>
                  ))}
                </ul>
              </section>
            ) : null}
          </div>
        )}
      </RightPaneBody>
      {viewerImage ? <ImageViewer attachment={viewerImage} onClose={() => setViewerImage(null)} /> : null}
    </RightPane>
  )
}
