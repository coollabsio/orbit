import { useState } from 'react'
import { toast } from 'sonner'
import { Link, LinkOff } from 'reicon-react'
import type { MailFileLinkView } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { TitleTip } from '@/components/common/TitleTip'
import { Button } from '@/components/ui/button'
import { formatBytes } from '@/lib/formatBytes'
import { fullDate } from '@/lib/format'
import { useRevokeFileLink } from '@/features/mail/api/mutations'
import { problemText } from '@/features/mail/mailLib'

function linkState(link: MailFileLinkView, now: number): string {
  if (link.revoked) return 'Revoked'
  if (new Date(link.expires_at).getTime() <= now) return `Expired ${fullDate(link.expires_at)}`
  return `Expires ${fullDate(link.expires_at)}`
}

/**
 * The files a sent message carries as secure Orbit links (§9.1 U14), shown only to the member who sent it: size,
 * expiry, downloads, and **Revoke** to stop a link at once.
 */
export function FileLinks({ threadId, messageId, links }: { threadId: string; messageId: string; links: MailFileLinkView[] }) {
  const revoke = useRevokeFileLink()
  // read when the message shows, not on every render
  const [now] = useState(Date.now)
  if (links.length === 0) return null
  return (
    <div data-slot="mail-file-links" className="mt-3 flex flex-col gap-1.5" data-print-hide="">
      <TitleTip
        as="h4"
        title="Sent as links"
        text="These files were too large to attach, so the message has a secure Orbit link for each. Only you see this list. Revoke stops a link at once."
        className="text-xs font-medium text-muted-foreground"
      />
      <ul className="flex flex-col gap-1.5">
        {links.map((link) => {
          const live = !link.revoked && new Date(link.expires_at).getTime() > now
          return (
            <li key={link.id} className="flex min-w-0 items-center gap-2.5 rounded-lg border px-2.5 py-1.5 text-[0.8125rem]">
              {live ? <Link className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" /> : <LinkOff className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />}
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-foreground">{link.filename}</span>
                <span className="text-xs text-muted-foreground">
                  {formatBytes(link.size)} · {linkState(link, now)} · {link.downloads === 1 ? '1 download' : `${link.downloads} downloads`}
                </span>
              </span>
              {live ? (
                <Button
                  size="sm"
                  variant="outline"
                  disabled={revoke.isPending && revoke.variables?.linkId === link.id}
                  onClick={async () => {
                    const confirmed = await confirmAction({
                      title: `Revoke the link to ${link.filename}?`,
                      description: 'People who got the message can no longer download the file. This cannot be undone.',
                      confirmLabel: 'Revoke',
                      danger: true,
                    })
                    if (!confirmed) return
                    revoke.mutate(
                      { threadId, messageId, linkId: link.id },
                      { onError: (error) => toast.error(problemText(error, 'The link could not be revoked.')) },
                    )
                  }}
                >
                  Revoke
                </Button>
              ) : null}
            </li>
          )
        })}
      </ul>
    </div>
  )
}
