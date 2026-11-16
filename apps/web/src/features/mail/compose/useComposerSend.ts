// Sending from the composer: the readiness and recipient checks, the pre-send warnings (§9.1 U5: the first try shows
// them, the next one sends anyway), saving the draft as it is, the send itself, and the offer to send large files as
// links when the server refuses the size (§5.9).
import { useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import type { MailDraft, MailFromAddress, MailOutbound } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { problemText } from '../mailLib'
import { useSaveDraft, useSendDraft } from '../api/mutations'
import { draftCheckQuery, useFromAddresses, useMailMe, useThread } from '../api/queries'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import {
  isValidAddress,
  linkRefusal,
  MESSAGE_OVERHEAD_BYTES,
  offerLinks,
  preSendWarnings,
  recipientsKey,
  sizeCheck,
  unusedInlineFiles,
  type SuppressedRecipient,
  type TooLarge,
  type Warning,
} from './composeLib'

/** The composer's fields as they are when Send is pressed (typed recipients committed, the body exported). */
export interface ComposedMessage {
  to: string[]
  cc: string[]
  bcc: string[]
  subject: string
  fromId: string
  html: string
  /** The body as plain text, without code blocks. */
  text: string
  /** Typed recipients were added just now: warnings shown before are stale. */
  recipientsChanged?: boolean
}

export interface ComposerSendOptions {
  draft: MailDraft
  saver: ReturnType<typeof useSaveDraft>
  /** Why Send is off (not-ready address, draft being sent elsewhere, uploads running), or `null`. */
  blocked: string | null
  fromAddresses: readonly MailFromAddress[]
  collect: () => ComposedMessage
  /** Removes draft files (inline images no longer in the text); rejects when one could not be removed. */
  removeFiles: (ids: string[]) => Promise<unknown>
  /** The draft's files as cached now (after removals). */
  draftFiles: () => MailDraft['files']
  /** No recipient, or one is invalid: show the fields. */
  onRecipientProblem: () => void
  onSent?: (outbound: MailOutbound, draft: MailDraft) => void
  onClose: () => void
}

const utf8Length = (text: string) => new TextEncoder().encode(text).length
const sizedFiles = (files: MailDraft['files']) =>
  files.map((file) => ({ id: file.id, size: file.size, asLink: file.as_link, linkable: linkRefusal(file) === null }))

/** The workspace's mail domains as a member can know them: own addresses, the mailboxes, inside senders. */
function workspaceDomains(addresses: readonly string[], messages: readonly { from?: { address: string } | null; from_external: boolean }[] | undefined) {
  const domains = new Set<string>()
  const add = (address: string) => {
    const domain = address.trim().toLowerCase().split('@')[1]
    if (domain) domains.add(domain)
  }
  addresses.forEach(add)
  for (const message of messages ?? []) if (message.from && !message.from_external) add(message.from.address)
  return [...domains]
}

export function useComposerSend({ draft, saver, blocked, fromAddresses, collect, removeFiles, draftFiles, onRecipientProblem, onSent, onClose }: ComposerSendOptions) {
  const queryClient = useQueryClient()
  const workspaceId = useWorkspace().workspace.id
  const [warnings, setWarnings] = useState<Warning[]>([])
  const [notice, setNotice] = useState<string | null>(null)
  const [tooLarge, setTooLarge] = useState<TooLarge | null>(null)
  const [sending, setSending] = useState(false)
  const mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])
  const sendDraft = useSendDraft()
  const thread = useThread(draft.thread_id ?? null)
  const allFrom = useFromAddresses()
  const me = useMailMe()

  /** A field or the files changed: the next send checks again. */
  const recheck = () => {
    setWarnings([])
    setNotice(null)
  }

  /** The saved draft's suppressed recipients (the check reads the saved draft); none when the check fails. */
  const suppressedOf = async (message: ComposedMessage): Promise<SuppressedRecipient[]> => {
    try {
      const check = await queryClient.fetchQuery(draftCheckQuery(workspaceId, draft.id, recipientsKey(message.to, message.cc, message.bcc)))
      return check.suppressed
    } catch {
      return []
    }
  }

  const warningsFor = (message: ComposedMessage, fileCount: number, suppressed: SuppressedRecipient[]) => {
    const messages = thread.data?.messages
    return preSendWarnings({
      text: message.text,
      subject: message.subject,
      fileCount,
      mode: draft.mode,
      to: message.to,
      cc: message.cc,
      bcc: message.bcc,
      threadAddresses: messages ? messages.flatMap((entry) => [entry.from, ...entry.to, ...entry.cc]).flatMap((person) => (person ? [person.address] : [])) : null,
      workspaceDomains: workspaceDomains(
        [...(allFrom.data ?? fromAddresses).map((address) => address.address), ...(me.data?.mailboxes.map((mailbox) => mailbox.primary_address ?? '') ?? [])],
        messages,
      ),
      suppressed,
    })
  }

  const send = async (scheduledAt?: Date) => {
    if (sending) return
    if (blocked) {
      setNotice(blocked)
      return
    }
    const message = collect()
    const recipients = [...message.to, ...message.cc, ...message.bcc]
    if (recipients.length === 0 || recipients.some((address) => !isValidAddress(address))) {
      setNotice(recipients.length === 0 ? 'Add a recipient.' : 'Fix the address marked in red.')
      onRecipientProblem()
      return
    }
    setSending(true)
    setNotice(null)
    setTooLarge(null)
    const bodyBytes = MESSAGE_OVERHEAD_BYTES + utf8Length(message.subject) + utf8Length(message.html)
    // The server sends every inline file: images deleted from the text are removed, but only once every check has
    // passed (until then ⌘Z can bring the block back). The checks count the files the message will have.
    const unused = unusedInlineFiles(draft.files, message.html)
    const files = draft.files.filter((file) => !unused.includes(file.id))
    try {
      // over the route's limit: it would be refused, so offer links now
      const limit = fromAddresses.find((address) => address.address_id === message.fromId)?.max_message_bytes
      const over = sizeCheck(sizedFiles(files), bodyBytes, limit)
      if (over) {
        setTooLarge(over)
        return
      }
      const before = saver.current().restore
      saver.save({ to: message.to, cc: message.cc, bcc: message.bcc, subject: message.subject, from_address_id: message.fromId, body_html: message.html })
      await saver.flush()
      const after = saver.current()
      if (after.problem || after.restore !== before) {
        setNotice(
          after.problem === 'sending'
            ? 'This draft is being sent.'
            : after.problem === 'error'
              ? 'The draft could not be saved. Try again.'
              : 'This draft changed on another device. Check it, then send again.',
        )
        return
      }
      // U5: the first try shows the checks, the next one sends anyway (the server still skips suppressed recipients)
      if (warnings.length === 0 || message.recipientsChanged) {
        const found = warningsFor(message, files.length, await suppressedOf(message))
        if (found.length > 0) {
          if (mounted.current) setWarnings(found)
          return
        }
      }
      if (unused.length > 0) {
        // a failed delete stops the send; the files that were deleted leave the cached draft, the others stay
        const results = await Promise.allSettled(unused.map((id) => removeFiles([id])))
        if (results.some((result) => result.status === 'rejected')) {
          toast.error('An image removed from the text could not be deleted. Try sending again.')
          return
        }
      }
      const outbound = await sendDraft.mutateAsync({ draftId: draft.id, body: scheduledAt ? { scheduled_at: scheduledAt.toISOString() } : undefined })
      saver.stop()
      const sent: MailDraft = { ...draft, files, to: message.to, cc: message.cc, bcc: message.bcc, subject: message.subject, from_address_id: message.fromId, body_html: message.html }
      if (onSent) onSent(outbound, sent)
      else toast.success(outbound.scheduled ? 'Message scheduled' : 'Message sent')
      onClose()
    } catch (error) {
      if (!mounted.current) return
      if (error instanceof ApiProblem && error.code === 'message_too_large') {
        // the server says its limit and the size it counted
        const sizes = error.problem as { limit_bytes?: number | null; size_bytes?: number | null }
        // the files as they are now (inline images may have been removed just before)
        const offer = offerLinks(sizedFiles(draftFiles()), bodyBytes, sizes.limit_bytes)
        setTooLarge({ ...offer, size: sizes.size_bytes || offer.size })
      } else {
        setNotice(problemText(error, 'The message could not be sent. Try again.'))
      }
    } finally {
      if (mounted.current) setSending(false)
    }
  }

  return { send, sending, warnings, notice, tooLarge, clearTooLarge: () => setTooLarge(null), recheck }
}
