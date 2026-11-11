import type { ComponentType, ReactNode } from 'react'
import { Danger, DocumentText, Global, ProfileCircle, ShieldCross } from 'reicon-react'
import type { MailMessageView } from '@orbit/api/generated/types.gen'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'

const domainOf = (address: string | undefined) => address?.split('@').pop() ?? ''

function Banner({
  kind,
  icon: Icon,
  title,
  variant,
  children,
}: {
  kind: string
  icon: ComponentType<{ className?: string }>
  title: string
  variant?: 'destructive'
  children: ReactNode
}) {
  return (
    <Alert variant={variant} data-kind={kind}>
      <Icon />
      <AlertTitle>{title}</AlertTitle>
      <AlertDescription>{children}</AlertDescription>
    </Alert>
  )
}

/**
 * The warnings over a message (spec §8.3): failed authentication (DMARC), a sender that uses a member's name, a
 * message from outside that joined a conversation started inside, an external sender, and a message that could not
 * be read in full.
 */
export function SafetyBanner({ message }: { message: MailMessageView }) {
  const address = message.from?.address
  const domain = domainOf(address)
  const banners: ReactNode[] = []
  if (message.auth.dmarc === 'fail') {
    banners.push(
      <Banner key="auth" kind="auth" icon={ShieldCross} variant="destructive" title="Failed authentication">
        This message says it is from {domain || 'a domain'}, but that domain did not confirm it (DMARC). It may be forged: do
        not trust its links or requests.
      </Banner>,
    )
  }
  if (message.lookalike_member) {
    banners.push(
      <Banner key="lookalike" kind="lookalike" icon={ProfileCircle} variant="destructive" title="Lookalike name">
        The sender uses the name of {message.lookalike_member}, but {address ?? 'this address'} is not theirs.
      </Banner>,
    )
  }
  if (message.external) {
    banners.push(
      <Banner key="joined" kind="joined" icon={Danger} title="Outside reply">
        This message came from outside the workspace into a conversation that started inside it. Check the sender before
        you act on it.
      </Banner>,
    )
  } else if (message.from_external) {
    banners.push(
      <Banner key="external" kind="external" icon={Global} title="External sender">
        {domain || 'The sender'} is outside this workspace. Be careful with links and attachments.
      </Banner>,
    )
  }
  if (message.warning) {
    banners.push(
      <Banner key="warning" kind="warning" icon={DocumentText} title="Shown as plain text">
        {message.warning === 'limit'
          ? 'This message is too large or too complex to show in full.'
          : 'This message could not be read in full.'}{' '}
        Download the original to see all of it.
      </Banner>,
    )
  }
  if (banners.length === 0) return null
  return <div className="mt-3 flex flex-col gap-2">{banners}</div>
}
