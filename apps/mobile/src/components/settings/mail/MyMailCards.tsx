/** The cards of the web's `MyMailPage.tsx` (Settings → Mail → Mail preferences): signatures, undo send, snippets and the vacation reply. */
import { useRef, useState } from 'react'
import { View } from 'react-native'

import { htmlToText } from '../../../data/mail-compose'
import type { MailFromAddress } from '../../../data/mail-api'
import type { MailSnippet } from '../../../data/mail-sending-api'
import { mailProblem, UNDO_SECONDS } from '../../../data/mail-settings'
import { SWIPE_ACTIONS, SWIPE_LABELS, type SwipeSettings } from '../../../data/mail-swipe'
import { useFromAddresses, useMailMe } from '../../../hooks/mail'
import { setMailSwipes, useMailSwipes } from '../../../hooks/mail-swipe'
import { useMailSettingsMe, useSaveMailSettings, useSaveSignature, useSignatures, useSnippets, useSnippetWrites } from '../../../hooks/mail-sending'
import { Button, EmptyState, Input, Text } from '../../../ui'
import { ConfirmDialog, type ConfirmRequest, Dialog } from '../Dialog'
import { Note, OptionText, Select, SettingsCard, SettingsRow } from '../Shell'
import { MailBodyField, plainBodyHtml, VacationForm } from './VacationForm'

export function SignaturesCard() {
  const addresses = useFromAddresses()
  const list = addresses.data ?? []
  const signatures = useSignatures(list.map((address) => address.address_id))
  return (
    <SettingsCard title="Signatures" description="Added at the end of each new message you write from the address. Each address has its own." flush>
      {addresses.isPending || (list.length > 0 && signatures.isPending) ? (
        <SettingsRow first>
          <Note loading>Loading signatures…</Note>
        </SettingsRow>
      ) : null}
      {addresses.isError || signatures.isError ? (
        <SettingsRow first>
          <Note error>{mailProblem(addresses.error ?? signatures.error, 'The signatures could not be loaded.')}</Note>
        </SettingsRow>
      ) : null}
      {addresses.isSuccess && list.length === 0 ? (
        <SettingsRow first>
          <Note>You have no address to send from yet.</Note>
        </SettingsRow>
      ) : null}
      {signatures.data
        ? list.map((address, index) => (
            <SignatureRow key={address.address_id} first={index === 0} address={address} saved={signatures.data.find((signature) => signature.address_id === address.address_id)?.body_html ?? ''} />
          ))
        : null}
    </SettingsCard>
  )
}

function SignatureRow({ first, address, saved }: { first: boolean; address: MailFromAddress; saved: string }) {
  const save = useSaveSignature()
  const html = useRef(saved)
  const [changed, setChanged] = useState(false)
  const [notice, setNotice] = useState<{ text: string; error: boolean } | null>(null)
  const unchanged = plainBodyHtml(saved)

  function submit() {
    setNotice(null)
    save.mutate(
      { addressId: address.address_id, bodyHtml: html.current },
      {
        onSuccess: () => {
          setChanged(false)
          setNotice({ text: html.current.trim() ? 'Signature saved' : 'Signature removed', error: false })
        },
        onError: (error) => setNotice({ text: mailProblem(error, 'The signature could not be saved.'), error: true }),
      },
    )
  }

  return (
    <View className={`gap-2 border-border px-4 py-3 ${first ? '' : 'border-t'}`}>
      <Text className="font-noto-medium text-[13px] text-foreground">
        {address.display_name} <Text className="font-mono text-[12px] text-muted-foreground">&lt;{address.address}&gt;</Text>
      </Text>
      {/* a new field when the saved signature changes (after a save, or on another device) */}
      <MailBodyField
        key={saved}
        accessibilityLabel={`Signature for ${address.address}`}
        placeholder="Write your signature…"
        initialHtml={saved}
        onChange={(next) => {
          html.current = next
          setChanged(next !== unchanged)
        }}
      />
      {notice ? <Note error={notice.error}>{notice.text}</Note> : null}
      <View className="flex-row">
        <Button label={save.isPending ? 'Saving…' : 'Save signature'} size="sm" variant="outline" disabled={!changed || save.isPending} onPress={submit} />
      </View>
    </View>
  )
}

const UNDO_OPTIONS = UNDO_SECONDS.map((seconds) => ({ value: String(seconds), label: seconds === 0 ? 'Off' : `${seconds} seconds` }))

export function UndoSendCard() {
  const settings = useMailSettingsMe()
  const save = useSaveMailSettings()
  const [notice, setNotice] = useState<{ text: string; error: boolean } | null>(null)
  return (
    <SettingsCard title="Undo send" flush>
      <SettingsRow first>
        <OptionText title="Time to undo" help="How long Orbit waits before a message leaves, so you can take it back with Undo.">
          {notice ? <Note error={notice.error}>{notice.text}</Note> : null}
          {settings.isError ? <Note error>{mailProblem(settings.error, 'The setting could not be loaded.')}</Note> : null}
        </OptionText>
        <View className="w-36">
          <Select
            label="Time to undo"
            value={settings.data ? String(settings.data.undo_send_seconds) : ''}
            options={UNDO_OPTIONS}
            disabled={!settings.data || save.isPending}
            onChange={(value) => {
              setNotice(null)
              save.mutate(Number(value), {
                onSuccess: () => setNotice({ text: 'Undo send saved', error: false }),
                onError: (error) => setNotice({ text: mailProblem(error, 'The setting could not be saved.'), error: true }),
              })
            }}
          />
        </View>
      </SettingsRow>
    </SettingsCard>
  )
}

const SWIPE_OPTIONS = SWIPE_ACTIONS.map((value) => ({ value, label: SWIPE_LABELS[value] }))
const SWIPE_SIDES: readonly { side: keyof SwipeSettings; title: string }[] = [
  { side: 'left', title: 'Swipe left' },
  { side: 'right', title: 'Swipe right' },
]

/** The mail list's swipe actions: kept on this device, as only a touch screen swipes. */
export function SwipeActionsCard() {
  const swipes = useMailSwipes()
  return (
    <SettingsCard title="Swipe actions" description="What a swipe to each side does to a conversation in the mail list. Saved on this device." flush>
      {SWIPE_SIDES.map(({ side, title }, index) => (
        <SettingsRow key={side} first={index === 0}>
          <OptionText title={title} />
          <View className="w-44">
            <Select label={title} value={swipes[side]} options={SWIPE_OPTIONS} onChange={(value) => setMailSwipes({ ...swipes, [side]: value })} />
          </View>
        </SettingsRow>
      ))}
    </SettingsCard>
  )
}

export function SnippetsCard() {
  const snippets = useSnippets()
  const { remove } = useSnippetWrites()
  const [editing, setEditing] = useState<MailSnippet | 'new' | null>(null)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const [problem, setProblem] = useState<string | null>(null)

  const removeSnippet = (snippet: MailSnippet) =>
    setConfirm({
      title: `Delete ${snippet.name}?`,
      description: 'Messages that use it keep their text.',
      confirmLabel: 'Delete',
      danger: true,
      onConfirm: () => {
        setProblem(null)
        remove.mutate(snippet.id, { onError: (error) => setProblem(mailProblem(error, 'The snippet could not be deleted.')) })
      },
    })

  return (
    <>
      <SettingsCard title="Snippets" description="Text you use often. Type ; in a message to insert one." actions={<Button label="Add snippet" icon="plus" onPress={() => setEditing('new')} />} flush>
        {snippets.isPending ? (
          <SettingsRow first>
            <Note loading>Loading snippets…</Note>
          </SettingsRow>
        ) : null}
        {snippets.isError ? (
          <SettingsRow first>
            <Note error>{mailProblem(snippets.error, 'The snippets could not be loaded.')}</Note>
          </SettingsRow>
        ) : null}
        {snippets.data?.length === 0 ? <EmptyState icon="noteText" title="No snippets yet" /> : null}
        {snippets.data?.map((snippet, index) => (
          <SettingsRow key={snippet.id} first={index === 0}>
            <View className="min-w-0 flex-1 gap-0.5">
              <Text className="font-noto-medium text-[13px] text-foreground">{snippet.name}</Text>
              <Text numberOfLines={1} className="text-[12px] text-muted-foreground">
                {htmlToText(snippet.body_html) || 'Empty'}
              </Text>
            </View>
            <Button label="Change" size="sm" variant="ghost" onPress={() => setEditing(snippet)} />
            <Button label="Delete" size="sm" variant="destructive" disabled={remove.isPending} onPress={() => removeSnippet(snippet)} />
          </SettingsRow>
        ))}
        {problem ? (
          <SettingsRow>
            <Note error>{problem}</Note>
          </SettingsRow>
        ) : null}
      </SettingsCard>
      {editing ? <SnippetDialog snippet={editing === 'new' ? undefined : editing} onClose={() => setEditing(null)} /> : null}
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
    </>
  )
}

function SnippetDialog({ snippet, onClose }: { snippet?: MailSnippet; onClose: () => void }) {
  const { create, update } = useSnippetWrites()
  const [name, setName] = useState(snippet?.name ?? '')
  const body = useRef(snippet?.body_html ?? '')
  const saving = create.isPending || update.isPending
  const error = create.error ?? update.error

  function submit() {
    const trimmed = name.trim()
    if (!trimmed || saving) return
    const payload = { name: trimmed, body_html: body.current }
    if (snippet) update.mutate({ snippetId: snippet.id, body: payload }, { onSuccess: onClose })
    else create.mutate(payload, { onSuccess: onClose })
  }

  return (
    <Dialog
      visible
      title={snippet ? `Change ${snippet.name}` : 'Add a snippet'}
      onClose={() => (saving ? undefined : onClose())}
      footer={
        <>
          <Button label="Cancel" variant="outline" disabled={saving} onPress={onClose} />
          <Button label={saving ? 'Saving…' : snippet ? 'Save' : 'Add snippet'} disabled={!name.trim() || saving} onPress={submit} />
        </>
      }
    >
      <View className="gap-4">
        <Input label="Name" maxLength={80} autoComplete="off" value={name} onChangeText={setName} />
        <MailBodyField
          label="Text"
          accessibilityLabel="Snippet text"
          placeholder="Write the snippet…"
          initialHtml={snippet?.body_html ?? ''}
          onChange={(html) => {
            body.current = html
          }}
        />
        {error ? <Note error>{mailProblem(error, 'The snippet was not saved.')}</Note> : null}
      </View>
    </Dialog>
  )
}

export function VacationCard() {
  const me = useMailMe()
  const personal = me.data?.mailboxes.find((mailbox) => mailbox.kind === 'personal')
  return (
    <SettingsCard title="Vacation reply" description="An automatic answer while you are away, for your personal mailbox. A shared mailbox’s reply is set in Mailboxes.">
      {me.isPending ? <Note loading>Loading…</Note> : null}
      {me.isError ? <Note error>{mailProblem(me.error, 'Your mailboxes could not be loaded.')}</Note> : null}
      {me.isSuccess && !personal ? <Note>You have no personal mailbox.</Note> : null}
      {personal ? <VacationForm mailboxId={personal.id} mailboxName={personal.primary_address ?? personal.name} /> : null}
    </SettingsCard>
  )
}
