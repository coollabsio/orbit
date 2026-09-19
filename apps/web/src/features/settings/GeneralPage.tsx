import { useRef, useState } from 'react'
import { useMutation } from '@tanstack/react-query'
import { apiClient } from '../../api/client'
import { createBackup } from '../../api/generated/sdk.gen'
import { Listbox } from '../../components/ui/Listbox'
import { UnsavedBar } from '../../components/ui/UnsavedBar'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useTheme, type Theme } from '../../lib/themeContext'
import { useRenameWorkspace } from '../workspaces/api'
import { useWorkspace } from '../workspaces/workspaceContext'
import { SettingsCard } from './SettingsCard'

const FIELD_LABEL = 'mb-1.5 h-4 gap-1 text-[13px] leading-4 font-medium text-muted-foreground'
const GRID = 'grid grid-cols-1 gap-4 min-[900px]:grid-cols-2'
const FIELD = 'w-full min-w-0'

export function GeneralPage() {
  const { theme, setTheme } = useTheme()
  const { workspace } = useWorkspace()
  const renameWorkspace = useRenameWorkspace(workspace.id)
  const backup = useMutation({
    mutationFn: async () => {
      const { data } = await createBackup({ client: apiClient, throwOnError: true })
      if (!data) throw new Error('Backup response was empty.')
      return data
    },
  })
  const formRef = useRef<HTMLFormElement>(null)
  const [nameDraft, setNameDraft] = useState<{ workspaceId: string; value: string } | null>(null)
  const name = nameDraft?.workspaceId === workspace.id ? nameDraft.value : workspace.name
  const dirty = name.trim() !== workspace.name

  function saveWorkspace() {
    if (!name.trim() || !dirty || renameWorkspace.isPending) return
    renameWorkspace.mutate(
      { name: name.trim(), version: workspace.version },
      { onSuccess: () => setNameDraft(null) },
    )
  }

  return (
    <>
      <SettingsCard title="Workspace" description="Rename this workspace.">
        <form ref={formRef} className={GRID} onSubmit={(event) => { event.preventDefault(); saveWorkspace() }}>
          <div className={FIELD}><Label className={FIELD_LABEL} htmlFor="workspace-name">Name</Label><Input id="workspace-name" required disabled={renameWorkspace.isPending} value={name} onChange={(event) => setNameDraft({ workspaceId: workspace.id, value: event.target.value })} /></div>
        </form>
        {renameWorkspace.isError ? <p role="alert" className="text-destructive">Workspace rename failed. <Button variant="ghost" onClick={saveWorkspace}>Retry</Button></p> : null}
        {renameWorkspace.isPending ? <p role="status">Saving workspace…</p> : null}
      </SettingsCard>

      <SettingsCard title="Appearance" description="Theme for this browser.">
        <div className={GRID}>
          <div className={FIELD}>
            <Label className={FIELD_LABEL} htmlFor="appearance-theme">
              Theme
            </Label>
            <Listbox<Theme>
              id="appearance-theme"
              value={theme}
              options={[
                { value: 'light', label: 'Light' },
                { value: 'dark', label: 'Dark' },
              ]}
              onChange={setTheme}
            />
          </div>
        </div>
      </SettingsCard>

      <SettingsCard title="About" description="Version and backend status of this Orbit instance.">
        <div className={GRID}>
          <div className={FIELD}>
            <Label className={FIELD_LABEL} htmlFor="about-version">
              Version
            </Label>
            <Input id="about-version" value="0.1.0" readOnly />
          </div>
          <div className={FIELD}>
            <Label className={FIELD_LABEL} htmlFor="about-backend">
              Backend
            </Label>
            <Input
              id="about-backend"
              value="Connected"
              readOnly
            />
          </div>
          <div className={FIELD}>
            <Label className={FIELD_LABEL} htmlFor="about-storage">
              Storage
            </Label>
            <Input id="about-storage" value="Server data directory" readOnly />
          </div>
        </div>
      </SettingsCard>
      <SettingsCard title="Backup" description="Create a verified snapshot of the database and attachments without stopping Orbit.">
        <Button disabled={backup.isPending} onClick={() => backup.mutate()}>
          {backup.isPending ? 'Creating backup…' : 'Create backup now'}
        </Button>
        {backup.isSuccess ? <p role="status">Backup created: <code>{backup.data.id}</code></p> : null}
        {backup.isError ? <p role="alert" className="text-destructive">Backup failed. Installation administrator access is required.</p> : null}
      </SettingsCard>
      {dirty ? (
        <UnsavedBar
          onReset={() => { if (renameWorkspace.isPending) return; setNameDraft(null); renameWorkspace.reset() }}
          onSave={() => formRef.current?.requestSubmit()}
          saving={renameWorkspace.isPending}
        />
      ) : null}
    </>
  )
}
