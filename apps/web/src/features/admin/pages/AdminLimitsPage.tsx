import { useState } from 'react'
import { toast } from 'sonner'
import type { LimitsView } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { useInstanceSettings, useSaveLimits } from '@/features/admin/api'
import { FieldGrid } from '@/features/settings/components/SettingsParts'

function problemText(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
}

/** What one account and one workspace may use. */
export function AdminLimitsPage() {
  const settings = useInstanceSettings()
  if (settings.isPending) return <SettingsCard title="Limits"><p role="status">Loading settings…</p></SettingsCard>
  if (settings.isError) return <SettingsCard title="Limits"><p role="alert">Settings could not be loaded. <Button variant="ghost" onClick={() => void settings.refetch()}>Retry</Button></p></SettingsCard>
  // Remount on save so the form shows what the server stored.
  return <LimitsCard key={JSON.stringify(settings.data.limits)} limits={settings.data.limits} />
}

const BYTES_PER_GB = 1_000_000_000

/** A whole number above 0, or `null` (unlimited) for an empty field or 0. `undefined` when it is not a number. */
function parseLimit(value: string, scale = 1): number | null | undefined {
  if (!value.trim()) return null
  const number = Number(value)
  if (!Number.isFinite(number) || number < 0) return undefined
  return number === 0 ? null : Math.round(number * scale)
}

function LimitsCard({ limits }: { limits: LimitsView }) {
  const save = useSaveLimits()
  const [workspaces, setWorkspaces] = useState(limits.max_owned_workspaces ? String(limits.max_owned_workspaces) : '')
  const [storage, setStorage] = useState(limits.workspace_storage_quota_bytes ? String(limits.workspace_storage_quota_bytes / BYTES_PER_GB) : '')

  function submit(event: React.FormEvent) {
    event.preventDefault()
    const maxOwnedWorkspaces = parseLimit(workspaces)
    const storageQuota = parseLimit(storage, BYTES_PER_GB)
    if (maxOwnedWorkspaces === undefined || storageQuota === undefined) {
      toast.error('Enter a number, or leave the field empty for no limit.')
      return
    }
    save.mutate({ max_owned_workspaces: maxOwnedWorkspaces, workspace_storage_quota_bytes: storageQuota }, {
      onSuccess: () => toast('Limits saved'),
      onError: (error) => toast.error(problemText(error, 'The limits could not be saved.')),
    })
  }

  return (
    <form onSubmit={submit}>
      <SettingsCard title="Limits" description="What one account and one workspace may use. Empty or 0 means no limit. A lower limit removes nothing that exists.">
        <FieldGrid>
          <Field>
            <FieldLabel htmlFor="limit-workspaces">Workspaces per account</FieldLabel>
            <Input id="limit-workspaces" type="number" min={0} step={1} placeholder="No limit" value={workspaces} onChange={(event) => setWorkspaces(event.target.value)} />
            <FieldDescription>Workspaces one account may own, not counting the trash. Admins have no limit.</FieldDescription>
          </Field>
          <Field>
            <FieldLabel htmlFor="limit-storage">Storage per workspace (GB)</FieldLabel>
            <Input id="limit-storage" type="number" min={0} step="any" placeholder="No limit" value={storage} onChange={(event) => setStorage(event.target.value)} />
            <FieldDescription>Files, emoji and stickers of one workspace, each file counted once.</FieldDescription>
          </Field>
        </FieldGrid>
        <div className="mt-4 flex justify-end">
          <Button type="submit" disabled={save.isPending}>{save.isPending ? 'Saving…' : 'Save limits'}</Button>
        </div>
      </SettingsCard>
    </form>
  )
}
