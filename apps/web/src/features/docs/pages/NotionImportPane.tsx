import { useState, type FormEvent, type ReactNode } from 'react'
import { Link, useNavigate } from 'react-router'
import { toast } from 'sonner'
import { Import, TickCircle, Warning } from 'reicon-react'
import type { NotionImport } from '@orbit/api/generated/types.gen'
import { notionDatabaseNotes } from '@orbit/database/notionImport'
import { apiClient } from '@/api/client'
import { cn } from 'cn'
import { Button, buttonVariants } from '@/components/ui/button'
import { PasswordInput } from '@/components/ui/password-input'
import { Label } from '@/components/ui/label'
import { Spinner } from '@/components/ui/spinner'
import { EmptyState } from '@/components/common/EmptyState'
import { Pane } from '@/components/common/Pane'
import { confirmAction } from '@/components/common/confirmAction'
import {
  createNotionImportRequest,
  isActiveImport,
  isFinalImport,
  isImportInProgress,
  notionImportErrorMessage,
  reconcileNotionImport,
  useCancelNotionImport,
  useNotionImport,
  useNotionImportTree,
  useNotionImports,
} from '@/features/docs/api/notionImports'
import { isPageNotFound, usePageTree } from '@/features/docs/api/pages'
import { useTeamspaces } from '@/features/docs/api/teamspaces'
import { NotionImportChooser } from '@/features/docs/components/NotionImportChooser'
import { ImportBody, ImportHeader, ImportedPages, ProgressBar, RecentImports, Stat } from '@/features/docs/components/ImportParts'
import { destinationLabel } from '@/features/docs/import/destination'
import { useQueryClient } from '@tanstack/react-query'

const TOKEN_URL = 'https://www.notion.so/developers/tokens'

/** Step 1: paste a token. Also lists the member's recent imports. */
function ConnectStep({ workspaceId }: { workspaceId: string }) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const imports = useNotionImports(workspaceId)
  const [token, setToken] = useState('')
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<unknown>(null)
  const running = imports.data?.find((item) => isActiveImport(item.status))

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    const value = token.trim()
    if (!value || pending) return
    setPending(true)
    setError(null)
    try {
      const created = await createNotionImportRequest(apiClient, workspaceId, value)
      setToken('')
      reconcileNotionImport(queryClient, workspaceId, created)
      navigate(`/docs/import/${created.id}`)
    } catch (caught) {
      setError(caught)
      if (isImportInProgress(caught)) void imports.refetch()
    } finally {
      setPending(false)
    }
  }

  return (
    <>
      <ImportHeader title="Import from Notion" />
      <ImportBody>
        <section className="flex flex-col gap-4 rounded-lg border border-border p-5">
          <div className="flex flex-col gap-1">
            <h2 className="text-sm font-semibold text-foreground">Connect Notion</h2>
            <p className="text-sm text-muted-foreground">
              Create a personal access token at{' '}
              <a className="text-primary underline underline-offset-2" href={TOKEN_URL} target="_blank" rel="noreferrer noopener">
                {TOKEN_URL}
              </a>{' '}
              (it can read everything you can see in Notion). Orbit keeps it encrypted only until the import finishes.
            </p>
          </div>
          <form className="flex flex-col gap-3" onSubmit={(event) => void submit(event)} autoComplete="off">
            <div className="grid gap-1.5">
              <Label htmlFor="notion-token">Notion token</Label>
              <PasswordInput
                id="notion-token"
                name="notion-token"
                autoComplete="off"
                spellCheck={false}
                autoCapitalize="off"
                placeholder="ntn_…"
                value={token}
                aria-invalid={error ? true : undefined}
                onChange={(event) => setToken(event.target.value)}
              />
            </div>
            {error ? (
              <p role="alert" className="text-sm text-destructive">
                {notionImportErrorMessage(error)}{' '}
                {isImportInProgress(error) && running ? (
                  <Link className="text-primary underline underline-offset-2" to={`/docs/import/${running.id}`}>
                    Open the running import
                  </Link>
                ) : null}
              </p>
            ) : null}
            <div>
              <Button type="submit" disabled={!token.trim() || pending}>
                {pending ? <Spinner className="size-4" /> : null}
                Scan workspace
              </Button>
            </div>
          </form>
        </section>
        <RecentImports
          imports={imports.data}
          isPending={imports.isPending}
          name={(item) => item.notion_workspace_name || 'Notion'}
          href={(item) => `/docs/import/${item.id}`}
        />
      </ImportBody>
    </>
  )
}

function useCancel(workspaceId: string, item: NotionImport) {
  const cancel = useCancelNotionImport(workspaceId)
  const request = async (started: boolean) => {
    const confirmed = await confirmAction({
      title: started ? 'Cancel the import?' : 'Cancel this import?',
      description: started
        ? 'Pages already imported stay. Empty pages the import created but did not fill yet are moved to the trash.'
        : 'Orbit deletes the Notion token. You can start a new import at any time.',
      confirmLabel: 'Cancel import',
      cancelLabel: 'Keep going',
      danger: true,
    })
    if (!confirmed) return
    cancel.mutate(
      { importId: item.id },
      { onError: (error) => toast.error(notionImportErrorMessage(error)) },
    )
  }
  return { request, pending: cancel.isPending }
}

/** Step 2: the scan job lists what the token can see. */
function ScanningStep({ workspaceId, item }: { workspaceId: string; item: NotionImport }) {
  const cancel = useCancel(workspaceId, item)
  return (
    <>
      <ImportHeader title="Import from Notion" detail={item.notion_workspace_name} />
      <ImportBody>
        <div className="flex flex-col items-center gap-3 py-16 text-center" role="status">
          <Spinner className="size-6 text-muted-foreground" />
          <p className="text-sm font-medium text-foreground">
            Scanning {item.notion_workspace_name ? `“${item.notion_workspace_name}”` : 'your Notion workspace'}…
          </p>
          <p className="max-w-sm text-sm text-muted-foreground">Listing every page the token can see. Large workspaces take a minute.</p>
          <Button type="button" variant="outline" size="sm" disabled={cancel.pending} onClick={() => void cancel.request(false)}>
            Cancel
          </Button>
        </div>
      </ImportBody>
    </>
  )
}

/** Step 4: the import job runs. */
function ImportingStep({ workspaceId, item }: { workspaceId: string; item: NotionImport }) {
  const cancel = useCancel(workspaceId, item)
  const { done, total, failed } = item.progress
  return (
    <>
      <ImportHeader title="Import from Notion" detail={item.notion_workspace_name} />
      <ImportBody>
        <section className="flex flex-col gap-4 py-10" role="status" aria-live="polite">
          <div className="flex items-center gap-2">
            <Spinner className="size-4 text-muted-foreground" />
            <p className="text-sm font-medium text-foreground">
              {item.status === 'queued' ? 'Waiting for the import to start…' : 'Importing pages…'}
            </p>
          </div>
          <ProgressBar done={done + failed} total={total} label="Import progress" />
          <div className="flex items-center gap-4 text-sm text-muted-foreground">
            <span>
              {done} of {total} pages imported
            </span>
            {failed > 0 ? <span className="text-destructive">{failed} failed</span> : null}
            <span className="flex-1" />
            <Button type="button" variant="outline" size="sm" disabled={cancel.pending} onClick={() => void cancel.request(true)}>
              Cancel import
            </Button>
          </div>
          <p className="text-sm text-muted-foreground">
            You can leave this page; the import continues on the server and new pages appear in the sidebar as they are created.
          </p>
        </section>
      </ImportBody>
    </>
  )
}

function countEntries(map: Record<string, number>): [string, number][] {
  return Object.entries(map)
    .filter(([, count]) => count > 0)
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
}

/** Step 5: completed, failed, cancelled or expired. */
function ResultStep({ workspaceId, item }: { workspaceId: string; item: NotionImport }) {
  const tree = usePageTree(workspaceId)
  const teamspaces = useTeamspaces(workspaceId)
  const report = item.report
  const destination = destinationLabel(item.destination, teamspaces.data, tree.data)
  const headline =
    item.status === 'completed'
      ? 'Import complete'
      : item.status === 'failed'
        ? 'Import failed'
        : item.status === 'cancelled'
          ? 'Import cancelled'
          : 'Import expired'
  const database = report ? notionDatabaseNotes(report) : null
  const skipped = countEntries(database?.skipped ?? {})
  const lossy = countEntries(database?.lossy ?? {})
  const notes: string[] = []
  if (report) {
    if (report.missing_files > 0) notes.push(`${report.missing_files} Notion files could not be downloaded.`)
    if (report.unresolved_page_links > 0)
      notes.push(`${report.unresolved_page_links} links point to Notion pages that were not imported; they still open Notion.`)
    if (report.truncated_blocks > 0) notes.push(`${report.truncated_blocks} blocks were cut from pages that were too large.`)
    if (report.previously_imported > 0)
      notes.push(`${report.previously_imported} pages were imported before; this import created new copies.`)
    if (report.unfilled_pages_trashed > 0)
      notes.push(`${report.unfilled_pages_trashed} empty pages the import had created were moved to the trash.`)
    notes.push(...(database?.notes ?? []))
  }

  return (
    <>
      <ImportHeader title="Import from Notion" detail={item.notion_workspace_name} />
      <ImportBody>
        <section className="flex flex-col gap-3">
          <div className="flex items-center gap-2">
            {item.status === 'completed' ? (
              <TickCircle className="size-5 text-primary" />
            ) : (
              <Warning className={cn('size-5', item.status === 'failed' ? 'text-destructive' : 'text-muted-foreground')} />
            )}
            <h2 className="text-base font-semibold text-foreground">{headline}</h2>
          </div>
          {item.error ? <p className="text-sm text-muted-foreground">{item.error}</p> : null}
          {destination && item.progress.total > 0 ? <p className="text-sm text-muted-foreground">Destination: {destination}</p> : null}
        </section>
        {item.progress.total > 0 ? (
          <div className="grid grid-cols-3 gap-2 max-[599px]:grid-cols-1">
            <Stat label="Pages imported" value={item.progress.done} />
            <Stat label="Files imported" value={report?.files_imported ?? 0} />
            <Stat label="Pages failed" value={item.progress.failed} />
          </div>
        ) : null}
        <ImportedPages rootIds={item.root_page_ids} pages={tree.data} />
        {notes.length > 0 || skipped.length > 0 || lossy.length > 0 ? (
          <section className="flex flex-col gap-2" aria-label="Conversion notes">
            <h3 className="text-xs font-medium text-muted-foreground">Conversion notes</h3>
            {notes.map((note) => (
              <p key={note} className="text-sm text-foreground">
                {note}
              </p>
            ))}
            {skipped.length > 0 ? (
              <p className="text-sm text-foreground">
                Skipped blocks (not supported in Orbit):{' '}
                <span className="text-muted-foreground">{skipped.map(([kind, count]) => `${kind.replaceAll('_', ' ')} (${count})`).join(', ')}</span>
              </p>
            ) : null}
            {lossy.length > 0 ? (
              <p className="text-sm text-foreground">
                Simplified blocks:{' '}
                <span className="text-muted-foreground">{lossy.map(([kind, count]) => `${kind.replaceAll('_', ' ')} (${count})`).join(', ')}</span>
              </p>
            ) : null}
          </section>
        ) : null}
        {report && report.failures.length > 0 ? (
          <section className="flex flex-col gap-2" aria-label="Failed pages">
            <h3 className="text-xs font-medium text-muted-foreground">Failed pages</h3>
            <ul className="flex flex-col border-t border-border">
              {report.failures.map((failure) => (
                <li key={failure.notion_id} className="flex flex-col gap-0.5 border-b border-border px-1 py-2">
                  <span className="text-[13px] text-foreground">{failure.title.trim() ? failure.title : 'Untitled'}</span>
                  <span className="text-xs text-muted-foreground">{failure.error}</span>
                </li>
              ))}
            </ul>
          </section>
        ) : null}
        <div>
          <Link className={buttonVariants({ variant: 'outline' })} to="/docs/import">
            <Import className="size-4" />
            Import more
          </Link>
        </div>
      </ImportBody>
    </>
  )
}

/** Step 3 wrapper: header with the workspace name and a cancel action, then the chooser (scan tree fetched once). */
function ChooseStep({ workspaceId, item }: { workspaceId: string; item: NotionImport }) {
  const cancel = useCancel(workspaceId, item)
  const scan = useNotionImportTree(workspaceId, item.id)
  const tree = usePageTree(workspaceId)
  const teamspaces = useTeamspaces(workspaceId)
  return (
    <>
      <ImportHeader
        title="Choose pages"
        detail={item.notion_workspace_name}
        actions={
          <Button type="button" variant="ghost" size="sm" disabled={cancel.pending} onClick={() => void cancel.request(false)}>
            Cancel
          </Button>
        }
      />
      {scan.data ? (
        <NotionImportChooser workspaceId={workspaceId} item={item} tree={scan.data} teamspaces={teamspaces.data} pages={tree.data} />
      ) : scan.isError || (scan.isSuccess && !scan.data) ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center" role="alert">
          <p className="text-sm text-muted-foreground">
            {scan.isError ? notionImportErrorMessage(scan.error) : 'The scanned page list is not available.'}
          </p>
          <Button type="button" variant="outline" size="sm" onClick={() => void scan.refetch()}>
            Retry
          </Button>
        </div>
      ) : (
        <div className="flex flex-1 items-center justify-center">
          <Spinner className="text-muted-foreground" />
        </div>
      )}
    </>
  )
}

/**
 * `/docs/import` (connect + recent imports) and `/docs/import/:importId` (one import through its states). A pane
 * next to the page tree, like Trash: imported pages appear in the tree while the import runs, and the URL survives
 * reloads during a long import.
 */
export function NotionImportPane({ workspaceId, importId }: { workspaceId: string; importId?: string }) {
  const query = useNotionImport(workspaceId, importId)
  let content: ReactNode
  if (!importId) {
    content = <ConnectStep workspaceId={workspaceId} />
  } else if (query.data) {
    const item = query.data
    if (item.status === 'scanning') content = <ScanningStep workspaceId={workspaceId} item={item} />
    else if (item.status === 'ready') content = <ChooseStep key={item.id} workspaceId={workspaceId} item={item} />
    else if (item.status === 'queued' || item.status === 'importing') content = <ImportingStep workspaceId={workspaceId} item={item} />
    else if (isFinalImport(item.status)) content = <ResultStep workspaceId={workspaceId} item={item} />
  } else if (query.isError) {
    const missing = isPageNotFound(query.error)
    content = (
      <>
        <ImportHeader title="Import from Notion" />
        <div className="flex flex-1 flex-col p-6">
          <EmptyState
            icon={Import}
            title={missing ? 'Import not found' : 'Import unavailable'}
            description={missing ? 'This import does not exist or belongs to someone else.' : 'The server could not load this import.'}
            action={
              <div className="flex gap-2">
                {missing ? null : (
                  <Button type="button" variant="outline" onClick={() => void query.refetch()}>
                    Retry
                  </Button>
                )}
                <Link className={buttonVariants({ variant: 'outline' })} to="/docs/import">
                  New import
                </Link>
              </div>
            }
          />
        </div>
      </>
    )
  } else {
    content = (
      <>
        <ImportHeader title="Import from Notion" />
        <div className="flex flex-1 items-center justify-center">
          <Spinner className="text-muted-foreground" />
        </div>
      </>
    )
  }
  return <Pane>{content}</Pane>
}
