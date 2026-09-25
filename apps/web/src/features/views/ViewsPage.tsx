import { useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { Layer, Menu, Add as Plus } from 'reicon-react'
import { EmptyState } from '@/components/common/EmptyState'
import { UserAvatar } from '@/components/common/UserAvatar'
import { Button } from '@/components/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import type { User } from '@/features/workspaces/models'
import { useMembers } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useSavedViews, type SavedView } from './api/views'
import { FavoriteStar } from './components/FavoriteStar'
import { PRESS_MOTION } from './components/motion'
import { SaveViewDialog } from './components/SaveViewDialog'
import { ViewActionsMenu } from './components/ViewActionsMenu'
import { ViewIcon } from './components/ViewIcon'
import { rebaseViewSessionEdit } from './useViewState'
import { viewPath } from './viewActions'
import { defaultViewState } from './viewState'

type Tab = 'mine' | 'workspace'
type RowDialog = { mode: 'edit' | 'duplicate'; view: SavedView }

const TAB_TRIGGER =
  'h-7 flex-none gap-1 rounded-md border-0 px-2.5 py-0 text-[13px] font-medium text-muted-foreground after:hidden hover:bg-accent hover:text-foreground dark:text-muted-foreground data-active:bg-primary/10! data-active:text-primary! data-active:ring-1 data-active:ring-primary/25 data-active:ring-inset'

/** `/views`: every saved view the user can see, split into their own and the workspace's shared ones. */
export function ViewsPage() {
  const { workspace } = useWorkspace()
  const navigate = useNavigate()
  const viewsQuery = useSavedViews(workspace.id)
  const members = useMembers(workspace.id).data ?? []
  const [tab, setTab] = useState<Tab>('mine')
  const [dialog, setDialog] = useState<RowDialog | null>(null)
  const views = Array.isArray(viewsQuery.data) ? viewsQuery.data : []
  const newView = () => navigate('/tasks?save_view=1')

  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col bg-background">
      <Tabs value={tab} onValueChange={(value) => setTab(value as Tab)} className="min-h-0 flex-1 gap-0">
        {/* the shell's top bar is hidden on /views, so this header carries the mobile menu button */}
        <div className="flex h-12 shrink-0 items-center gap-3 border-b border-border px-3 max-[899px]:h-11 max-[899px]:gap-2 max-[899px]:px-2">
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="hidden shrink-0 text-muted-foreground/70 max-[899px]:inline-flex"
            aria-label="Menu"
            onClick={() => window.dispatchEvent(new CustomEvent('open-sidebar'))}
          >
            <Menu className="size-[18px]" />
          </Button>
          <h1 className="shrink-0 text-[13px] font-semibold text-foreground">Views</h1>
          <TabsList variant="line" className="min-w-0 justify-start gap-1 overflow-x-auto p-0 [scrollbar-width:none]">
            <TabsTrigger value="mine" className={TAB_TRIGGER}>My views</TabsTrigger>
            <TabsTrigger value="workspace" className={TAB_TRIGGER}>Workspace views</TabsTrigger>
          </TabsList>
          <div className="flex-1" />
          <Button type="button" className={PRESS_MOTION} aria-label="New view" onClick={newView}>
            <Plus className="size-4" />
            <span className="max-[899px]:hidden">New view</span>
          </Button>
        </div>
        {(['mine', 'workspace'] as const).map((key) => (
          <TabsContent key={key} value={key} className="flex min-h-0 flex-1 flex-col overflow-y-auto">
            {viewsQuery.isPending ? (
              <p className="p-4 text-[13px] text-muted-foreground">Loading views…</p>
            ) : viewsQuery.isError ? (
              <div className="flex flex-1 flex-col p-2 *:flex-1">
                <EmptyState icon={Layer} title="Views unavailable" description="The server could not load views." />
              </div>
            ) : (
              <ViewList
                views={views.filter((item) => (key === 'mine' ? item.visibility === 'personal' : item.visibility === 'workspace'))}
                members={members}
                workspaceId={workspace.id}
                onNew={newView}
                onEdit={(item) => setDialog({ mode: 'edit', view: item })}
                onDuplicate={(item) => setDialog({ mode: 'duplicate', view: item })}
              />
            )}
          </TabsContent>
        ))}
      </Tabs>
      <SaveViewDialog
        open={dialog !== null}
        onOpenChange={(open) => {
          if (!open) setDialog(null)
        }}
        mode={dialog?.mode ?? 'edit'}
        workspaceId={workspace.id}
        state={dialog?.view.state ?? defaultViewState()}
        view={dialog?.view}
        onSaved={(saved) => {
          // our own rename moved the version: unsaved edits on the view now build on it (as on the view's page)
          if (dialog?.mode === 'edit') rebaseViewSessionEdit(workspace.id, saved.id, dialog.view.version, saved.version)
          // a duplicate or a visibility change can land on the other tab; follow it there
          setTab(saved.visibility === 'workspace' ? 'workspace' : 'mine')
        }}
      />
    </section>
  )
}

interface ViewListProps {
  views: SavedView[]
  members: User[]
  workspaceId: string
  onNew: () => void
  onEdit: (view: SavedView) => void
  onDuplicate: (view: SavedView) => void
}

function ViewList({ views, members, workspaceId, onNew, onEdit, onDuplicate }: ViewListProps) {
  if (views.length === 0) {
    return (
      <div className="flex flex-1 flex-col p-2 *:flex-1">
        <EmptyState
          icon={Layer}
          title="No views yet"
          description="Save a filter from any task page to create a view"
          action={
            <Button type="button" className={PRESS_MOTION} onClick={onNew}>
              <Plus className="size-4" />
              New view
            </Button>
          }
        />
      </div>
    )
  }
  return (
    <ul aria-label="Views" className="flex flex-col">
      {views.map((item) => (
        <ViewRow
          key={item.id}
          view={item}
          owner={members.find((person) => person.id === item.owner.user_id)}
          workspaceId={workspaceId}
          onEdit={onEdit}
          onDuplicate={onDuplicate}
        />
      ))}
    </ul>
  )
}

interface ViewRowProps {
  view: SavedView
  owner: User | undefined
  workspaceId: string
  onEdit: (view: SavedView) => void
  onDuplicate: (view: SavedView) => void
}

/** One flat 44px row; the name's stretched `after:` overlay makes the whole row the link. */
function ViewRow({ view, owner, workspaceId, onEdit, onDuplicate }: ViewRowProps) {
  return (
    <li className="relative flex h-11 items-center gap-3 border-b border-border px-3 transition-colors duration-150 ease-out hover-fine:hover:bg-foreground/[0.02] has-[a:focus-visible]:bg-muted/40 has-[a:focus-visible]:ring-2 has-[a:focus-visible]:ring-ring/50 has-[a:focus-visible]:ring-inset max-[899px]:gap-2 max-[899px]:px-2">
      <ViewIcon icon={view.icon} color={view.color} />
      <Link
        to={viewPath(view.id)}
        className="max-w-[40%] min-w-0 shrink-0 truncate text-[13px] font-medium text-foreground outline-none after:absolute after:inset-0 max-[640px]:max-w-none max-[640px]:shrink"
      >
        {view.name}
      </Link>
      <span className="min-w-0 flex-1 truncate text-[13px] text-muted-foreground max-[640px]:hidden">{view.description}</span>
      <span className="relative z-10 ml-auto flex shrink-0 items-center" title={view.owner.display_name}>
        <UserAvatar user={owner} name={view.owner.display_name} size={20} />
      </span>
      <FavoriteStar workspaceId={workspaceId} view={view} named className="relative z-10" />
      <ViewActionsMenu
        workspaceId={workspaceId}
        view={view}
        onEdit={() => onEdit(view)}
        onDuplicate={() => onDuplicate(view)}
        named
        align="end"
        className="relative z-10"
      />
    </li>
  )
}
