import { useNavigate } from 'react-router'
import { PauseCircle } from 'reicon-react'
import type { MailMeMailbox } from '@orbit/api/generated/types.gen'
import { Alert, AlertAction, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'

/**
 * Over the thread list when a mailbox in view is suspended: it refuses new mail, and the mail already in it stays
 * readable. Managers get the way to resume it; members learn whom to ask.
 */
export function MailboxNotice({ mailboxes, canManage }: { mailboxes: MailMeMailbox[]; canManage: boolean }) {
  const navigate = useNavigate()
  const suspended = mailboxes.filter((mailbox) => mailbox.status === 'suspended')
  if (suspended.length === 0) return null
  const [first] = suspended
  const one = suspended.length === 1
  return (
    <Alert data-slot="mailbox-notice" className="m-3 w-auto">
      <PauseCircle />
      <AlertTitle>{one ? `${first.name} is suspended` : `${suspended.length} mailboxes are suspended`}</AlertTitle>
      <AlertDescription>
        {one && first.primary_address ? `New mail to ${first.primary_address} is refused.` : 'New mail to them is refused.'} The
        mail already here stays readable.{canManage ? null : ' Ask an admin to resume it.'}
      </AlertDescription>
      {canManage ? (
        <AlertAction>
          <Button size="sm" variant="outline" onClick={() => navigate('/settings/mail/mailboxes')}>
            Resume
          </Button>
        </AlertAction>
      ) : null}
    </Alert>
  )
}
