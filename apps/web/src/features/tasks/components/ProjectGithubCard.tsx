import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ChevronDown } from 'reicon-react'
import { selectTriggerClassName } from '@/components/ui/select'
import { SearchPicker } from '@/components/common/SearchPicker'
import { InfoTip } from '@/components/common/InfoTip'
import { SettingsCard } from '@/components/common/SettingsCard'
import { UnsavedBar } from '@/components/common/UnsavedBar'
import { useDeleteGithubProjectConnection, useGithubProjectSettings, useSaveGithubProjectConnection } from '@/features/tasks/api/github'
import { loadFailed } from '@/lib/connection'

export type PendingProjectSave = { reset: () => void; save: () => void; saving: boolean }

export function ProjectGithubCard({ workspaceId, projectId, onPendingChange }: {
  workspaceId: string
  projectId: string
  onPendingChange?: (pending: PendingProjectSave | null) => void
}) {
  const query = useGithubProjectSettings(workspaceId, projectId)
  const save = useSaveGithubProjectConnection(workspaceId, projectId)
  const remove = useDeleteGithubProjectConnection(workspaceId, projectId)

  if (loadFailed(query)) return <SettingsCard title="GitHub">GitHub settings are not available.</SettingsCard>
  if (query.data === undefined) return <SettingsCard title="GitHub">Loading GitHub settings.</SettingsCard>

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
              <SearchPicker
                label="Repository"
                placeholder="Search repositories…"
                emptyText="No repositories match"
                options={repositoryOptions}
                selected={[selection]}
                onSelect={setSelection}
                className="w-(--anchor-width) min-w-60"
                trigger={
                  <button type="button" id="github-repository" data-size="default" className={selectTriggerClassName}>
                    <span data-slot="select-value" className="flex flex-1 text-left">{repositoryOptions.find((item) => item.value === selection)?.label ?? 'No repository'}</span>
                    <ChevronDown className="size-4 text-muted-foreground" aria-hidden="true" />
                  </button>
                }
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="github-label">
                Issue label
                <InfoTip text="Add this label to a GitHub issue to create a task in this project. Create the label in GitHub first." />
              </Label>
              <Input id="github-label" maxLength={50} value={label} onChange={(event) => setLabel(event.target.value)} />
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
