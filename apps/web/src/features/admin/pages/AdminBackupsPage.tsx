import { useState } from 'react'
import { Download, FolderOpen, Trash } from 'reicon-react'
import { ApiProblem } from '@orbit/api/problem'
import type { BackupSummary } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Badge } from '@/components/ui/badge'
import { Button, buttonVariants } from '@/components/ui/button'
import { useBackups, useCreateBackup, useDeleteBackup } from '@/features/admin/api'
import { BackupBrowseDialog } from '@/features/admin/components/BackupBrowseDialog'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { formatSize } from '@/lib/attachmentLib'

/** Snapshots of the whole instance: every workspace, account and attachment. */
export function AdminBackupsPage() {
  const backups = useBackups()
  const backup = useCreateBackup()
  const remove = useDeleteBackup()
  const [browsing, setBrowsing] = useState<BackupSummary | null>(null)
  return (
    <SettingsCard
      title="Backups"
      description="Create a verified snapshot of the database and attachments without stopping Orbit."
      actions={
        <Button disabled={backup.isPending} onClick={() => backup.mutate()}>
          {backup.isPending ? 'Creating backup…' : 'Create backup now'}
        </Button>
      }
      flush
    >
      <div className="flex flex-col divide-y text-[0.8125rem]">
        {backup.isError ? <SettingsRow role="alert"><span className="text-destructive">{backup.error instanceof ApiProblem ? backup.error.detail : 'Backup failed.'}</span></SettingsRow> : null}
        {remove.isError ? <SettingsRow role="alert"><span className="text-destructive">The backup could not be deleted.</span></SettingsRow> : null}
        {backups.isError ? <SettingsRow role="alert">Backups could not be loaded. <Button variant="ghost" onClick={() => void backups.refetch()}>Retry</Button></SettingsRow> : null}
        {backups.data?.length === 0 ? <SettingsRow className="text-muted-foreground">No backups yet.</SettingsRow> : null}
        {backups.data?.map((item) => <BackupRow key={item.id} backup={item} deleting={remove.isPending && remove.variables === item.id} onBrowse={() => setBrowsing(item)} onDelete={() => remove.mutate(item.id)} />)}
      </div>
      {browsing ? <BackupBrowseDialog backup={browsing} onClose={() => setBrowsing(null)} /> : null}
    </SettingsCard>
  )
}

function BackupRow({ backup, deleting, onBrowse, onDelete }: { backup: BackupSummary; deleting: boolean; onBrowse: () => void; onDelete: () => void }) {
  const created = new Date(backup.created_at)
  async function confirmDelete() {
    if (!await confirmAction({ title: 'Delete this backup?', description: `The backup from ${created.toLocaleString()} is removed for good. You cannot undo this.`, confirmLabel: 'Delete', danger: true })) return
    onDelete()
  }
  return (
    <SettingsRow className="flex-nowrap">
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="flex items-center gap-2 font-medium text-foreground">
          <time dateTime={created.toISOString()}>{created.toLocaleString()}</time>
          {backup.kind === 'pre_migration' ? <Badge variant="secondary">Before upgrade</Badge> : null}
          {backup.location === 's3' ? <Badge variant="secondary">S3</Badge> : null}
        </span>
        <span className="text-xs text-muted-foreground/70">
          {formatSize(backup.byte_size)} · {backup.file_count} {backup.file_count === 1 ? 'file' : 'files'} · Orbit {backup.application_version}
        </span>
      </div>
      <Button variant="outline" size="sm" onClick={onBrowse} aria-label={`Browse backup from ${created.toLocaleString()}`}>
        <FolderOpen className="size-4" />
        Browse
      </Button>
      <a
        href={`/api/v1/admin/backups/${encodeURIComponent(backup.id)}/download`}
        download
        className={buttonVariants({ variant: 'outline', size: 'sm' })}
        aria-label={`Download backup from ${created.toLocaleString()}`}
      >
        <Download className="size-4" />
        Download
      </a>
      <Button variant="outline" size="sm" disabled={deleting} onClick={() => void confirmDelete()} aria-label={`Delete backup from ${created.toLocaleString()}`}>
        <Trash className="size-4" />
        {deleting ? 'Deleting…' : 'Delete'}
      </Button>
    </SettingsRow>
  )
}
