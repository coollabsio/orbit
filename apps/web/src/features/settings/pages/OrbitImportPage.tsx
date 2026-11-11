import { useRef, useState, type FormEvent, type ReactNode } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { toast } from 'sonner'
import { useQueryClient } from '@tanstack/react-query'
import { Copy, DocumentUpload, Import, TickCircle, Warning } from 'reicon-react'
import type { OrbitImport, OrbitImportTree } from '@orbit/api/generated/types.gen'
import { CHANNEL_GROUPS, fullSelection, orbitImportName, orbitImportPhaseText } from '@orbit/api/orbitImports'
import { ApiProblem } from '@orbit/api/problem'
import { cn } from 'cn'
import { SettingsCard } from '@/components/common/SettingsCard'
import { TitleTip } from '@/components/common/TitleTip'
import { confirmAction } from '@/components/common/confirmAction'
import { Badge } from '@/components/ui/badge'
import { Button, buttonVariants } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { PasswordInput } from '@/components/ui/password-input'
import { Spinner } from '@/components/ui/spinner'
import { relativeTime } from '@/lib/format'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { IMPORT_STATUS_LABELS, importStatusVariant, isActiveImport, isFinalImport } from '@/features/tasks/api/importStatus'
import { ImportProgressBar, ImportStat } from '@/features/tasks/components/ImportLayout'
import {
  createOrbitImportRequest,
  isOrbitImportInProgress,
  orbitImportErrorMessage,
  reconcileOrbitImport,
  uploadOrbitExport,
  useCancelOrbitImport,
  useOrbitImport,
  useOrbitImportTree,
  useOrbitImports,
  useStartOrbitImport,
} from '@/features/settings/api/orbitImports'

const ROOT = '/settings/import/orbit'
const TITLE = 'Import from Orbit'

const linkClass = 'text-primary underline underline-offset-2'

function RunningLink({ error, running }: { error: unknown; running: OrbitImport | undefined }) {
  if (!isOrbitImportInProgress(error) || !running) return null
  return (
    <Link className={linkClass} to={`${ROOT}/${running.id}`}>
      Open the running import
    </Link>
  )
}

/** Step 1: the other instance's address and a token, or an export ZIP. Also lists the member's recent imports. */
function ConnectStep({ workspaceId }: { workspaceId: string }) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const imports = useOrbitImports(workspaceId)
  const running = imports.data?.find((item) => isActiveImport(item.status))
  const [url, setUrl] = useState('')
  const [token, setToken] = useState('')
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<unknown>(null)
  const input = useRef<HTMLInputElement>(null)
  const [upload, setUpload] = useState<{ name: string; fraction: number; controller: AbortController } | null>(null)
  const [uploadError, setUploadError] = useState<unknown>(null)

  const opened = (created: OrbitImport) => {
    reconcileOrbitImport(queryClient, workspaceId, created)
    navigate(`${ROOT}/${created.id}`)
  }

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    if (!url.trim() || !token.trim() || pending) return
    setPending(true)
    setError(null)
    try {
      const created = await createOrbitImportRequest(workspaceId, url.trim(), token.trim())
      setToken('')
      opened(created)
    } catch (caught) {
      setError(caught)
      if (isOrbitImportInProgress(caught)) void imports.refetch()
    } finally {
      setPending(false)
    }
  }

  const send = async (file: File | undefined) => {
    if (input.current) input.current.value = ''
    if (!file || upload) return
    const controller = new AbortController()
    setUploadError(null)
    setUpload({ name: file.name, fraction: 0, controller })
    try {
      opened(
        await uploadOrbitExport(workspaceId, file, {
          signal: controller.signal,
          onProgress: (fraction) => setUpload((current) => (current ? { ...current, fraction } : current)),
        }),
      )
    } catch (caught) {
      if (caught instanceof DOMException && caught.name === 'AbortError') return
      setUploadError(caught)
      if (isOrbitImportInProgress(caught)) void imports.refetch()
    } finally {
      setUpload(null)
    }
  }

  return (
    <>
      <SettingsCard title={TITLE} description="Copy projects, pages and chats from another Orbit instance into this workspace. Nobody is notified.">
        <div className="flex flex-col gap-4 text-sm">
          <p className="text-muted-foreground">
            On the other instance, open <span className="font-medium text-foreground">Settings → API tokens</span> and create a token
            with <span className="font-medium text-foreground">Workspace export</span> (owners and administrators: the whole
            workspace) or <span className="font-medium text-foreground">Personal export</span> (any member: your private pages,
            channels and direct messages). Orbit keeps the token encrypted only until the export is downloaded.
          </p>
          <form className="flex flex-col gap-3" onSubmit={(event) => void submit(event)} autoComplete="off">
            <div className="grid gap-1.5">
              <Label htmlFor="orbit-url">Instance address</Label>
              <Input
                id="orbit-url"
                type="url"
                inputMode="url"
                spellCheck={false}
                autoCapitalize="off"
                placeholder="https://orbit.example.com"
                value={url}
                onChange={(event) => setUrl(event.target.value)}
              />
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="orbit-token">API token</Label>
              <PasswordInput
                id="orbit-token"
                name="orbit-token"
                autoComplete="off"
                spellCheck={false}
                autoCapitalize="off"
                value={token}
                aria-invalid={error ? true : undefined}
                onChange={(event) => setToken(event.target.value)}
              />
            </div>
            {error ? (
              <p role="alert" className="text-destructive">
                {orbitImportErrorMessage(error)} <RunningLink error={error} running={running} />
              </p>
            ) : null}
            <div>
              <Button type="submit" disabled={!url.trim() || !token.trim() || pending}>
                {pending ? <Spinner className="size-4" /> : null}
                Connect
              </Button>
            </div>
          </form>
        </div>
      </SettingsCard>
      <SettingsCard title="Or upload an export" description="Download an export ZIP on the other instance in Settings → Export and upload it here.">
        <div className="flex flex-col gap-3 text-sm">
          {upload ? (
            <div className="flex flex-col gap-2" role="status" aria-live="polite">
              <div className="flex items-center gap-2">
                <Spinner className="size-4 text-muted-foreground" />
                <span className="min-w-0 flex-1 truncate text-foreground">Uploading {upload.name}…</span>
                <span className="shrink-0 text-muted-foreground tabular-nums">{Math.round(upload.fraction * 100)}%</span>
              </div>
              <ImportProgressBar done={Math.round(upload.fraction * 100)} total={100} />
              <div>
                <Button type="button" variant="outline" size="sm" onClick={() => upload.controller.abort()}>
                  Cancel upload
                </Button>
              </div>
            </div>
          ) : (
            <div>
              <input
                ref={input}
                type="file"
                accept=".zip,application/zip"
                className="hidden"
                aria-label="Export .zip file"
                onChange={(event) => void send(event.target.files?.[0])}
              />
              <Button type="button" variant="outline" onClick={() => input.current?.click()}>
                <DocumentUpload className="size-4" />
                Choose file
              </Button>
            </div>
          )}
          {uploadError ? (
            <p role="alert" className="text-destructive">
              {orbitImportErrorMessage(uploadError)} <RunningLink error={uploadError} running={running} />
            </p>
          ) : null}
        </div>
      </SettingsCard>
      <RecentImports imports={imports.data} />
    </>
  )
}

function RecentImports({ imports }: { imports: OrbitImport[] | undefined }) {
  if (!imports?.length) return null
  return (
    <SettingsCard title="Recent imports" flush>
      <div className="flex flex-col divide-y">
        {imports.map((item) => (
          <Link
            key={item.id}
            to={`${ROOT}/${item.id}`}
            className="flex min-h-10 min-w-0 items-center gap-3 px-4 py-1.5 text-[13px] transition-colors duration-150 ease-out hover:bg-sidebar-accent/50"
          >
            <span className="min-w-0 flex-1 truncate text-foreground">{orbitImportName(item)}</span>
            {item.mode ? <span className="shrink-0 text-xs text-muted-foreground">{item.mode === 'full' ? 'Workspace' : 'Private data'}</span> : null}
            <Badge variant={importStatusVariant(item.status)}>{IMPORT_STATUS_LABELS[item.status]}</Badge>
            <span className="w-12 shrink-0 text-right text-xs text-muted-foreground" title={new Date(item.created_at).toLocaleString()}>
              {relativeTime(item.created_at)}
            </span>
          </Link>
        ))}
      </div>
    </SettingsCard>
  )
}

function useCancel(workspaceId: string, item: OrbitImport) {
  const cancel = useCancelOrbitImport(workspaceId)
  const request = async (started: boolean) => {
    const confirmed = await confirmAction({
      title: started ? 'Cancel the import?' : 'Cancel this import?',
      description: started
        ? 'What was already imported stays. The import stops before the next item.'
        : 'Orbit deletes the token and the export. You can start a new import at any time.',
      confirmLabel: 'Cancel import',
      cancelLabel: 'Keep going',
      danger: true,
    })
    if (!confirmed) return
    cancel.mutate({ importId: item.id }, { onError: (error) => toast.error(orbitImportErrorMessage(error)) })
  }
  return { request, pending: cancel.isPending }
}

/** Step 2: the other instance exports, Orbit downloads the ZIP and reads it. */
function WaitingStep({ workspaceId, item }: { workspaceId: string; item: OrbitImport }) {
  const cancel = useCancel(workspaceId, item)
  const { headline, text } = orbitImportPhaseText(item)
  return (
    <SettingsCard title={TITLE}>
      <div className="flex flex-col items-center gap-3 py-12 text-center" role="status" aria-live="polite">
        <Spinner className="size-6 text-muted-foreground" />
        <p className="text-sm font-medium text-foreground">{headline}</p>
        <p className="max-w-sm text-sm text-muted-foreground">{text}</p>
        <Button type="button" variant="outline" size="sm" disabled={cancel.pending} onClick={() => void cancel.request(false)}>
          Cancel
        </Button>
      </div>
    </SettingsCard>
  )
}

type Choice = { id: string; name: string; label?: ReactNode; count: string }

function plural(count: number, one: string, many: string) {
  return `${count} ${count === 1 ? one : many}`
}

/** One group of checkboxes with a "select all" checkbox in its header. */
function ChoiceGroup({ title, choices, selected, onChange }: { title: string; choices: Choice[]; selected: ReadonlySet<string>; onChange: (next: ReadonlySet<string>) => void }) {
  if (choices.length === 0) return null
  const chosen = choices.filter((choice) => selected.has(choice.id)).length
  const all = chosen === choices.length
  const toggle = (ids: string[], on: boolean) => {
    const next = new Set(selected)
    for (const id of ids) {
      if (on) next.add(id)
      else next.delete(id)
    }
    onChange(next)
  }
  return (
    <section className="flex flex-col gap-1" aria-label={title}>
      <div className="flex h-8 items-center gap-2 px-2">
        <Checkbox
          aria-label={`Select all ${title.toLowerCase()}`}
          checked={all}
          indeterminate={chosen > 0 && !all}
          onCheckedChange={() => toggle(choices.map((choice) => choice.id), !all)}
        />
        <h3 className="text-xs font-semibold text-foreground">{title}</h3>
      </div>
      {choices.map((choice) => (
        <div
          key={choice.id}
          className="flex h-8 items-center gap-2 rounded-md px-2 pl-8 text-[13px] text-foreground transition-colors duration-150 ease-out hover:bg-sidebar-accent/50"
        >
          <Checkbox aria-label={`Select ${choice.name}`} checked={selected.has(choice.id)} onCheckedChange={(checked) => toggle([choice.id], checked)} />
          {/* Clicking the name toggles the row, like the checkbox (the checkbox is the keyboard control). */}
          <button type="button" tabIndex={-1} className="min-w-0 flex-1 truncate text-left" onClick={() => toggle([choice.id], !selected.has(choice.id))}>
            {choice.label ?? choice.name}
          </button>
          <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{choice.count}</span>
        </div>
      ))}
    </section>
  )
}

/** Step 3: everything starts selected. A personal import has only private pages and channels. */
function Chooser({ workspaceId, item, tree }: { workspaceId: string; item: OrbitImport; tree: OrbitImportTree }) {
  const full = item.mode === 'full'
  const initial = fullSelection(item.mode, tree)
  const [projects, setProjects] = useState<ReadonlySet<string>>(() => new Set(initial.project_ids))
  const [teamspaces, setTeamspaces] = useState<ReadonlySet<string>>(() => new Set(initial.teamspace_ids))
  const [privatePages, setPrivatePages] = useState(initial.include_private_pages)
  const [channels, setChannels] = useState<ReadonlySet<string>>(() => new Set(initial.channel_ids))
  const start = useStartOrbitImport(workspaceId)
  const selection = {
    project_ids: full ? [...projects] : [],
    teamspace_ids: full ? [...teamspaces] : [],
    include_private_pages: privatePages,
    channel_ids: [...channels],
  }
  const empty = !selection.project_ids.length && !selection.teamspace_ids.length && !privatePages && !selection.channel_ids.length
  const nothing = (full ? tree.projects.length + tree.teamspaces.length : 0) + tree.private_page_count + tree.channels.length === 0

  return (
    <div className="flex flex-col">
      <div className="flex flex-col gap-3 p-2">
        {nothing ? <p className="p-2 text-sm text-muted-foreground">This export has nothing to import.</p> : null}
        {full ? (
          <>
            <ChoiceGroup
              title="Projects"
              selected={projects}
              onChange={setProjects}
              choices={tree.projects.map((project) => ({
                id: project.id,
                name: project.name,
                label: (
                  <>
                    {project.name}
                    {project.key ? <span className="text-muted-foreground"> · {project.key}</span> : null}
                  </>
                ),
                count: plural(project.task_count, 'task', 'tasks'),
              }))}
            />
            <ChoiceGroup
              title="Teamspaces"
              selected={teamspaces}
              onChange={setTeamspaces}
              choices={tree.teamspaces.map((teamspace) => ({ id: teamspace.id, name: teamspace.name, count: plural(teamspace.page_count, 'page', 'pages') }))}
            />
          </>
        ) : null}
        {tree.private_page_count > 0 ? (
          <div className="flex h-8 items-center gap-2 rounded-md px-2 text-[13px] text-foreground transition-colors duration-150 ease-out hover:bg-sidebar-accent/50">
            <Checkbox aria-label="Import my private pages" checked={privatePages} onCheckedChange={setPrivatePages} />
            <button type="button" tabIndex={-1} className="min-w-0 flex-1 truncate text-left text-xs font-semibold" onClick={() => setPrivatePages(!privatePages)}>
              My private pages
            </button>
            <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{plural(tree.private_page_count, 'page', 'pages')}</span>
          </div>
        ) : null}
        {CHANNEL_GROUPS.map((group) => (
          <ChoiceGroup
            key={group.kind}
            title={group.label}
            selected={channels}
            onChange={setChannels}
            choices={tree.channels
              .filter((channel) => channel.kind === group.kind)
              .map((channel) => ({
                id: channel.id,
                name: group.kind === 'dm' ? channel.name : `#${channel.name}`,
                count: plural(channel.message_count, 'message', 'messages'),
              }))}
          />
        ))}
      </div>
      <div className="flex flex-wrap items-center gap-3 border-t border-border px-4 py-3">
        <p className="min-w-0 flex-1 text-xs text-muted-foreground">
          {full
            ? 'Projects keep their tasks, comments and files; teamspaces keep their pages. Items imported before are skipped. Nobody is notified.'
            : 'Your private pages stay private, and channels keep their messages and files. Items imported before are skipped. Nobody is notified.'}
        </p>
        <div className="flex flex-col items-end gap-1.5">
          {start.isError ? (
            <p role="alert" className="max-w-96 text-right text-xs text-destructive">
              {orbitImportErrorMessage(start.error)}
            </p>
          ) : null}
          <Button type="button" disabled={empty || start.isPending} onClick={() => start.mutate({ importId: item.id, selection })}>
            Import
          </Button>
        </div>
      </div>
    </div>
  )
}

function ChooseStep({ workspaceId, item }: { workspaceId: string; item: OrbitImport }) {
  const cancel = useCancel(workspaceId, item)
  const scan = useOrbitImportTree(workspaceId, item.id)
  return (
    <SettingsCard
      title={`Choose what to import from ${orbitImportName(item)}`}
      flush
      actions={
        <Button type="button" variant="ghost" size="sm" disabled={cancel.pending} onClick={() => void cancel.request(false)}>
          Cancel
        </Button>
      }
    >
      {scan.data ? (
        <Chooser workspaceId={workspaceId} item={item} tree={scan.data} />
      ) : scan.isError || scan.isSuccess ? (
        <div className="flex flex-col items-center gap-3 p-6 text-center" role="alert">
          <p className="text-sm text-muted-foreground">{scan.isError ? orbitImportErrorMessage(scan.error) : 'The export contents are not available.'}</p>
          <Button type="button" variant="outline" size="sm" onClick={() => void scan.refetch()}>
            Retry
          </Button>
        </div>
      ) : (
        <div className="flex items-center justify-center p-10">
          <Spinner className="text-muted-foreground" />
        </div>
      )}
    </SettingsCard>
  )
}

/** Step 4: the import job runs. */
function ImportingStep({ workspaceId, item }: { workspaceId: string; item: OrbitImport }) {
  const cancel = useCancel(workspaceId, item)
  const { done, total, failed } = item.progress
  return (
    <SettingsCard title={TITLE}>
      <section className="flex flex-col gap-4" role="status" aria-live="polite">
        <div className="flex items-center gap-2">
          <Spinner className="size-4 text-muted-foreground" />
          <p className="text-sm font-medium text-foreground">{item.status === 'queued' ? 'Waiting for the import to start…' : 'Importing…'}</p>
        </div>
        <ImportProgressBar done={done + failed} total={total} />
        <div className="flex items-center gap-4 text-sm text-muted-foreground">
          <span>
            {done} of {total} items imported
          </span>
          {failed > 0 ? <span className="text-destructive">{failed} failed</span> : null}
          <span className="flex-1" />
          <Button type="button" variant="outline" size="sm" disabled={cancel.pending} onClick={() => void cancel.request(true)}>
            Cancel import
          </Button>
        </div>
        <p className="text-sm text-muted-foreground">You can leave this page; the import continues on the server.</p>
      </section>
    </SettingsCard>
  )
}

function copyImportLink() {
  void navigator.clipboard.writeText(`${window.location.origin}${ROOT}`).then(
    () => toast('Link copied'),
    () => toast.error('Could not copy the link.'),
  )
}

/** Step 5: completed, failed, cancelled or expired. */
function ResultStep({ item }: { item: OrbitImport }) {
  const report = item.report
  const headline =
    item.status === 'completed' ? 'Import complete' : item.status === 'failed' ? 'Import failed' : item.status === 'cancelled' ? 'Import cancelled' : 'Import expired'
  return (
    <SettingsCard title={TITLE}>
      <div className="flex flex-col gap-6">
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
          <div className="grid grid-cols-3 gap-2 max-[599px]:grid-cols-2">
            <ImportStat label="Projects" value={report.created.projects ?? 0} />
            <ImportStat label="Tasks" value={report.created.tasks ?? 0} />
            <ImportStat label="Pages" value={report.created.pages ?? 0} />
            <ImportStat label="Channels" value={report.created.channels ?? 0} />
            <ImportStat label="Messages" value={report.created.messages ?? 0} />
            <ImportStat label="Files" value={report.created.files ?? 0} />
          </div>
        ) : null}
        {report && report.skipped_existing > 0 ? (
          <p className="text-sm text-foreground">
            {plural(report.skipped_existing, 'item was', 'items were')} imported before and skipped.
          </p>
        ) : null}
        {report && report.unmatched_users.length > 0 ? (
          <section className="flex flex-col gap-2" aria-label="Unmatched people">
            <TitleTip
              title="People with no member here"
              text="No member here has their email, so their tasks are unassigned."
              className="text-xs font-medium text-muted-foreground"
            />
            <ul className="flex flex-col border-t border-border">
              {report.unmatched_users.map((user) => (
                <li key={user.email ?? user.name} className="flex items-center gap-3 border-b border-border px-1 py-2 text-[13px]">
                  <span className="min-w-0 flex-1 truncate text-foreground">{user.name}</span>
                  <span className="min-w-0 truncate text-muted-foreground">{user.email}</span>
                </li>
              ))}
            </ul>
          </section>
        ) : null}
        {report && report.failures.length > 0 ? (
          <section className="flex flex-col gap-2" aria-label="Failed items">
            <h3 className="text-xs font-medium text-muted-foreground">Not imported</h3>
            <ul className="flex flex-col border-t border-border">
              {report.failures.map((failure, index) => (
                <li key={`${failure.kind}-${index}`} className="flex flex-col gap-0.5 border-b border-border px-1 py-2">
                  <span className="text-[13px] text-foreground">
                    {failure.title.trim() ? failure.title : 'Untitled'}
                    <span className="text-muted-foreground"> ({failure.kind})</span>
                  </span>
                  <span className="text-xs text-muted-foreground">{failure.error}</span>
                </li>
              ))}
            </ul>
          </section>
        ) : null}
        {report?.members_must_import_private && item.status === 'completed' ? (
          <section className="flex flex-col gap-2 rounded-lg border border-border p-3" aria-label="Private data">
            <p className="text-sm text-foreground">
              Other members’ private pages and chats are not included. Each member must import them with their own token.
            </p>
            <div>
              <Button type="button" variant="outline" size="sm" onClick={copyImportLink}>
                <Copy className="size-4" />
                Copy link for members
              </Button>
            </div>
          </section>
        ) : null}
        <div>
          <Link className={buttonVariants({ variant: 'outline' })} to={ROOT}>
            <Import className="size-4" />
            Import more
          </Link>
        </div>
      </div>
    </SettingsCard>
  )
}

/**
 * `/settings/import/orbit` (connect + recent imports) and `/settings/import/orbit/:importId` (one import through its
 * states). The URL survives reloads during a long import.
 */
export function OrbitImportPage() {
  const { workspace } = useWorkspace()
  const { importId } = useParams()
  const query = useOrbitImport(workspace.id, importId)
  if (!importId) return <ConnectStep workspaceId={workspace.id} />
  const item = query.data
  if (item) {
    if (item.status === 'scanning') return <WaitingStep workspaceId={workspace.id} item={item} />
    if (item.status === 'ready') return <ChooseStep key={item.id} workspaceId={workspace.id} item={item} />
    if (item.status === 'queued' || item.status === 'importing') return <ImportingStep workspaceId={workspace.id} item={item} />
    if (isFinalImport(item.status)) return <ResultStep item={item} />
  }
  if (query.isError) {
    const missing = query.error instanceof ApiProblem && query.error.status === 404
    return (
      <SettingsCard title={missing ? 'Import not found' : 'Import unavailable'}>
        <div className="flex flex-col gap-3 text-sm">
          <p className="text-muted-foreground">{missing ? 'This import does not exist or belongs to someone else.' : 'The server could not load this import.'}</p>
          <div className="flex gap-2">
            {missing ? null : (
              <Button type="button" variant="outline" onClick={() => void query.refetch()}>
                Retry
              </Button>
            )}
            <Link className={buttonVariants({ variant: 'outline' })} to={ROOT}>
              New import
            </Link>
          </div>
        </div>
      </SettingsCard>
    )
  }
  return (
    <div className="flex items-center justify-center p-10">
      <Spinner className="text-muted-foreground" />
    </div>
  )
}
