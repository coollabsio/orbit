import type { MailDraft } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { router, useLocalSearchParams } from 'expo-router'
import { useEffect, useRef, useState } from 'react'
import { Alert, ScrollView, View } from 'react-native'

import { RecipientField } from '../../components/mail/RecipientField'
import { mailKeys } from '../../data/keys'
import * as mail from '../../data/mail-api'
import type { MailFromAddress } from '../../data/mail-api'
import {
  commitTyped,
  type ComposeFields,
  draftChange,
  initialFields,
  isUntouched,
  isValidAddress,
  notReadyText,
  preSendWarnings,
  serverFields,
  swapSignature,
} from '../../data/mail-compose'
import { refreshSendViews, setPendingSend, useFromAddresses, useThread } from '../../hooks/mail'
import { useOrbit } from '../../orbit-context'
import { errorMessage } from '../../session-context'
import { useTheme } from '../../theme'
import { Button, ErrorView, Icon, IconButton, KeyboardSpace, LoadingView, PaneHeader, PickerSheet, SafeAreaView, Text } from '../../ui'
import { TextInput } from '../../ui/scaled'

/** The wait before a change is saved: typing makes one save, not one per key (the web's `DRAFT_SAVE_DELAY_MS`). */
const SAVE_DELAY_MS = 2000

/** A new message, a reply or a forward: the draft opens here, saves itself as it changes, and sends. */
export default function ComposeScreen() {
  const { draft: draftId } = useLocalSearchParams<{ draft: string }>()
  const { client, workspaceId } = useOrbit()
  const draft = useQuery({ queryKey: mailKeys.draft(workspaceId, draftId), queryFn: () => mail.draft(client, workspaceId, draftId), staleTime: Infinity })
  const addresses = useFromAddresses(draft.data?.thread_id ?? undefined)
  if (!draft.data || !addresses.data) {
    const failed = draft.isError ? draft : addresses.isError ? addresses : null
    return (
      <SafeAreaView className="flex-1 bg-background">
        <PaneHeader className="px-2">
          <IconButton icon="close" label="Close" onPress={() => router.back()} />
        </PaneHeader>
        {failed ? <ErrorView error={failed.error} onRetry={() => void failed.refetch()} /> : <LoadingView />}
      </SafeAreaView>
    )
  }
  return <Composer key={draftId} draft={draft.data} addresses={addresses.data} />
}

type SaveProblem = 'conflict' | 'sending' | 'error'

function Composer({ draft, addresses }: { draft: MailDraft; addresses: MailFromAddress[] }) {
  const { client, workspaceId } = useOrbit()
  const { colors, scheme } = useTheme()
  const queryClient = useQueryClient()
  const thread = useThread(draft.thread_id ?? '')
  const [opened] = useState(() => initialFields(draft, addresses))
  const [fields, setFields] = useState<ComposeFields>(opened)
  const [typed, setTyped] = useState({ to: '', cc: '', bcc: '' })
  const [copies, setCopies] = useState(draft.cc.length > 0 || draft.bcc.length > 0)
  const [picking, setPicking] = useState(false)
  const [warnings, setWarnings] = useState<string[]>([])
  const [notice, setNotice] = useState<string | null>(null)
  const [sending, setSending] = useState(false)

  // the save state machine: what the server has, its version, the save in flight and the next one's timer
  const saved = useRef(serverFields(draft))
  const version = useRef(draft.version)
  const inflight = useRef<Promise<void> | null>(null)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const problem = useRef<SaveProblem | null>(null)
  const latest = useRef(fields)
  latest.current = fields

  /** Saves what differs from the server copy, one save at a time. */
  const saveNow = async (): Promise<void> => {
    if (timer.current) clearTimeout(timer.current)
    timer.current = null
    if (inflight.current) await inflight.current
    if (problem.current === 'sending') return
    const now = latest.current
    const change = draftChange(saved.current, now)
    if (Object.keys(change).length === 0) return
    const run = mail.saveDraft(client, workspaceId, draft.id, { ...change, version: version.current }).then(
      (next) => {
        version.current = next.version
        saved.current = now
        problem.current = null
        queryClient.setQueryData(mailKeys.draft(workspaceId, draft.id), next)
      },
      (error: unknown) => {
        const current = error instanceof ApiProblem && error.status === 409 && error.code === 'draft_conflict' ? (error.problem.conflict as { current?: MailDraft } | null | undefined)?.current : undefined
        if (current) {
          // someone saved a newer version: it replaces this text
          problem.current = 'conflict'
          version.current = current.version
          saved.current = serverFields(current)
          setFields(saved.current)
          setNotice('This draft changed on another device. It shows that version now.')
        } else if (error instanceof ApiProblem && error.status === 409 && error.code === 'draft_sending') {
          problem.current = 'sending'
          setNotice('This draft is being sent.')
        } else {
          problem.current = 'error'
          setNotice('The draft could not be saved. Check your connection.')
        }
      },
    )
    inflight.current = run.finally(() => {
      inflight.current = null
    })
    await inflight.current
  }
  const saveRef = useRef(saveNow)
  saveRef.current = saveNow

  useEffect(() => {
    if (fields === opened) return
    if (timer.current) clearTimeout(timer.current)
    timer.current = setTimeout(() => void saveRef.current(), SAVE_DELAY_MS)
  }, [fields, opened])
  // leaving another way than Close (Android back): what was typed is saved once
  useEffect(
    () => () => {
      if (!timer.current) return
      clearTimeout(timer.current)
      void saveRef.current()
    },
    [],
  )

  const edit = (next: Partial<ComposeFields>) => {
    setWarnings([])
    setNotice(null)
    setFields((current) => ({ ...current, ...next }))
  }
  /** The fields with the typed text of each recipient line as chips. */
  const committed = (): ComposeFields => {
    const next = { ...latest.current, to: commitTyped(latest.current.to, typed.to), cc: commitTyped(latest.current.cc, typed.cc), bcc: commitTyped(latest.current.bcc, typed.bcc) }
    if (typed.to || typed.cc || typed.bcc) {
      setTyped({ to: '', cc: '', bcc: '' })
      setFields(next)
      latest.current = next
    }
    return next
  }

  const close = async () => {
    const now = committed()
    if (isUntouched(draft, opened, now)) {
      // made just now and left as it was: it goes, as on the web
      await mail.removeDraft(client, workspaceId, draft.id).catch(() => {})
    } else {
      await saveNow()
    }
    void refreshSendViews(queryClient, workspaceId)
    router.back()
  }

  const from = addresses.find((address) => address.address_id === fields.fromId)
  const blocked = from && from.ready !== 'ready' ? notReadyText(from.not_ready_reason) : null

  const send = async () => {
    if (sending) return
    if (blocked) return setNotice(blocked)
    const now = committed()
    const recipients = [...now.to, ...now.cc, ...now.bcc]
    if (recipients.length === 0) return setNotice('Add a recipient.')
    if (recipients.some((address) => !isValidAddress(address))) return setNotice('Fix the address marked in red.')
    setSending(true)
    setNotice(null)
    // a conflict or a failed save showed its notice already: Send tries again
    if (problem.current !== 'sending') problem.current = null
    try {
      await saveNow()
      if (problem.current) return
      // the first Send shows the checks, the next one sends anyway (the server still skips suppressed recipients)
      if (warnings.length === 0) {
        const suppressed = await mail.draftSuppressed(client, workspaceId, draft.id).catch(() => [])
        const messages = thread.data?.messages
        const found = preSendWarnings({
          text: now.body,
          subject: now.subject,
          fileCount: draft.files.length,
          mode: draft.mode,
          to: now.to,
          cc: now.cc,
          bcc: now.bcc,
          threadAddresses: messages ? messages.flatMap((message) => [message.from, ...message.to, ...message.cc]).flatMap((person) => (person ? [person.address] : [])) : null,
          workspaceDomains: [...new Set([...addresses.map((address) => address.address), ...(messages ?? []).flatMap((message) => (message.from && !message.from_external ? [message.from.address] : []))].map((address) => address.split('@')[1]?.toLowerCase() ?? ''))].filter(Boolean),
          suppressed,
        })
        if (found.length > 0) return setWarnings(found)
      }
      const outbound = await mail.sendDraft(client, workspaceId, draft.id)
      queryClient.removeQueries({ queryKey: mailKeys.draft(workspaceId, draft.id) })
      void refreshSendViews(queryClient, workspaceId)
      if (outbound.status === 'held' && !outbound.scheduled) setPendingSend({ outboundId: outbound.id, releaseAt: new Date(outbound.release_at).getTime() })
      router.back()
    } catch (error) {
      setNotice(errorMessage(error) || 'The message could not be sent. Try again.')
    } finally {
      setSending(false)
    }
  }

  const title = fields.subject.trim() || (draft.mode === 'new' ? 'New message' : draft.mode.startsWith('forward') ? 'Forward' : 'Reply')
  const forwarded = draft.files.filter((file) => file.source_attachment_id)
  return (
    <SafeAreaView className="flex-1 bg-background">
      <KeyboardSpace className="flex-1">
        <PaneHeader className="px-2 gap-2">
          <IconButton icon="close" label="Close" disabled={sending} onPress={() => void close()} />
          <Text numberOfLines={1} className="flex-1 font-noto-semibold text-[13px]">
            {title}
          </Text>
          <Button label={warnings.length > 0 ? 'Send anyway' : 'Send'} icon="send" size="sm" loading={sending} onPress={() => void send()} />
        </PaneHeader>
        {notice || warnings.length > 0 ? (
          <View accessibilityRole="alert" accessibilityLiveRegion="polite" className="gap-1 border-b border-border px-3 py-2">
            {notice ? <Text className="text-[13px] text-destructive">{notice}</Text> : null}
            {warnings.map((warning) => (
              <View key={warning} className="flex-row gap-1.5">
                <Icon name="warning" size={14} color={colors.mutedForeground} />
                <Text className="flex-1 text-[13px] text-muted-foreground">{warning}</Text>
              </View>
            ))}
          </View>
        ) : null}
        <ScrollView keyboardShouldPersistTaps="handled" contentContainerClassName="grow">
          <View className="min-h-11 flex-row items-center gap-2 border-b border-border px-3">
            <Text className="text-[14px] text-muted-foreground">From</Text>
            <Button label={from ? `${from.display_name} <${from.address}>` : 'Choose an address'} variant="ghost" size="sm" onPress={() => setPicking(true)} />
          </View>
          {blocked ? <Text className="border-b border-border px-3 py-1.5 text-[12px] text-destructive">{blocked}</Text> : null}
          <RecipientField
            label="To"
            value={fields.to}
            onChange={(to) => edit({ to })}
            typed={typed.to}
            onTyped={(to) => setTyped((current) => ({ ...current, to }))}
            accessory={copies ? null : <Button label="Cc Bcc" variant="ghost" size="xs" muted onPress={() => setCopies(true)} />}
          />
          {copies ? (
            <>
              <RecipientField label="Cc" value={fields.cc} onChange={(cc) => edit({ cc })} typed={typed.cc} onTyped={(cc) => setTyped((current) => ({ ...current, cc }))} />
              <RecipientField label="Bcc" value={fields.bcc} onChange={(bcc) => edit({ bcc })} typed={typed.bcc} onTyped={(bcc) => setTyped((current) => ({ ...current, bcc }))} />
            </>
          ) : null}
          <TextInput
            value={fields.subject}
            onChangeText={(subject) => edit({ subject })}
            placeholder="Subject"
            placeholderTextColor={colors.mutedForeground}
            selectionColor={colors.primary}
            keyboardAppearance={scheme}
            accessibilityLabel="Subject"
            className="min-h-11 border-b border-border px-3 font-noto text-foreground"
          />
          <TextInput
            value={fields.body}
            onChangeText={(body) => edit({ body })}
            placeholder="Write your message"
            placeholderTextColor={colors.mutedForeground}
            selectionColor={colors.primary}
            keyboardAppearance={scheme}
            accessibilityLabel="Message"
            multiline
            scrollEnabled={false}
            textAlignVertical="top"
            autoFocus={draft.mode !== 'new'}
            className="min-h-48 flex-1 px-3 py-3 font-noto leading-[24px] text-foreground"
          />
          {draft.mode !== 'new' ? (
            <Text className="px-3 pb-4 text-[12px] text-muted-foreground">
              {draft.mode.startsWith('forward') ? 'The forwarded message goes below your text.' : 'The message you answer is quoted below your text.'}
              {forwarded.length > 0 ? ` Its files go with it: ${forwarded.map((file) => file.filename).join(', ')}.` : ''}
            </Text>
          ) : null}
        </ScrollView>
      </KeyboardSpace>
      <PickerSheet
        visible={picking}
        title="Send from"
        options={addresses.map((address) => ({
          key: address.address_id,
          label: `${address.display_name} <${address.address}>`,
          subtitle: address.ready === 'ready' ? undefined : notReadyText(address.not_ready_reason),
          selected: address.address_id === fields.fromId,
        }))}
        onSelect={(key) => {
          setPicking(false)
          const next = addresses.find((address) => address.address_id === key)
          edit({ fromId: key, body: swapSignature(latest.current.body, from, next) })
        }}
        onClose={() => setPicking(false)}
      />
    </SafeAreaView>
  )
}
