import { useState } from 'react'
import { toast } from 'sonner'
import type { JobLimitView } from '@/api/generated/types.gen'
import { ApiProblem } from '@/api/problem'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button } from '@/components/ui/button'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { useJobLimits, useSaveJobLimits } from '@/features/admin/api'
import { SettingsRow } from '@/features/settings/components/SettingsParts'

const KIND_LABELS: Record<string, string> = {
  'notion.scan': 'Notion scans',
  'notion.import': 'Notion imports',
}

type Draft = Record<string, { concurrency: string; workspace_concurrency: string }>

const text = (limit: number | null | undefined) => (limit == null ? '' : String(limit))
const limit = (value: string) => (value.trim() === '' ? null : Number(value))
const shown = (limit: number | null | undefined) => (limit == null ? 'no limit' : String(limit))

function draftOf(kinds: JobLimitView[], useDefaults = false): Draft {
  return Object.fromEntries(kinds.map((kind) => [kind.kind, {
    concurrency: text(useDefaults ? kind.default_concurrency : kind.concurrency),
    workspace_concurrency: text(useDefaults ? kind.default_workspace_concurrency : kind.workspace_concurrency),
  }]))
}

/** How many background jobs of each kind may run at once, on all servers and per workspace. */
export function AdminJobsPage() {
  const limits = useJobLimits()
  if (limits.isPending) return <SettingsCard title="Background jobs"><p role="status">Loading limits…</p></SettingsCard>
  if (limits.isError) return <SettingsCard title="Background jobs"><p role="alert">Limits could not be loaded. <Button variant="ghost" onClick={() => void limits.refetch()}>Retry</Button></p></SettingsCard>
  // Remount on save so the form shows what the server stored.
  return <JobLimitsForm key={JSON.stringify(limits.data.kinds)} kinds={limits.data.kinds} />
}

function JobLimitsForm({ kinds }: { kinds: JobLimitView[] }) {
  const save = useSaveJobLimits()
  const [draft, setDraft] = useState(() => draftOf(kinds))
  const set = (kind: string, field: 'concurrency' | 'workspace_concurrency') => (event: React.ChangeEvent<HTMLInputElement>) =>
    setDraft((current) => ({ ...current, [kind]: { ...current[kind], [field]: event.target.value } }))

  function submit(event: React.FormEvent) {
    event.preventDefault()
    save.mutate({
      kinds: kinds.map((kind) => ({
        kind: kind.kind,
        concurrency: limit(draft[kind.kind].concurrency),
        workspace_concurrency: limit(draft[kind.kind].workspace_concurrency),
      })),
    }, {
      onSuccess: () => toast('Job limits saved'),
      onError: (error) => toast.error(error instanceof ApiProblem ? error.detail : 'Job limits could not be saved.'),
    })
  }

  return (
    <form onSubmit={submit}>
      <SettingsCard
        title="Background jobs"
        description="How many jobs of each kind run at once. Every server uses a new limit for the next job it starts. Per workspace applies only while another workspace has a job waiting. Leave a field empty for no limit."
        actions={(
          <>
            <Button type="button" variant="ghost" onClick={() => setDraft(draftOf(kinds, true))}>Reset to defaults</Button>
            <Button type="submit" disabled={save.isPending}>{save.isPending ? 'Saving…' : 'Save limits'}</Button>
          </>
        )}
        flush
      >
        <div className="divide-y">
          {kinds.map((kind) => {
            const label = KIND_LABELS[kind.kind] ?? kind.kind
            return (
              <SettingsRow key={kind.kind}>
                <div className="flex min-w-0 flex-1 flex-col gap-0.5 text-[13px]">
                  <span className="font-medium text-foreground">{label}</span>
                  <span className="text-xs text-muted-foreground">
                    Default: {shown(kind.default_concurrency)} on all servers, {shown(kind.default_workspace_concurrency)} per workspace
                  </span>
                </div>
                <Field className="w-32">
                  <FieldLabel htmlFor={`${kind.kind}-concurrency`}>All servers</FieldLabel>
                  <Input id={`${kind.kind}-concurrency`} aria-label={`${label} on all servers`} type="number" min={1} max={100} placeholder="No limit" value={draft[kind.kind].concurrency} onChange={set(kind.kind, 'concurrency')} />
                </Field>
                <Field className="w-32">
                  <FieldLabel htmlFor={`${kind.kind}-workspace`}>Per workspace</FieldLabel>
                  <Input id={`${kind.kind}-workspace`} aria-label={`${label} per workspace`} type="number" min={1} max={100} placeholder="No limit" value={draft[kind.kind].workspace_concurrency} onChange={set(kind.kind, 'workspace_concurrency')} />
                </Field>
              </SettingsRow>
            )
          })}
        </div>
      </SettingsCard>
    </form>
  )
}
