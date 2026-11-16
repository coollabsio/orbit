import { useState } from 'react'
import { toast } from 'sonner'
import { PauseCircle, Sun } from 'reicon-react'
import type { MailMeMailbox } from '@orbit/api/generated/types.gen'
import { Alert, AlertAction, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { fullDate, timeOfDay } from '@/lib/format'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useSetVacation } from '@/features/mail/api/mutations'
import { useSendPolicy, useVacation } from '@/features/mail/api/queries'
import { problemText } from '@/features/mail/mailLib'
import { vacationState } from '@/features/mail/mailStatus'

/** The vacation reply of one mailbox, while it is on (§9.1 U13), with **Turn off**. */
function VacationBanner({ mailbox, canManage, several }: { mailbox: MailMeMailbox; canManage: boolean; several: boolean }) {
  const vacation = useVacation(mailbox.id)
  const save = useSetVacation()
  // read when the banner shows: a reply that ends while the page is open goes with the next read
  const [now] = useState(() => new Date())
  const data = vacation.data
  if (!data || vacationState(data, now) !== 'on') return null
  // the owner of a personal mailbox, or a mail manager for a shared one
  const mayChange = mailbox.kind === 'personal' || canManage
  const until = data.ends_at ? ` until ${fullDate(data.ends_at)}, ${timeOfDay(data.ends_at)}` : ''
  return (
    <Alert data-slot="vacation-banner">
      <Sun />
      <AlertTitle>{several ? `Vacation reply is on for ${mailbox.name}` : 'Vacation reply is on'}</AlertTitle>
      <AlertDescription>
        People who write to {mailbox.primary_address ?? mailbox.name} get {mailbox.kind === 'personal' ? 'your' : 'the'} automatic reply{until}.
      </AlertDescription>
      {mayChange ? (
        <AlertAction>
          <Button
            size="sm"
            variant="outline"
            disabled={save.isPending}
            onClick={() =>
              save.mutate(
                {
                  mailboxId: mailbox.id,
                  // the server replaces every field: send the rest as it is
                  body: {
                    enabled: false,
                    starts_at: data.starts_at ?? null,
                    ends_at: data.ends_at ?? null,
                    subject: data.subject,
                    body_html: data.body_html,
                    contacts_only: data.contacts_only,
                  },
                },
                {
                  onSuccess: () => toast('Vacation reply turned off.'),
                  onError: (error) => toast.error(problemText(error, 'The vacation reply could not be turned off.')),
                },
              )
            }
          >
            Turn off
          </Button>
        </AlertAction>
      ) : null}
    </Alert>
  )
}

/** A banner for each mailbox in view whose vacation reply is on. */
export function VacationBanners({ mailboxes, canManage }: { mailboxes: MailMeMailbox[]; canManage: boolean }) {
  return mailboxes.map((mailbox) => <VacationBanner key={mailbox.id} mailbox={mailbox} canManage={canManage} several={mailboxes.length > 1} />)
}

/** Owners and admins: the workspace's sending is paused (bounces or complaints), and why. */
export function PausedBanner() {
  const { workspace } = useWorkspace()
  const policy = useSendPolicy(workspace.role === 'owner' || workspace.role === 'admin')
  if (!policy.data?.paused_at) return null
  return (
    <Alert data-slot="paused-banner" variant="destructive">
      <PauseCircle />
      <AlertTitle>Sending is paused for this workspace</AlertTitle>
      <AlertDescription>
        {policy.data.pause_reason ? `${policy.data.pause_reason} ` : null}
        New messages wait in the Outbox. Ask the instance admin to resume sending.
      </AlertDescription>
    </Alert>
  )
}
