import { useMemo, useState, type FormEvent, type ReactNode } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { toast } from 'sonner'
import { useQueryClient } from '@tanstack/react-query'
import { Import, Search4 as Search, TickCircle, Warning } from 'reicon-react'
import type { AsanaImport, AsanaImportProject, AsanaImportTree } from '@orbit/api/generated/types.gen'
import { apiClient } from '@/api/client'
import { cn } from 'cn'
import { Badge } from '@/components/ui/badge'
import { Button, buttonVariants } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { Label } from '@/components/ui/label'
import { PasswordInput } from '@/components/ui/password-input'
import { Spinner } from '@/components/ui/spinner'
import { EmptyState } from '@/components/common/EmptyState'
import { Pane } from '@/components/common/Pane'
import { confirmAction } from '@/components/common/confirmAction'
import { relativeTime } from '@/lib/format'
import { ApiProblem } from '@orbit/api/problem'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useProjects } from '@/features/tasks/api/projects'
import { IMPORT_STATUS_LABELS, importStatusVariant as statusVariant, isActiveImport, isFinalImport } from '@/features/tasks/api/importStatus'
import { ImportBody as Body, ImportHeader, ImportProgressBar as ProgressBar, ImportStat as Stat } from '@/features/tasks/components/ImportLayout'
import {
  asanaImportErrorMessage,
  createAsanaImportRequest,
  isAsanaImportInProgress,
  reconcileAsanaImport,
  useAsanaImport,
  useAsanaImportTree,
  useAsanaImports,
  useCancelAsanaImport,
  useStartAsanaImport,
} from '@/features/tasks/api/asanaImports'

const TOKEN_URL = 'https://app.asana.com/0/my-apps'

/** Step 1: paste a personal access token. Also lists the member's recent imports. */
function ConnectStep({ workspaceId }: { workspaceId: string }) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const imports = useAsanaImports(workspaceId)
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
      const created = await createAsanaImportRequest(apiClient, workspaceId, value)
      setToken('')
      reconcileAsanaImport(queryClient, workspaceId, created)
      navigate(`/tasks-import/${created.id}`)
    } catch (caught) {
      setError(caught)
      if (isAsanaImportInProgress(caught)) void imports.refetch()
    } finally {
      setPending(false)
    }
  }

  return (
    <>
      <ImportHeader title="Import from Asana" />
      <Body>
        <section className="flex flex-col gap-4 rounded-lg border border-border p-5">
          <div className="flex flex-col gap-1">
            <h2 className="text-sm font-semibold text-foreground">Connect Asana</h2>
            <p className="text-sm text-muted-foreground">
              In Asana, open{' '}
              <a className="text-primary underline underline-offset-2" href={TOKEN_URL} target="_blank" rel="noreferrer noopener">
                {TOKEN_URL}
              </a>
              , choose <span className="font-medium text-foreground">Create new token</span>, name it (for example “Orbit import”) and copy it. The
              token can read everything you can see in Asana. Orbit keeps it encrypted only until the import finishes; you can delete it in Asana
              afterwards.
            </p>
          </div>
          <form className="flex flex-col gap-3" onSubmit={(event) => void submit(event)} autoComplete="off">
            <div className="grid gap-1.5">
              <Label htmlFor="asana-token">Asana personal access token</Label>
              <PasswordInput
                id="asana-token"
                name="asana-token"
                autoComplete="off"
                spellCheck={false}
                autoCapitalize="off"
                placeholder="2/1200…"
                value={token}
                aria-invalid={error ? true : undefined}
                onChange={(event) => setToken(event.target.value)}
              />
            </div>
            {error ? (
              <p role="alert" className="text-sm text-destructive">
                {asanaImportErrorMessage(error)}{' '}
                {isAsanaImportInProgress(error) && running ? (
                  <Link className="text-primary underline underline-offset-2" to={`/tasks-import/${running.id}`}>
                    Open the running import
                  </Link>
                ) : null}
              </p>
            ) : null}
            <div>
              <Button type="submit" disabled={!token.trim() || pending}>
                {pending ? <Spinner className="size-4" /> : null}
                Find projects
              </Button>
            </div>
          </form>
        </section>
        <RecentImports imports={imports.data} />
      </Body>
    </>
  )
}

function RecentImports({ imports }: { imports: AsanaImport[] | undefined }) {
  if (!imports?.length) return null
  return (
    <section className="flex flex-col gap-2" aria-label="Recent imports">
      <h2 className="text-xs font-medium text-muted-foreground">Recent imports</h2>
      <div className="flex flex-col border-t border-border">
        {imports.map((item) => (
          <Link
            key={item.id}
            to={`/tasks-import/${item.id}`}
            className="flex min-h-10 min-w-0 items-center gap-3 border-b border-border px-1 py-1.5 text-[13px] transition-colors duration-150 ease-out hover:bg-sidebar-accent/50"
          >
            <span className="min-w-0 flex-1 truncate text-foreground">{item.asana_user_name || 'Asana'}</span>
            {item.progress.total > 0 ? (
              <span className="shrink-0 text-xs text-muted-foreground">
                {item.progress.done} of {item.progress.total} tasks
              </span>
            ) : null}
            <Badge variant={statusVariant(item.status)}>{IMPORT_STATUS_LABELS[item.status]}</Badge>
            <span className="w-12 shrink-0 text-right text-xs text-muted-foreground" title={new Date(item.created_at).toLocaleString()}>
              {relativeTime(item.created_at)}
            </span>
          </Link>
        ))}
      </div>
    </section>
  )
}

function useCancel(workspaceId: string, item: AsanaImport) {
  const cancel = useCancelAsanaImport(workspaceId)
  const request = async (started: boolean) => {
    const confirmed = await confirmAction({
      title: started ? 'Cancel the import?' : 'Cancel this import?',
      description: started
        ? 'Projects and tasks already imported stay. The import stops before the next task.'
        : 'Orbit deletes the Asana token. You can start a new import at any time.',
      confirmLabel: 'Cancel import',
      cancelLabel: 'Keep going',
      danger: true,
    })
    if (!confirmed) return
    cancel.mutate({ importId: item.id }, { onError: (error) => toast.error(asanaImportErrorMessage(error)) })
  }
  return { request, pending: cancel.isPending }
}

/** Step 2: the scan job lists the workspaces and projects the token can see. */
function ScanningStep({ workspaceId, item }: { workspaceId: string; item: AsanaImport }) {
  const cancel = useCancel(workspaceId, item)
  return (
    <>
      <ImportHeader title="Import from Asana" detail={item.asana_user_name} />
      <Body>
        <div className="flex flex-col items-center gap-3 py-16 text-center" role="status">
          <Spinner className="size-6 text-muted-foreground" />
          <p className="text-sm font-medium text-foreground">Finding your Asana projects…</p>
          <p className="max-w-sm text-sm text-muted-foreground">Listing every workspace and project the token can see.</p>
          <Button type="button" variant="outline" size="sm" disabled={cancel.pending} onClick={() => void cancel.request(false)}>
            Cancel
          </Button>
        </div>
      </Body>
    </>
  )
}

/** One workspace's projects grouped by team, teams in the order the scan sorted them. */
function groupByTeam(projects: AsanaImportProject[]): { team: string; projects: AsanaImportProject[] }[] {
  const groups = new Map<string, AsanaImportProject[]>()
  for (const project of projects) {
    const team = project.team_name ?? 'No team'
    groups.set(team, [...(groups.get(team) ?? []), project])
  }
  return [...groups].map(([team, list]) => ({ team, projects: list }))
}

function checkState(gids: string[], selected: ReadonlySet<string>): { checked: boolean; indeterminate: boolean } {
  const count = gids.filter((gid) => selected.has(gid)).length
  return { checked: count > 0 && count === gids.length, indeterminate: count > 0 && count < gids.length }
}

/** Step 3: pick projects. Active projects start selected, archived ones do not. */
function ProjectChooser({ workspaceId, item, tree }: { workspaceId: string; item: AsanaImport; tree: AsanaImportTree }) {
  const [selected, setSelected] = useState<ReadonlySet<string>>(
    () => new Set(tree.workspaces.flatMap((workspace) => workspace.projects.filter((project) => !project.archived).map((project) => project.gid))),
  )
  const [query, setQuery] = useState('')
  const start = useStartAsanaImport(workspaceId)
  const needle = query.trim().toLowerCase()
  const workspaces = useMemo(
    () =>
      tree.workspaces
        .map((workspace) => ({
          ...workspace,
          projects: needle
            ? workspace.projects.filter((project) => `${project.name} ${project.team_name ?? ''}`.toLowerCase().includes(needle))
            : workspace.projects,
        }))
        .filter((workspace) => workspace.projects.length > 0),
    [tree.workspaces, needle],
  )
  const visible = workspaces.flatMap((workspace) => workspace.projects.map((project) => project.gid))
  const count = selected.size
  const toggle = (gids: string[], on: boolean) =>
    setSelected((current) => {
      const next = new Set(current)
      for (const gid of gids) {
        if (on) next.add(gid)
        else next.delete(gid)
      }
      return next
    })
  const all = checkState(visible, selected)

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {tree.truncated ? (
        <p role="status" className="flex items-start gap-2 border-b border-border bg-amber-500/5 px-4 py-2 text-xs text-foreground">
          <Warning className="mt-px size-3.5 shrink-0 text-amber-600" />
          This token can see more than 5,000 projects. Only the first 5,000 are listed.
        </p>
      ) : null}
      <div className="flex flex-wrap items-center gap-3 border-b border-border px-3 py-2">
        <InputGroup className="w-64 rounded-lg border-border bg-foreground/[0.02] max-[899px]:w-full">
          <InputGroupAddon align="inline-start">
            <Search className="size-3.5 text-muted-foreground/70" />
          </InputGroupAddon>
          <InputGroupInput
            type="search"
            className="text-xs"
            placeholder="Filter projects"
            aria-label="Filter projects"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </InputGroup>
        <div className="flex items-center gap-2 text-[13px] text-foreground">
          <Checkbox
            aria-label="Select all projects"
            checked={all.checked}
            indeterminate={all.indeterminate}
            disabled={visible.length === 0}
            onCheckedChange={() => toggle(visible, !all.checked)}
          />
          <button type="button" tabIndex={-1} disabled={visible.length === 0} onClick={() => toggle(visible, !all.checked)}>
            Select all
          </button>
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-2">
        {workspaces.length === 0 ? (
          <p className="p-4 text-sm text-muted-foreground">{needle ? 'No project matches the filter.' : 'This token cannot see any project.'}</p>
        ) : (
          workspaces.map((workspace) => (
            <section key={workspace.gid} className="flex flex-col gap-1 pb-3" aria-label={workspace.name}>
              <h2 className="px-2 pt-2 text-xs font-semibold text-foreground">{workspace.name}</h2>
              {groupByTeam(workspace.projects).map((group) => {
                const gids = group.projects.map((project) => project.gid)
                const state = checkState(gids, selected)
                return (
                  <div key={group.team} className="flex flex-col">
                    <div className="flex h-8 items-center gap-2 px-2 text-xs font-medium text-muted-foreground">
                      <Checkbox
                        aria-label={`Select team ${group.team}`}
                        checked={state.checked}
                        indeterminate={state.indeterminate}
                        onCheckedChange={() => toggle(gids, !state.checked)}
                      />
                      <button type="button" tabIndex={-1} className="min-w-0 truncate" onClick={() => toggle(gids, !state.checked)}>
                        {group.team}
                      </button>
                    </div>
                    {group.projects.map((project) => (
                      <div
                        key={project.gid}
                        className="flex h-8 items-center gap-2 rounded-md pr-2 pl-8 text-[13px] text-foreground transition-colors duration-150 ease-out hover:bg-sidebar-accent/50"
                      >
                        <Checkbox
                          aria-label={`Select ${project.name}`}
                          checked={selected.has(project.gid)}
                          onCheckedChange={(checked) => toggle([project.gid], checked)}
                        />
                        {/* Clicking the name toggles the row, like the checkbox (the checkbox is the keyboard control). */}
                        <button
                          type="button"
                          tabIndex={-1}
                          className="min-w-0 flex-1 truncate text-left"
                          onClick={() => toggle([project.gid], !selected.has(project.gid))}
                        >
                          {project.name}
                        </button>
                        {project.archived ? <Badge variant="outline">Archived</Badge> : null}
                      </div>
                    ))}
                  </div>
                )
              })}
            </section>
          ))
        )}
      </div>
      <div className="flex flex-wrap items-center gap-3 border-t border-border px-3 py-3">
        <p className="min-w-0 flex-1 text-xs text-muted-foreground">
          Each project becomes a new Orbit project with its sections, tasks, subtasks, comments, files and dependencies. Nobody is notified.
        </p>
        <div className="flex flex-col items-end gap-1.5">
          {start.isError ? (
            <p role="alert" className="max-w-96 text-right text-xs text-destructive">
              {asanaImportErrorMessage(start.error)}
            </p>
          ) : null}
          <Button
            type="button"
            className="tabular-nums"
            disabled={count === 0 || start.isPending}
            onClick={() => start.mutate({ importId: item.id, projectGids: [...selected] })}
          >
            Import {count} {count === 1 ? 'project' : 'projects'}
          </Button>
        </div>
      </div>
    </div>
  )
}

function ChooseStep({ workspaceId, item }: { workspaceId: string; item: AsanaImport }) {
  const cancel = useCancel(workspaceId, item)
  const scan = useAsanaImportTree(workspaceId, item.id)
  return (
    <>
      <ImportHeader
        title="Choose projects"
        detail={item.asana_user_name}
        actions={
          <Button type="button" variant="ghost" size="sm" disabled={cancel.pending} onClick={() => void cancel.request(false)}>
            Cancel
          </Button>
        }
      />
      {scan.data ? (
        <ProjectChooser workspaceId={workspaceId} item={item} tree={scan.data} />
      ) : scan.isError || (scan.isSuccess && !scan.data) ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 p-6 text-center" role="alert">
          <p className="text-sm text-muted-foreground">{scan.isError ? asanaImportErrorMessage(scan.error) : 'The project list is not available.'}</p>
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

/** Step 4: the import job runs. */
function ImportingStep({ workspaceId, item }: { workspaceId: string; item: AsanaImport }) {
  const cancel = useCancel(workspaceId, item)
  const { done, total, failed } = item.progress
  return (
    <>
      <ImportHeader title="Import from Asana" detail={item.asana_user_name} />
      <Body>
        <section className="flex flex-col gap-4 py-10" role="status" aria-live="polite">
          <div className="flex items-center gap-2">
            <Spinner className="size-4 text-muted-foreground" />
            <p className="text-sm font-medium text-foreground">
              {item.status === 'queued' ? 'Waiting for the import to start…' : total === 0 ? 'Reading projects…' : 'Importing tasks…'}
            </p>
          </div>
          <ProgressBar done={done + failed} total={total} />
          <div className="flex items-center gap-4 text-sm text-muted-foreground">
            <span>
              {done} of {total} tasks imported
            </span>
            {failed > 0 ? <span className="text-destructive">{failed} failed</span> : null}
            <span className="flex-1" />
            <Button type="button" variant="outline" size="sm" disabled={cancel.pending} onClick={() => void cancel.request(true)}>
              Cancel import
            </Button>
          </div>
          <p className="text-sm text-muted-foreground">
            You can leave this page; the import continues on the server. Asana allows about two requests per second, so large projects take a
            while.
          </p>
        </section>
      </Body>
    </>
  )
}

/** Step 5: completed, failed, cancelled or expired. */
function ResultStep({ workspaceId, item }: { workspaceId: string; item: AsanaImport }) {
  const projects = useProjects(workspaceId)
  const names = new Map((projects.data ?? []).map((project) => [project.id, project]))
  const report = item.report
  const headline =
    item.status === 'completed'
      ? 'Import complete'
      : item.status === 'failed'
        ? 'Import failed'
        : item.status === 'cancelled'
          ? 'Import cancelled'
          : 'Import expired'
  const notes: string[] = []
  if (report) {
    if (report.dependencies_linked > 0) notes.push(`${report.dependencies_linked} dependencies became “blocked by” relations.`)
    if (report.links_added > 0) notes.push(`${report.links_added} linked files (Google Drive, Dropbox and others) were added to descriptions as links.`)
    if (report.unmatched_assignees > 0)
      notes.push(`${report.unmatched_assignees} tasks had an assignee with no Orbit member of the same email; they are unassigned and name the assignee. Assign them to a member in Settings → Import.`)
    if (report.missing_files > 0) notes.push(`${report.missing_files} Asana files could not be downloaded.`)
    if (report.skipped_subtasks > 0) notes.push(`${report.skipped_subtasks} subtasks deeper than five levels were not imported.`)
  }
  return (
    <>
      <ImportHeader title="Import from Asana" detail={item.asana_user_name} />
      <Body>
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
        </section>
        {report ? (
          <div className="grid grid-cols-4 gap-2 max-[599px]:grid-cols-2">
            <Stat label="Tasks imported" value={item.progress.done} />
            <Stat label="Comments" value={report.comments_imported} />
            <Stat label="Files" value={report.files_imported} />
            <Stat label="Failed" value={item.progress.failed} />
          </div>
        ) : null}
        {item.project_ids.length > 0 ? (
          <section className="flex flex-col gap-2" aria-label="Imported projects">
            <h3 className="text-xs font-medium text-muted-foreground">Imported projects</h3>
            <ul className="flex flex-col border-t border-border">
              {item.project_ids.map((id) => {
                const project = names.get(id)
                return (
                  <li key={id} className="border-b border-border">
                    <Link
                      to={`/tasks?project=${id}`}
                      className="flex h-9 items-center gap-2 px-1 text-[13px] text-foreground transition-colors duration-150 ease-out hover:bg-sidebar-accent/50"
                    >
                      <span className="mx-[3px] size-2 shrink-0 rounded-[2px]" style={{ background: project?.color }} />
                      <span className="truncate">{project?.name ?? 'Imported project'}</span>
                    </Link>
                  </li>
                )
              })}
            </ul>
          </section>
        ) : null}
        {notes.length > 0 ? (
          <section className="flex flex-col gap-2" aria-label="Import notes">
            <h3 className="text-xs font-medium text-muted-foreground">Notes</h3>
            {notes.map((note) => (
              <p key={note} className="text-sm text-foreground">
                {note}
              </p>
            ))}
          </section>
        ) : null}
        {report && report.failures.length > 0 ? (
          <section className="flex flex-col gap-2" aria-label="Failed items">
            <h3 className="text-xs font-medium text-muted-foreground">Not imported</h3>
            <ul className="flex flex-col border-t border-border">
              {report.failures.map((failure) => (
                <li key={`${failure.kind}-${failure.asana_gid}`} className="flex flex-col gap-0.5 border-b border-border px-1 py-2">
                  <span className="text-[13px] text-foreground">
                    {failure.title.trim() ? failure.title : 'Untitled'}
                    {failure.kind === 'project' ? <span className="text-muted-foreground"> (project)</span> : null}
                  </span>
                  <span className="text-xs text-muted-foreground">{failure.error}</span>
                </li>
              ))}
            </ul>
          </section>
        ) : null}
        <div>
          <Link className={buttonVariants({ variant: 'outline' })} to="/tasks-import">
            <Import className="size-4" />
            Import more
          </Link>
        </div>
      </Body>
    </>
  )
}

/**
 * `/tasks-import` (connect + recent imports) and `/tasks-import/:importId` (one import through its states), a pane
 * in the Tasks area like the task trash. The URL survives reloads during a long import.
 */
export function AsanaImportPage() {
  const { workspace } = useWorkspace()
  const { importId } = useParams()
  const query = useAsanaImport(workspace.id, importId)
  let content: ReactNode
  if (!importId) {
    content = <ConnectStep workspaceId={workspace.id} />
  } else if (query.data) {
    const item = query.data
    if (item.status === 'scanning') content = <ScanningStep workspaceId={workspace.id} item={item} />
    else if (item.status === 'ready') content = <ChooseStep key={item.id} workspaceId={workspace.id} item={item} />
    else if (item.status === 'queued' || item.status === 'importing') content = <ImportingStep workspaceId={workspace.id} item={item} />
    else if (isFinalImport(item.status)) content = <ResultStep workspaceId={workspace.id} item={item} />
  } else if (query.isError) {
    const missing = query.error instanceof ApiProblem && query.error.status === 404
    content = (
      <>
        <ImportHeader title="Import from Asana" />
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
                <Link className={buttonVariants({ variant: 'outline' })} to="/tasks-import">
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
        <ImportHeader title="Import from Asana" />
        <div className="flex flex-1 items-center justify-center">
          <Spinner className="text-muted-foreground" />
        </div>
      </>
    )
  }
  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane>{content}</Pane>
    </div>
  )
}
