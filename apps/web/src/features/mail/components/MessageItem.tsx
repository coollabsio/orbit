import { useState } from 'react'
import { toast } from 'sonner'
import { Code, DocumentDownload, Gallery, More, Printer, ShieldTick } from 'reicon-react'
import type { MailMessageView, MailPerson } from '@/api/generated/types.gen'
import { Attachments } from '@/components/common/Attachments'
import { Modal } from '@/components/common/Modal'
import { Tip } from '@/components/common/Tip'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Skeleton } from '@/components/ui/skeleton'
import { fullDate, timeOfDay } from '@/lib/format'
import { useSetSenderPref } from '@/features/mail/api/mutations'
import { useRawMessage } from '@/features/mail/api/queries'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { firstLine, mailAttachment, personName, problemText, rawMessageUrl } from '@/features/mail/mailLib'
import { MailHtmlFrame } from './MailHtmlFrame'
import { SafetyBanner } from './SafetyBanner'

interface MessageItemProps {
  message: MailMessageView
  expanded: boolean
  onToggle: () => void
  onPrint: () => void
}

const people = (list: MailPerson[]) => list.map((person) => (person.name ? `${person.name} <${person.address}>` : person.address)).join(', ')

function download(url: string) {
  const link = document.createElement('a')
  link.href = url
  link.download = ''
  link.rel = 'noopener'
  document.body.append(link)
  link.click()
  link.remove()
}

/** **Show original**: the message as received, headers and all. */
function OriginalMessage({ messageId, onClose }: { messageId: string; onClose: () => void }) {
  const raw = useRawMessage(messageId)
  return (
    <Modal title="Original message" description="The message as Orbit received it." onClose={onClose} className="sm:max-w-4xl">
      {raw.data ? (
        <>
          <pre className="max-h-[60vh] overflow-auto rounded-md border bg-muted/40 p-3 font-mono text-xs whitespace-pre-wrap break-all">{raw.data.text}</pre>
          {raw.data.truncated ? <p className="text-xs text-muted-foreground">Only the start is shown. Download the .eml file for the whole message.</p> : null}
        </>
      ) : raw.isError ? (
        <p role="alert" className="text-sm text-muted-foreground">
          {problemText(raw.error, 'The original message could not be loaded.')}
        </p>
      ) : (
        <Skeleton className="h-40 w-full" />
      )}
    </Modal>
  )
}

/** One message of a thread: a header that expands it, then the warnings, the body and the attachments. */
export function MessageItem({ message, expanded, onToggle, onPrint }: MessageItemProps) {
  const { workspace } = useWorkspace()
  const [imagesLoaded, setImagesLoaded] = useState(false)
  const [originalOpen, setOriginalOpen] = useState(false)
  const senderPref = useSetSenderPref()
  const loadImages = imagesLoaded || message.load_images_default
  const address = message.from?.address
  const name = personName(message.from)
  const date = message.sent_at ?? message.received_at
  const files = message.attachments.filter((attachment) => !attachment.inline).map((attachment) => mailAttachment(workspace.id, attachment))
  // the view cannot tell how many remote images it removed, so every HTML message offers them until they load
  const offerImages = message.has_html && !loadImages

  return (
    <div data-slot="mail-message" className="py-4 [[data-slot=mail-message]+&]:border-t" data-expanded={expanded || undefined}>
      <div className="flex items-center gap-1">
        <div
          className="-m-1 flex min-w-0 flex-1 cursor-pointer items-center gap-2.5 rounded-md p-1 transition-colors outline-none hover:bg-muted/45 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset"
          role="button"
          tabIndex={0}
          aria-expanded={expanded}
          onClick={onToggle}
          onKeyDown={(event) => {
            if (event.target !== event.currentTarget || (event.key !== 'Enter' && event.key !== ' ')) return
            event.preventDefault()
            onToggle()
          }}
        >
          <UserAvatar user={null} name={name} size={32} />
          <div className="flex min-w-0 flex-col">
            <span className="truncate text-[13px] font-semibold text-foreground">{name}</span>
            <span className="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground/70">
              <span className="truncate">{address}</span>
              {message.auth.aligned_domain ? (
                <span className="inline-flex shrink-0 items-center gap-0.5 text-muted-foreground" title="DMARC confirmed that the message comes from this domain.">
                  <ShieldTick className="size-3.5" />
                  {message.auth.aligned_domain}
                </span>
              ) : null}
            </span>
          </div>
          <span className="flex-1" />
          <span className="shrink-0 text-xs text-muted-foreground/70" title={new Date(message.received_at).toLocaleString()}>
            {fullDate(date)}, {timeOfDay(date)}
          </span>
        </div>
        <DropdownMenu>
          <Tip label="Message actions">
            <DropdownMenuTrigger
              render={<Button variant="ghost" size="icon-sm" className="shrink-0 text-muted-foreground/70" aria-label="Message actions" data-print-hide="" />}
            >
              <More className="size-4" />
            </DropdownMenuTrigger>
          </Tip>
          <DropdownMenuContent align="end" className="w-auto min-w-40">
            <DropdownMenuItem onClick={() => setOriginalOpen(true)}>
              <Code />
              Show original
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => download(rawMessageUrl(workspace.id, message.id))}>
              <DocumentDownload />
              Download .eml
            </DropdownMenuItem>
            <DropdownMenuItem onClick={onPrint}>
              <Printer />
              Print
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {expanded ? (
        <div className="animate-relation-enter motion-reduce:animate-none">
          <div className="mt-2 flex flex-col gap-0.5 pl-[42px] text-xs text-muted-foreground">
            {message.to.length > 0 ? <span className="truncate">To: {people(message.to)}</span> : null}
            {message.cc.length > 0 ? <span className="truncate">Cc: {people(message.cc)}</span> : null}
            {message.sender ? <span className="truncate">Sent by {message.sender}</span> : null}
          </div>
          <SafetyBanner message={message} />
          {offerImages ? (
            <div data-print-hide="" className="mt-3 flex flex-wrap items-center gap-2 rounded-lg border px-2.5 py-1.5 text-[13px] text-muted-foreground">
              <Gallery className="size-4 shrink-0" />
              <span className="min-w-0 flex-1">Remote images are hidden.</span>
              <Button variant="outline" size="sm" onClick={() => setImagesLoaded(true)}>
                Load images
              </Button>
              {message.auth.dmarc === 'pass' && address ? (
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={senderPref.isPending}
                  onClick={() =>
                    senderPref.mutate(
                      { address, load_images: true },
                      {
                        onSuccess: () => {
                          setImagesLoaded(true)
                          toast(`Images from ${address} now load at once.`)
                        },
                        onError: (error) => toast.error(problemText(error, 'The setting could not be saved.')),
                      },
                    )
                  }
                >
                  Always for this sender
                </Button>
              ) : null}
            </div>
          ) : null}
          <div className="mt-3">
            {message.has_html ? (
              <MailHtmlFrame messageId={message.id} loadImages={loadImages} title={message.subject || 'Message'} />
            ) : (
              <div className="text-sm leading-[1.6] break-words whitespace-pre-wrap text-foreground">{message.body_text}</div>
            )}
          </div>
          {files.length > 0 ? <Attachments attachments={files} /> : null}
        </div>
      ) : (
        <div className="mt-1.5 truncate pl-[42px] text-[13px] text-muted-foreground">{firstLine(message.body_text)}</div>
      )}
      {originalOpen ? <OriginalMessage messageId={message.id} onClose={() => setOriginalOpen(false)} /> : null}
    </div>
  )
}
