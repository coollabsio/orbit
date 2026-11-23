import { useState } from 'react'
import { toast } from 'sonner'
import { ArrowDown2 as ChevronDown, Clock, CloseCircle, Refresh, TickCircle, Warning } from 'reicon-react'
import { cn } from 'cn'
import type { MailOutboundRecipient } from '@orbit/api/generated/types.gen'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import { useRetryRecipient } from '@/features/mail/api/mutations'
import {
  bounceReason,
  type DeliveryLabel,
  deliverySummary,
  RECIPIENT_LABEL,
  recipientState,
  recipientTag,
  retryErrorText,
  type RecipientState,
  SUPPRESSED_REASON,
} from '@/features/mail/mailStatus'

/** The longest server reply a tooltip shows. */
const RESPONSE_CHARS = 600

const TONE: Record<RecipientState, string> = {
  sending: 'text-muted-foreground',
  delivered: 'text-emerald-600 dark:text-emerald-400',
  delayed: 'text-amber-600 dark:text-amber-400',
  bounced: 'text-destructive',
  suppressed: 'text-destructive',
  cancelled: 'text-muted-foreground',
}

const SUMMARY_STATE: Record<DeliveryLabel, RecipientState> = {
  Sending: 'sending',
  Delivered: 'delivered',
  Delayed: 'delayed',
  Bounced: 'bounced',
  Suppressed: 'suppressed',
  'Partly delivered': 'bounced',
}

function StateIcon({ state, className }: { state: RecipientState; className?: string }) {
  const Icon = state === 'delivered' ? TickCircle : state === 'delayed' ? Warning : state === 'bounced' || state === 'suppressed' ? CloseCircle : Clock
  return <Icon className={cn('size-3.5 shrink-0', TONE[state], className)} aria-hidden="true" />
}

const HELP =
  'Delivered means the next mail server accepted the message. Orbit does not track whether it was opened. Delayed: the receiving server has not accepted it for more than 4 hours; Orbit keeps trying for 5 days.'

/**
 * The delivery of a sent message (§9.1 U10): one quiet line with the summary (the help text on hover). With more than
 * one recipient, a click opens a status for each; the list opens by itself when something needs attention. A recipient
 * that bounced says why (the server's reply in a tooltip, as text) and offers **Retry**.
 */
export function DeliveryStatus({ outboundId, recipients }: { outboundId: string; recipients: MailOutboundRecipient[] }) {
  const { label, bounced } = deliverySummary(recipients)
  const shown = recipients.filter((recipient) => recipient.status !== 'cancelled')
  const attention = label !== 'Sending' && label !== 'Delivered'
  // `null`: follows the status (open while something needs attention); a click decides from then on
  const [chosen, setChosen] = useState<boolean | null>(null)
  const open = chosen ?? attention
  if (shown.length === 0) return null
  const state = SUMMARY_STATE[label]
  const many = shown.length > 1
  const failed = shown.filter((recipient) => ['bounced', 'suppressed'].includes(recipientState(recipient))).length
  const summary = (
    <>
      <StateIcon state={state} />
      <Tip label={HELP}>
        <span className={cn('cursor-default', TONE[state])}>{label}</span>
      </Tip>
      {many && failed > 0 && label === 'Partly delivered' ? <span className="text-muted-foreground">· {failed} not delivered</span> : null}
      {many && bounced.length > 0 && label !== 'Partly delivered' ? <span className="text-muted-foreground">· {bounced.length} bounced</span> : null}
    </>
  )

  return (
    <div data-slot="mail-delivery" className="mt-2 flex flex-col gap-1 pl-[42px] text-xs" data-print-hide="">
      {many ? (
        <button
          type="button"
          className="-mx-1 inline-flex items-center gap-1.5 self-start rounded-md px-1 py-0.5 outline-none hover:bg-muted/45 focus-visible:ring-2 focus-visible:ring-ring/50"
          aria-expanded={open}
          onClick={() => setChosen(!open)}
        >
          {summary}
          <ChevronDown className={cn('size-3 text-muted-foreground transition-transform duration-150', open && 'rotate-180')} aria-hidden="true" />
        </button>
      ) : (
        <div className="flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-0.5 py-0.5">
          {summary}
          <RecipientProblem outboundId={outboundId} recipient={shown[0]} />
        </div>
      )}
      {many && open ? (
        <ul className="flex flex-col gap-1" aria-label="Delivery to each recipient">
          {shown.map((recipient) => {
            const recipientNow = recipientState(recipient)
            return (
              <li key={recipient.id} className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5">
                <StateIcon state={recipientNow} />
                <span className="min-w-0 truncate text-foreground/90">{recipient.address}</span>
                {recipientTag(recipient.kind) ? (
                  <span className="rounded border px-1 text-[0.625rem] leading-4 text-muted-foreground">{recipientTag(recipient.kind)}</span>
                ) : null}
                <span className={TONE[recipientNow]}>{RECIPIENT_LABEL[recipientNow]}</span>
                <RecipientProblem outboundId={outboundId} recipient={recipient} />
              </li>
            )
          })}
        </ul>
      ) : null}
    </div>
  )
}

/** Why a recipient was not delivered (the server's reply in a tooltip, as text, never as HTML) and its **Retry**. */
function RecipientProblem({ outboundId, recipient }: { outboundId: string; recipient: MailOutboundRecipient }) {
  const retry = useRetryRecipient()
  const state = recipientState(recipient)
  if (state !== 'bounced' && state !== 'suppressed') return null
  const reason = state === 'bounced' ? bounceReason(recipient.bounce_kind, recipient.last_response) : SUPPRESSED_REASON
  const response = recipient.last_response?.trim().slice(0, RESPONSE_CHARS)
  return (
    <>
      {response && recipient.bounce_kind !== 'rejected' ? (
        <Tip label={<span className="block max-w-80 font-mono text-[0.6875rem] break-words whitespace-pre-wrap">{response}</span>}>
          <span tabIndex={0} className="min-w-0 cursor-help text-muted-foreground underline decoration-dotted underline-offset-2">
            {reason}
          </span>
        </Tip>
      ) : (
        <span className="min-w-0 text-muted-foreground">{reason}</span>
      )}
      {recipient.retryable ? (
        <Tip label={state === 'suppressed' ? 'Remove the address from the suppression list and send again' : 'Send to this address again'}>
          <Button
            size="xs"
            variant="outline"
            disabled={retry.isPending}
            onClick={() =>
              retry.mutate(
                { outboundId, recipientId: recipient.id },
                {
                  onSuccess: () => toast(`Sending to ${recipient.address} again.`),
                  onError: (error) => toast.error(retryErrorText(error)),
                },
              )
            }
          >
            <Refresh />
            Retry
          </Button>
        </Tip>
      ) : null}
    </>
  )
}
