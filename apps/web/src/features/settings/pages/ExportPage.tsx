import { Download } from 'reicon-react'
import { toast } from 'sonner'
import type { WorkspaceExport } from '@orbit/api/generated/types.gen'
import { ApiProblem } from '@orbit/api/problem'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Button, buttonVariants } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { isActiveExport, useStartWorkspaceExport, useWorkspaceExports } from '@/features/settings/api/exports'
import { SettingsRow } from '@/features/settings/components/SettingsParts'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { formatSize } from '@/lib/attachmentLib'

const TITLE = 'Export workspace'

/** A ZIP of the whole workspace (tasks, pages, chat and files) to download and keep. */
export function ExportPage() {
  const { workspace } = useWorkspace()
  const canExport = useCan('workspace.export')
  const exports = useWorkspaceExports(workspace.id, canExport)
  const start = useStartWorkspaceExport(workspace.id)
  if (!canExport) return <SettingsCard title={TITLE}><p className="m-0 text-muted-foreground">Only workspace owners and administrators can export the workspace.</p></SettingsCard>
  const active = exports.data?.some(isActiveExport) ?? false
  return (
    <SettingsCard
      title={TITLE}
      description="Download everything in this workspace as one ZIP file: tasks with comments, pages as Markdown, chat and all attached files. It includes your private pages and the private channels and direct messages you are in. Each download is kept for 7 days."
      actions={
        <Button
          disabled={active || start.isPending || !exports.isSuccess}
          onClick={() => start.mutate(undefined, {
            onSuccess: () => toast('Export started'),
            onError: (error) => toast.error(error instanceof ApiProblem ? error.detail : 'The export could not be started.'),
          })}
        >
          {active ? 'Export in progress' : 'Start export'}
        </Button>
      }
      flush
    >
      <div className="flex flex-col divide-y text-[13px]">
        {exports.isError ? <SettingsRow role="alert">Exports could not be loaded. <Button variant="ghost" onClick={() => void exports.refetch()}>Retry</Button></SettingsRow> : null}
        {exports.data?.length === 0 ? <SettingsRow className="text-muted-foreground">No exports yet.</SettingsRow> : null}
        {exports.data?.map((item) => <ExportRow key={item.id} workspaceId={workspace.id} item={item} />)}
      </div>
    </SettingsCard>
  )
}

function ExportRow({ workspaceId, item }: { workspaceId: string; item: WorkspaceExport }) {
  const created = new Date(item.created_at).toLocaleString()
  const expires = new Date(item.expires_at).toLocaleString()
  return (
    <SettingsRow className="flex-nowrap">
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="font-medium text-foreground">
          <time dateTime={item.created_at}>{created}</time>
          {item.requested_by_name ? <span className="font-normal text-muted-foreground"> · {item.requested_by_name}</span> : null}
        </span>
        {item.status === 'ready' ? (
          <span className="text-xs text-muted-foreground/70">{formatSize(item.size_bytes ?? 0)} · Deleted on {expires}</span>
        ) : item.status === 'failed' ? (
          <span className="text-xs text-destructive">The export failed. Start a new one.</span>
        ) : (
          <span role="status" className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <Spinner role="presentation" aria-hidden aria-label={undefined} className="size-3.5" />
            {item.status === 'queued' ? 'Waiting to start…' : 'Preparing the archive…'}
          </span>
        )}
      </div>
      {item.status === 'ready' ? (
        <a
          href={`/api/v1/workspaces/${encodeURIComponent(workspaceId)}/exports/${encodeURIComponent(item.id)}/download`}
          download
          className={buttonVariants({ variant: 'outline', size: 'sm' })}
          aria-label={`Download export from ${created}`}
        >
          <Download className="size-4" />
          Download
        </a>
      ) : null}
    </SettingsRow>
  )
}
