import { useState } from 'react'
import { View } from 'react-native'

import { AdminShell, Note, SettingsCard } from '../../../../components/settings/Shell'
import { limitsBody, limitsDraft, type LimitsView } from '../../../../data/admin-api'
import { useInstanceSettings, useSaveLimits } from '../../../../hooks/admin'
import { errorMessage } from '../../../../session-context'
import { Button, Input } from '../../../../ui'

/** The web's Admin → Limits: what one account and one workspace may use. */
export default function AdminLimitsScreen() {
  const settings = useInstanceSettings()
  return (
    <AdminShell refreshing={settings.isRefetching} onRefresh={() => void settings.refetch()}>
      {settings.isPending ? <SettingsCard title="Limits"><Note>Loading settings…</Note></SettingsCard> : null}
      {settings.isError ? (
        <SettingsCard title="Limits">
          <View className="flex-row items-center gap-2">
            <Note error>Settings could not be loaded.</Note>
            <Button label="Retry" variant="ghost" onPress={() => void settings.refetch()} />
          </View>
        </SettingsCard>
      ) : null}
      {settings.data ? <LimitsCard limits={settings.data.limits} /> : null}
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
          keyboardType="number-pad"
          placeholder="No limit"
          value={draft.workspaces}
          onChangeText={(workspaces) => edit({ workspaces })}
          description="Workspaces one account may own, not counting the trash. Admins have no limit."
        />
        <Input
          label="Storage per workspace (GB)"
          keyboardType="decimal-pad"
          placeholder="No limit"
          value={draft.storageGb}
          onChangeText={(storageGb) => edit({ storageGb })}
          description="Files, emoji and stickers of one workspace, each file counted once."
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
