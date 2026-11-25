import { useState } from 'react'
import { toast } from 'sonner'
import type { EnvironmentView, LimitsView } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { InfoTip } from '@/components/common/InfoTip'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { useInstanceSettings, useSaveLimits } from '@/features/admin/api'
import { SendPoliciesCard, TierDefaultsCard } from '@/features/admin/components/MailSendingAdmin'
import { SetByEnvironment } from '@/features/admin/components/SetByEnvironment'
import { useMailEnabled } from '@/features/mail/enabled'
import { FieldGrid } from '@/features/settings/components/SettingsParts'
import { LoadingText } from '@/components/ui/spinner'

function problemText(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
}

/** Limits → Workspaces: what one account and one workspace may use. */
export function AdminLimitsPage() {
  const settings = useInstanceSettings()
  if (settings.isPending) return <SettingsCard title="Limits"><p><LoadingText>Loading settings…</LoadingText></p></SettingsCard>
  if (settings.isError) return <SettingsCard title="Limits"><p role="alert">Settings could not be loaded. <Button variant="ghost" onClick={() => void settings.refetch()}>Retry</Button></p></SettingsCard>
  // Remount on save so the forms show what the server stored.
  const key = JSON.stringify(settings.data.limits)
  return (
    <>
      <LimitsCard key={key} limits={settings.data.limits} />
      <ImageFetchesCard key={`images-${key}`} limits={settings.data.limits} environment={settings.data.environment} />
    </>
  )
}

/** Limits → Email: the send limits of the tiers and of each workspace. */
export function AdminEmailLimitsPage() {
  if (!useMailEnabled()) return <SettingsCard title="Email"><p className="m-0 text-sm text-muted-foreground">Mail is off. Enable it in Admin → Mail to set send limits.</p></SettingsCard>
  return (
    <>
      <TierDefaultsCard />
      <SendPoliciesCard />
    </>
  )
}

const BYTES_PER_GB = 1_000_000_000

/** A whole number above 0, or `null` (unlimited) for an empty field or 0. `undefined` when it is not a number. */
function parseLimit(value: string, scale = 1): number | null | undefined {
  if (!value.trim()) return null
  const number = Number(value)
  if (!Number.isFinite(number) || number < 0) return undefined
  return number === 0 ? null : Math.round(number * scale)
}

const CHAT_CHARS_MIN = 100
const CHAT_CHARS_MAX = 40_000

/** A whole number of characters in the allowed range; `undefined` otherwise. Chat messages always have a limit. */
function parseChatLimit(value: string): number | undefined {
  const number = Number(value)
  return Number.isInteger(number) && number >= CHAT_CHARS_MIN && number <= CHAT_CHARS_MAX ? number : undefined
}

function LimitsCard({ limits }: { limits: LimitsView }) {
  const save = useSaveLimits()
  const [workspaces, setWorkspaces] = useState(limits.max_owned_workspaces ? String(limits.max_owned_workspaces) : '')
  const [storage, setStorage] = useState(limits.workspace_storage_quota_bytes ? String(limits.workspace_storage_quota_bytes / BYTES_PER_GB) : '')
  const [chatChars, setChatChars] = useState(String(limits.chat_message_max_chars))

  function submit(event: React.FormEvent) {
    event.preventDefault()
    const maxOwnedWorkspaces = parseLimit(workspaces)
    const storageQuota = parseLimit(storage, BYTES_PER_GB)
    if (maxOwnedWorkspaces === undefined || storageQuota === undefined) {
      toast.error('Enter a number, or leave the field empty for no limit.')
      return
    }
    const chatMessageMaxChars = parseChatLimit(chatChars)
    if (chatMessageMaxChars === undefined) {
      toast.error(`Enter a chat message length from ${CHAT_CHARS_MIN} to ${CHAT_CHARS_MAX}.`)
      return
    }
    save.mutate({ max_owned_workspaces: maxOwnedWorkspaces, workspace_storage_quota_bytes: storageQuota, chat_message_max_chars: chatMessageMaxChars }, {
      onSuccess: () => toast('Limits saved'),
      onError: (error) => toast.error(problemText(error, 'The limits could not be saved.')),
    })
  }

  return (
    <form onSubmit={submit}>
      <SettingsCard title="Limits" description="What one account and one workspace may use. Empty or 0 means no limit, except for chat messages. A lower limit removes or cuts nothing that exists.">
        <FieldGrid>
          <Field>
            <FieldLabel htmlFor="limit-workspaces">Workspaces per account <InfoTip text="Workspaces one account may own, not counting the trash. Admins have no limit." /></FieldLabel>
            <Input id="limit-workspaces" type="number" min={0} step={1} placeholder="No limit" value={workspaces} onChange={(event) => setWorkspaces(event.target.value)} />
          </Field>
          <Field>
            <FieldLabel htmlFor="limit-storage">Storage per workspace (GB) <InfoTip text="Files, emoji and stickers of one workspace, each file counted once." /></FieldLabel>
            <Input id="limit-storage" type="number" min={0} step="any" placeholder="No limit" value={storage} onChange={(event) => setStorage(event.target.value)} />
          </Field>
          <Field>
            <FieldLabel htmlFor="limit-chat-chars">Chat message length (characters) <InfoTip text={`The most characters a new or edited chat message can have: ${CHAT_CHARS_MIN}–${CHAT_CHARS_MAX}. Existing messages keep their text. A longer message must be shortened to be edited.`} /></FieldLabel>
            <Input id="limit-chat-chars" type="number" required min={CHAT_CHARS_MIN} max={CHAT_CHARS_MAX} step={1} value={chatChars} onChange={(event) => setChatChars(event.target.value)} />
          </Field>
        </FieldGrid>
        <div className="mt-4 flex justify-end">
          <Button type="submit" disabled={save.isPending}>{save.isPending ? 'Saving…' : 'Save limits'}</Button>
        </div>
      </SettingsCard>
    </form>
  )
}

/** Outside images that one server loads at once through Orbit (mail, messages, pages, link cards). */
function ImageFetchesCard({ limits, environment }: { limits: LimitsView; environment: EnvironmentView }) {
  const save = useSaveLimits()
  const [fetches, setFetches] = useState(String(limits.image_proxy_fetches))

  function submit(event: React.FormEvent) {
    event.preventDefault()
    const value = Number(fetches)
    if (!Number.isInteger(value) || value < 1 || value > 256) {
      toast.error('Enter a whole number from 1 to 256.')
      return
    }
    // The other limits go along unchanged: a save sets all of them.
    save.mutate({ max_owned_workspaces: limits.max_owned_workspaces, workspace_storage_quota_bytes: limits.workspace_storage_quota_bytes, image_proxy_fetches: value }, {
      onSuccess: () => toast('Image limit saved'),
      onError: (error) => toast.error(problemText(error, 'The image limit could not be saved.')),
    })
  }

  return (
    <form onSubmit={submit}>
      <SettingsCard title="Outside images" description="Images from other servers (in mail, messages, pages and link cards) load through Orbit, so those servers do not learn who reads them.">
        <FieldGrid>
          <Field>
            <FieldLabel htmlFor="limit-image-fetches">Images loaded at once per server <InfoTip text="More images wait for a free turn, up to 10 seconds. One user has at most 6 at once. Each image can use up to 5 MB of memory while it loads. Every server applies a change within a minute." /></FieldLabel>
            <Input id="limit-image-fetches" type="number" min={1} max={256} step={1} value={fetches} disabled={environment.image_proxy_fetches || save.isPending} onChange={(event) => setFetches(event.target.value)} />
            {environment.image_proxy_fetches ? <SetByEnvironment variable="ORBIT_IMAGE_PROXY_FETCHES" /> : null}
          </Field>
        </FieldGrid>
        {environment.image_proxy_fetches ? null : (
          <div className="mt-4 flex justify-end">
            <Button type="submit" disabled={save.isPending}>{save.isPending ? 'Saving…' : 'Save image limit'}</Button>
          </div>
        )}
      </SettingsCard>
    </form>
  )
}
