import { useState } from 'react'
import { View } from 'react-native'

import { MailSendLimits } from '../../../../components/settings/MailSendLimits'
import { AdminShell, Note, SetByEnvironment, SettingsCard } from '../../../../components/settings/Shell'
import { type EnvironmentView, imageFetchesBody, limitsBody, limitsDraft, type LimitsView } from '../../../../data/admin-api'
import { useInstanceSettings, useSaveLimits } from '../../../../hooks/admin'
import { useMailEnabled } from '../../../../hooks/mail'
import { errorMessage } from '../../../../session-context'
import { Button, Input } from '../../../../ui'

/** The web's Admin → Limits: Workspaces (what one account and one workspace may use), then Email while Mail is on. */
export default function AdminLimitsScreen() {
  const settings = useInstanceSettings()
  const mail = useMailEnabled()
  return (
    <AdminShell onRefresh={() => settings.refetch()}>
      {settings.isPending ? <SettingsCard title="Limits"><Note loading>Loading settings…</Note></SettingsCard> : null}
      {settings.isError ? (
        <SettingsCard title="Limits">
          <View className="flex-row items-center gap-2">
            <Note error>Settings could not be loaded.</Note>
            <Button label="Retry" variant="ghost" onPress={() => void settings.refetch()} />
          </View>
        </SettingsCard>
      ) : null}
      {settings.data ? <LimitsCard limits={settings.data.limits} /> : null}
      {settings.data ? <ImageFetchesCard key={settings.data.limits.image_proxy_fetches} limits={settings.data.limits} environment={settings.data.environment} /> : null}
      {mail ? <MailSendLimits /> : null}
    </AdminShell>
  )
}

function LimitsCard({ limits }: { limits: LimitsView }) {
  const save = useSaveLimits()
  const [draft, setDraft] = useState(() => limitsDraft(limits))
  const [invalid, setInvalid] = useState(false)
  const edit = (change: Partial<typeof draft>) => {
    save.reset()
    setDraft((current) => ({ ...current, ...change }))
  }

  const submit = () => {
    const body = limitsBody(draft)
    setInvalid(body === null)
    if (body) save.mutate(body, { onSuccess: (settings) => setDraft(limitsDraft(settings.limits)) })
  }

  return (
    <SettingsCard title="Limits" description="What one account and one workspace may use. Empty or 0 means no limit. A lower limit removes nothing that exists.">
      <View className="gap-4">
        <Input
          label="Workspaces per account"
          help="Workspaces one account may own, not counting the trash. Admins have no limit."
          keyboardType="number-pad"
          placeholder="No limit"
          value={draft.workspaces}
          onChangeText={(workspaces) => edit({ workspaces })}
        />
        <Input
          label="Storage per workspace (GB)"
          help="Files, emoji and stickers of one workspace, each file counted once."
          keyboardType="decimal-pad"
          placeholder="No limit"
          value={draft.storageGb}
          onChangeText={(storageGb) => edit({ storageGb })}
        />
        {invalid ? <Note error>Enter a number, or leave the field empty for no limit.</Note> : null}
        {save.isError ? <Note error>{errorMessage(save.error)}</Note> : null}
        {save.isSuccess ? <Note>Limits saved.</Note> : null}
        <View className="flex-row justify-end">
          <Button label={save.isPending ? 'Saving…' : 'Save limits'} disabled={save.isPending} onPress={submit} />
        </View>
      </View>
    </SettingsCard>
  )
}

/** The web's "Outside images" card: images that one server loads at once through Orbit. */
function ImageFetchesCard({ limits, environment }: { limits: LimitsView; environment: EnvironmentView }) {
  const save = useSaveLimits()
  const [fetches, setFetches] = useState(String(limits.image_proxy_fetches))
  const [invalid, setInvalid] = useState(false)
  const locked = environment.image_proxy_fetches

  const submit = () => {
    const body = imageFetchesBody(limits, fetches)
    setInvalid(body === null)
    if (body) save.mutate(body)
  }

  return (
    <SettingsCard title="Outside images" description="Images from other servers (in mail, messages, pages and link cards) load through Orbit, so those servers do not learn who reads them.">
      <View className="gap-4">
        <Input
          label="Images loaded at once per server"
          help="More images wait for a free turn, up to 10 seconds. One user has at most 6 at once. Each image can use up to 5 MB of memory while it loads. Every server applies a change within a minute."
          keyboardType="number-pad"
          value={fetches}
          editable={!locked && !save.isPending}
          onChangeText={(value) => {
            save.reset()
            setFetches(value)
          }}
        />
        {locked ? <SetByEnvironment variable="ORBIT_IMAGE_PROXY_FETCHES" /> : null}
        {invalid ? <Note error>Enter a whole number from 1 to 256.</Note> : null}
        {save.isError ? <Note error>{errorMessage(save.error)}</Note> : null}
        {save.isSuccess ? <Note>Image limit saved.</Note> : null}
        {locked ? null : (
          <View className="flex-row justify-end">
            <Button label={save.isPending ? 'Saving…' : 'Save image limit'} disabled={save.isPending} onPress={submit} />
          </View>
        )}
      </View>
    </SettingsCard>
  )
}
