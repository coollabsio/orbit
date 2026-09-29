import { useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Download } from 'reicon-react'
import { apiClient } from '@/api/client'
import { createBackup, listBackups } from '@/api/generated/sdk.gen'
import type { BackupSummary } from '@/api/generated/types.gen'
import { queryKeys } from '@/api/queryKeys'
import { UnsavedBar } from '@/components/common/UnsavedBar'
import { Badge } from '@/components/ui/badge'
import { Button, buttonVariants } from '@/components/ui/button'
import { formatSize } from '@/lib/attachmentLib'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useTheme, type Theme } from '@/lib/themeContext'
import { useRenameWorkspace } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { SettingsCard } from '@/components/common/SettingsCard'
import { FieldGrid, SettingsRow } from '@/features/settings/components/SettingsParts'
import { useSlowPending } from '@/lib/useDebouncedValue'

const THEME_OPTIONS = [
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
] as const


export function GeneralPage() {
  const { theme, setTheme } = useTheme()
  const { workspace } = useWorkspace()
  const renameWorkspace = useRenameWorkspace(workspace.id)
  const renameSlow = useSlowPending(renameWorkspace.isPending)
  const queryClient = useQueryClient()
  const backups = useQuery({
    queryKey: queryKeys.backups,
    queryFn: async () => {
      const { data } = await listBackups({ client: apiClient, throwOnError: true })
      return data.items
    },
  })
  const backup = useMutation({
    mutationFn: async () => {
      const { data } = await createBackup({ client: apiClient, throwOnError: true })
      if (!data) throw new Error('Backup response was empty.')
      return data
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: queryKeys.backups }),
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
        <form ref={formRef} onSubmit={(event) => { event.preventDefault(); saveWorkspace() }}>
          <FieldGrid>
            <Field><FieldLabel htmlFor="workspace-name">Name</FieldLabel><Input id="workspace-name" required disabled={renameSlow} value={name} onChange={(event) => setNameDraft({ workspaceId: workspace.id, value: event.target.value })} /></Field>
          </FieldGrid>
        </form>
        {renameWorkspace.isError ? <p role="alert" className="text-destructive">Workspace rename failed. <Button variant="ghost" onClick={saveWorkspace}>Retry</Button></p> : null}
        {renameSlow ? <p role="status">Saving workspace…</p> : null}
      </SettingsCard>

      <SettingsCard title="Appearance" description="Theme for this browser.">
        <FieldGrid>
          <Field>
            <FieldLabel htmlFor="appearance-theme">
              Theme
            </FieldLabel>
            {/* `items` lets Select.Value render the option label instead of the raw value. */}
            <Select items={THEME_OPTIONS} value={theme} onValueChange={(value) => setTheme(value as Theme)}>
              <SelectTrigger id="appearance-theme">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {THEME_OPTIONS.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
              </SelectContent>
            </Select>
          </Field>
        </FieldGrid>
      </SettingsCard>

      <SettingsCard title="About" description="Version and backend status of this Orbit instance.">
        <FieldGrid>
          <Field>
            <FieldLabel htmlFor="about-version">
              Version
            </FieldLabel>
            <Input id="about-version" value="0.1.0" readOnly />
          </Field>
          <Field>
            <FieldLabel htmlFor="about-backend">
              Backend
            </FieldLabel>
            <Input
              id="about-backend"
              value="Connected"
              readOnly
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="about-storage">
              Storage
            </FieldLabel>
            <Input id="about-storage" value="Server data directory" readOnly />
          </Field>
        </FieldGrid>
      </SettingsCard>
      <SettingsCard
        title="Backup"
        description="Create a verified snapshot of the database and attachments without stopping Orbit."
        actions={
          <Button disabled={backup.isPending} onClick={() => backup.mutate()}>
            {backup.isPending ? 'Creating backup…' : 'Create backup now'}
          </Button>
        }
        flush
      >
        <div className="flex flex-col divide-y text-[13px]">
          {backup.isError ? <SettingsRow role="alert"><span className="text-destructive">Backup failed. Installation administrator access is required.</span></SettingsRow> : null}
          {backups.isPending ? <SettingsRow>Loading backups…</SettingsRow> : null}
          {backups.isError ? <SettingsRow role="alert">Backups could not be loaded. Installation administrator access is required. <Button variant="ghost" onClick={() => void backups.refetch()}>Retry</Button></SettingsRow> : null}
          {backups.data?.length === 0 ? <SettingsRow className="text-muted-foreground">No backups yet.</SettingsRow> : null}
          {backups.data?.map((item) => <BackupRow key={item.id} backup={item} />)}
        </div>
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

function BackupRow({ backup }: { backup: BackupSummary }) {
  const created = new Date(backup.created_at)
  return (
    <SettingsRow className="flex-nowrap">
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="flex items-center gap-2 font-medium text-foreground">
          <time dateTime={created.toISOString()}>{created.toLocaleString()}</time>
          {backup.kind === 'pre_migration' ? <Badge variant="secondary">Before upgrade</Badge> : null}
        </span>
        <span className="text-xs text-muted-foreground/70">
          {formatSize(backup.byte_size)} · {backup.file_count} {backup.file_count === 1 ? 'file' : 'files'} · Orbit {backup.application_version}
        </span>
      </div>
      <a
        href={`/api/v1/admin/backups/${encodeURIComponent(backup.id)}/download`}
        download
        className={buttonVariants({ variant: 'outline', size: 'sm' })}
        aria-label={`Download backup from ${created.toLocaleString()}`}
      >
        <Download className="size-4" />
        Download
      </a>
    </SettingsRow>
  )
}
