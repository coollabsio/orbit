import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { SettingsCard } from '@/components/common/SettingsCard'
import { UnsavedBar } from '@/components/common/UnsavedBar'
import { useDeleteGithubProjectConnection, useGithubProjectSettings, useSaveGithubProjectConnection } from '@/features/tasks/api/github'

export type PendingProjectSave = { reset: () => void; save: () => void; saving: boolean }

export function ProjectGithubCard({ workspaceId, projectId, onPendingChange }: {
  workspaceId: string
  projectId: string
  onPendingChange?: (pending: PendingProjectSave | null) => void
}) {
  const query = useGithubProjectSettings(workspaceId, projectId)
  const save = useSaveGithubProjectConnection(workspaceId, projectId)
  const remove = useDeleteGithubProjectConnection(workspaceId, projectId)

  if (query.isPending) return <SettingsCard title="GitHub">Loading GitHub settings.</SettingsCard>
  if (query.isError) return <SettingsCard title="GitHub">GitHub settings are not available.</SettingsCard>

  return <GithubCardContent
    key={`${projectId}:${query.data.repository}:${query.data.label}`}
    settings={query.data}
    workspaceId={workspaceId}
    save={save}
    remove={remove}
    onPendingChange={onPendingChange}
  />
}

type Settings = NonNullable<ReturnType<typeof useGithubProjectSettings>['data']>
type Save = ReturnType<typeof useSaveGithubProjectConnection>
type Remove = ReturnType<typeof useDeleteGithubProjectConnection>
const NO_REPOSITORY = 'none'

function GithubCardContent({ settings, workspaceId, save, remove, onPendingChange }: {
  settings: Settings
  workspaceId: string
  save: Save
  remove: Remove
  onPendingChange?: (pending: PendingProjectSave | null) => void
}) {
  const current = settings.repositories.find((item) => item.repository === settings.repository)
  const savedSelection = current ? `${current.installation_id}:${current.repository}` : NO_REPOSITORY
  const savedLabel = settings.label ?? 'Orbit'
  const [selection, setSelection] = useState(savedSelection)
  const [label, setLabel] = useState(savedLabel)
  const selected = settings.repositories.find((item) => `${item.installation_id}:${item.repository}` === selection)
  const disconnecting = selection === NO_REPOSITORY && !!settings.repository
  const dirty = selection !== savedSelection || label !== savedLabel
  const repositoryOptions = [
    { value: NO_REPOSITORY, label: 'No repository' },
    ...settings.repositories.map((item) => ({ value: `${item.installation_id}:${item.repository}`, label: item.repository })),
  ]
  const saveConnection = save.mutate
  const removeConnection = remove.mutate
  const saving = save.isPending || remove.isPending || (!disconnecting && (!selected || !label.trim()))
  const reset = useCallback(() => { setSelection(savedSelection); setLabel(savedLabel) }, [savedSelection, savedLabel])
  const saveChanges = useCallback(() => {
    if (disconnecting) removeConnection()
    else if (selected && label.trim()) saveConnection({ installation_id: selected.installation_id, repository: selected.repository, label: label.trim() })
  }, [disconnecting, removeConnection, selected, label, saveConnection])

  useEffect(() => {
    onPendingChange?.(dirty ? { reset, save: saveChanges, saving } : null)
    return () => onPendingChange?.(null)
  }, [dirty, saving, reset, saveChanges, onPendingChange])

  return <><SettingsCard title="GitHub" description="Connect this project to a GitHub repository and issue label.">
    <div className="flex flex-col gap-4">
      {!settings.app_slug ? (
        <p className="text-sm text-muted-foreground">Set up the GitHub App in <Link className="text-primary underline" to={`/settings/github?workspace=${workspaceId}`}>workspace settings</Link> first.</p>
      ) : (
        <>
          {settings.repositories.length === 0 ? <p className="text-sm text-muted-foreground">Install the App on a repository in <Link className="text-primary underline" to={`/settings/github?workspace=${workspaceId}`}>workspace settings</Link>, then refresh this page.</p> : null}
          {settings.can_manage ? <>
            <div className="grid gap-1.5">
              <Label htmlFor="github-repository">Repository</Label>
              <Select items={repositoryOptions} value={selection} onValueChange={(value) => setSelection(value ?? NO_REPOSITORY)}>
                <SelectTrigger id="github-repository" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {repositoryOptions.map((item) => <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>)}
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="github-label">Issue label</Label>
              <Input id="github-label" maxLength={50} value={label} onChange={(event) => setLabel(event.target.value)} />
              <p className="text-xs text-muted-foreground">Add this label to a GitHub issue to create a task in this project. Create the label in GitHub first.</p>
            </div>
            {save.isError || remove.isError ? <p role="alert" className="text-xs text-destructive">{save.error instanceof Error ? save.error.message : remove.error instanceof Error ? remove.error.message : 'The GitHub connection could not be saved.'}</p> : null}
          </> : null}
          {!settings.can_manage && settings.repository ? <p className="text-sm text-muted-foreground">Connected to {settings.repository} with the {settings.label} label.</p> : null}
        </>
      )}
    </div>
  </SettingsCard>
    {!onPendingChange && settings.can_manage && dirty ? <UnsavedBar onReset={reset} onSave={saveChanges} saving={saving} /> : null}
  </>
}
