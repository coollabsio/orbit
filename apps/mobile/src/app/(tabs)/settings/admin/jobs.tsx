import { useState } from 'react'
import { View } from 'react-native'

import { AdminShell, Note, RowText, SettingsCard } from '../../../../components/settings/Shell'
import { jobDraft, jobLabel, type JobLimitView, jobLimitsBody, limitText } from '../../../../data/admin-api'
import { useJobLimits, useSaveJobLimits } from '../../../../hooks/admin'
import { errorMessage } from '../../../../session-context'
import { Button, Input } from '../../../../ui'

const DESCRIPTION =
  'How many jobs of each kind run at once. Every server uses a new limit for the next job it starts. Per workspace applies only while another workspace has a job waiting. Leave a field empty for no limit.'

/** The web's Admin → Background jobs: how many jobs of each kind may run at once, on all servers and per workspace. */
export default function AdminJobsScreen() {
  const limits = useJobLimits()
  return (
    <AdminShell onRefresh={() => limits.refetch()}>
      {limits.isPending ? <SettingsCard title="Background jobs"><Note loading>Loading limits…</Note></SettingsCard> : null}
      {limits.isError ? (
        <SettingsCard title="Background jobs">
          <View className="flex-row items-center gap-2">
            <Note error>Limits could not be loaded.</Note>
            <Button label="Retry" variant="ghost" onPress={() => void limits.refetch()} />
          </View>
        </SettingsCard>
      ) : null}
      {limits.data ? <JobLimitsForm kinds={limits.data} /> : null}
    </AdminShell>
  )
}

function JobLimitsForm({ kinds }: { kinds: JobLimitView[] }) {
  const save = useSaveJobLimits()
  const [draft, setDraft] = useState(() => jobDraft(kinds))
  const edit = (next: typeof draft) => {
    save.reset()
    setDraft(next)
  }
  const set = (kind: string, field: 'concurrency' | 'workspace_concurrency') => (value: string) => edit({ ...draft, [kind]: { ...draft[kind], [field]: value } })

  return (
    <SettingsCard
      title="Background jobs"
      description={DESCRIPTION}
      actions={
        <>
          <Button label="Reset to defaults" variant="ghost" onPress={() => edit(jobDraft(kinds, true))} />
          <Button
            label={save.isPending ? 'Saving…' : 'Save limits'}
            disabled={save.isPending}
            // the form then shows what the server stored
            onPress={() => save.mutate(jobLimitsBody(kinds, draft), { onSuccess: (stored) => setDraft(jobDraft(stored)) })}
          />
        </>
      }
      flush
    >
      {kinds.map((kind, index) => {
        const label = jobLabel(kind.kind)
        return (
          <View key={kind.kind} className={`gap-3 border-border px-4 py-3 ${index === 0 ? '' : 'border-t'}`}>
            <RowText title={label} subtitle={`Default: ${limitText(kind.default_concurrency)} on all servers, ${limitText(kind.default_workspace_concurrency)} per workspace`} />
            <View className="flex-row gap-3">
              <View className="flex-1">
                <Input
                  label="All servers"
                  accessibilityLabel={`${label} on all servers`}
                  keyboardType="number-pad"
                  placeholder="No limit"
                  value={draft[kind.kind]?.concurrency ?? ''}
                  onChangeText={set(kind.kind, 'concurrency')}
                />
              </View>
              <View className="flex-1">
                <Input
                  label="Per workspace"
                  accessibilityLabel={`${label} per workspace`}
                  keyboardType="number-pad"
                  placeholder="No limit"
                  value={draft[kind.kind]?.workspace_concurrency ?? ''}
                  onChangeText={set(kind.kind, 'workspace_concurrency')}
                />
              </View>
            </View>
          </View>
        )
      })}
      {save.isError || save.isSuccess ? (
        <View className="border-t border-border px-4 py-3">
          {save.isError ? <Note error>{errorMessage(save.error)}</Note> : <Note>Job limits saved.</Note>}
        </View>
      ) : null}
    </SettingsCard>
  )
}
