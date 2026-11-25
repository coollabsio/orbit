import { toast } from 'sonner'
import { useState, type FormEvent } from 'react'
import { Link } from 'react-router'
import { Global } from 'reicon-react'
import { isSlug, slugOf } from '@orbit/api/customPages'
import type { CustomPage } from '@orbit/api/generated/types.gen'
import { confirmAction } from '@/components/common/confirmAction'
import { InfoTip } from '@/components/common/InfoTip'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { LoadingText } from '@/components/ui/spinner'
import { Switch } from '@/components/ui/switch'
import { useCan } from '@/features/workspaces/permissions'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { RequiredMark, RowIcon, SettingsRow } from '@/features/settings/components/SettingsParts'
import { customPageError, useCustomPages, useDeleteCustomPage, useSaveCustomPage } from '@/features/pages/api'

export function CustomPagesPage() {
  const { workspace } = useWorkspace()
  const canManage = useCan('workspace.update')
  const pages = useCustomPages(workspace.id, canManage)
  const deletePage = useDeleteCustomPage(workspace.id)
  const [editing, setEditing] = useState<CustomPage | 'new'>()
  const policy = pages.data?.policy

  if (!canManage) return <SettingsCard title="Custom pages"><p className="m-0 text-muted-foreground">Only workspace owners and administrators can manage custom pages.</p></SettingsCard>

  return <>
    <SettingsCard
      title="Custom pages"
      description="Show your own dashboards and internal tools in the sidebar. Each page opens in a sandboxed frame. The site must allow Orbit in its frame-ancestors."
      actions={<Button type="button" disabled={!policy || policy.mode === 'off' || (policy.mode === 'listed' && policy.origins.length === 0)} onClick={() => setEditing('new')}>Add page</Button>}
    >
      {pages.isPending ? <LoadingText>Loading…</LoadingText> : policy?.mode === 'off' ? (
        <p className="m-0 text-muted-foreground">Custom pages are off on this Orbit. Its administrator turns them on in Admin → General.</p>
      ) : policy?.mode === 'listed' && policy.origins.length === 0 ? (
        <p className="m-0 text-muted-foreground">No site is allowed yet. The administrator of this Orbit adds sites in Admin → General → Custom pages, for example <code>https://grafana.example.com</code>.</p>
      ) : policy?.mode === 'listed' ? (
        <p className="m-0 text-muted-foreground">Allowed origins: {policy.origins.map((origin, index) => <span key={origin}>{index ? ', ' : null}<code>{origin}</code></span>)}</p>
      ) : (
        <p className="m-0 text-muted-foreground">Any https site can be a page.</p>
      )}
    </SettingsCard>
    <SettingsCard title="Pages" flush>
      <div className="flex flex-col divide-y">
        {pages.isError ? <SettingsRow role="alert">Custom pages could not be loaded.</SettingsRow> : null}
        {pages.data?.items.length === 0 ? <SettingsRow>No custom pages yet.</SettingsRow> : null}
        {pages.data?.items.map((page) => <SettingsRow key={page.id} className="flex-nowrap">
          <RowIcon>{page.icon ? <span aria-hidden="true" className="text-base">{page.icon}</span> : <Global />}</RowIcon>
          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="flex items-center gap-2 text-[13px] font-medium text-foreground">
              <Link to={`/pages/${page.slug}`} className="hover:underline">{page.name}</Link>
              {page.admins_only ? <Badge variant="secondary">Admins only</Badge> : null}
              {pages.data?.blocked.includes(page.id) ? <Badge variant="destructive">Origin not allowed</Badge> : null}
            </span>
            <span className="truncate text-xs text-muted-foreground/70">/pages/{page.slug} · {page.url}</span>
          </div>
          <span className="flex gap-1">
            <Button variant="ghost" type="button" onClick={() => setEditing(page)}>Edit</Button>
            <Button variant="ghost" type="button" disabled={deletePage.isPending} onClick={async () => {
              if (await confirmAction({ title: `Remove ${page.name}?`, description: 'The page leaves the sidebar for everyone. Links to it stop working.', confirmLabel: 'Remove', danger: true })) {
                deletePage.mutate(page, { onError: () => toast.error('The page could not be removed.') })
              }
            }}>Remove</Button>
          </span>
        </SettingsRow>)}
      </div>
    </SettingsCard>
    {editing ? <CustomPageDialog workspaceId={workspace.id} page={editing === 'new' ? undefined : editing} onClose={() => setEditing(undefined)} /> : null}
  </>
}

function CustomPageDialog({ workspaceId, page, onClose }: { workspaceId: string; page?: CustomPage; onClose: () => void }) {
  const save = useSaveCustomPage(workspaceId)
  const [name, setName] = useState(page?.name ?? '')
  const [icon, setIcon] = useState(page?.icon ?? '')
  const [url, setUrl] = useState(page?.url ?? '')
  // the address follows the name until someone edits it
  const [slug, setSlug] = useState(page?.slug)
  const [adminsOnly, setAdminsOnly] = useState(page?.admins_only ?? false)
  const [error, setError] = useState<string>()
  const address = slug ?? slugOf(name)
  const ready = name.trim() !== '' && url.trim() !== '' && isSlug(address)

  async function submit(event: FormEvent) {
    event.preventDefault()
    if (!ready) return
    setError(undefined)
    try {
      await save.mutateAsync({ page, body: { name: name.trim(), slug: address, url: url.trim(), icon: icon.trim() || null, admins_only: adminsOnly } })
      onClose()
    } catch (caught) {
      setError(customPageError(caught))
    }
  }

  return <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
    <DialogContent className="sm:max-w-lg">
      <DialogHeader><DialogTitle>{page ? `Edit ${page.name}` : 'Add page'}</DialogTitle></DialogHeader>
      <form id="custom-page-form" className="grid gap-4" onSubmit={(event) => void submit(event)}>
        <div className="grid grid-cols-[1fr_5rem] gap-3">
          <Field><FieldLabel htmlFor="custom-page-name">Name <RequiredMark /></FieldLabel><Input id="custom-page-name" value={name} maxLength={60} onChange={(event) => setName(event.target.value)} placeholder="Deploys" /></Field>
          <Field><FieldLabel htmlFor="custom-page-icon">Icon</FieldLabel><Input id="custom-page-icon" value={icon} maxLength={8} onChange={(event) => setIcon(event.target.value)} placeholder="🚀" /></Field>
        </div>
        <Field>
          <FieldLabel htmlFor="custom-page-url">URL <RequiredMark /> <InfoTip text="An https URL that this Orbit allows. A path or query after the page's address in Orbit is added to it." /></FieldLabel>
          <Input id="custom-page-url" type="url" value={url} maxLength={2048} onChange={(event) => setUrl(event.target.value)} placeholder="https://grafana.example.com/d/deploys" />
        </Field>
        <Field>
          <FieldLabel htmlFor="custom-page-slug">Address <RequiredMark /> <InfoTip text="The page opens at /pages/<address>. Lowercase letters, digits and hyphens." /></FieldLabel>
          <Input id="custom-page-slug" value={address} maxLength={48} aria-invalid={address !== '' && !isSlug(address)} onChange={(event) => setSlug(event.target.value.toLowerCase())} placeholder="deploys" />
        </Field>
        <Label className="flex items-center gap-2"><Switch checked={adminsOnly} onCheckedChange={setAdminsOnly} /> Only owners and admins see it</Label>
        {error ? <p className="m-0 text-destructive" role="alert">{error}</p> : null}
      </form>
      <DialogFooter>
        <Button variant="outline" type="button" onClick={onClose}>Cancel</Button>
        <Button type="submit" form="custom-page-form" disabled={save.isPending || !ready}>{page ? 'Save' : 'Add page'}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
}
