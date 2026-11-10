import { useEffect, useId, useRef, useState, type ComponentProps } from 'react'
import { ApiProblem } from '@orbit/api/problem'
import { Add as Plus, ChevronDown, ChevronExpandY } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Separator } from '@/components/ui/separator'
import { Tip } from '@/components/common/Tip'
import { useCreateWorkspace } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

/** The panel is a Popover rather than a DropdownMenu: picking a workspace swaps the
    panel for a "create workspace" form, and a menu's typeahead would swallow typing. */
export function WorkspaceSwitcher({ collapsed = false, topbar = false }: { collapsed?: boolean; /** The phone top bar's look: initial badge, name and an up/down chevron. */ topbar?: boolean }) {
  const { workspace } = useWorkspace()
  const createWorkspace = useCreateWorkspace()
  const [open, setOpen] = useState(false)

  return (
    <Popover open={open} onOpenChange={setOpen} modal={false}>
      {topbar ? (
        <PopoverTrigger
          render={<Button variant="ghost" className="-ml-1 h-8 min-w-0 shrink justify-start gap-1.5 rounded-lg px-2" aria-label={`Workspace: ${workspace.name}`} />}
        >
          <span className="flex size-5 shrink-0 items-center justify-center rounded-md bg-muted text-[0.6875rem] font-semibold text-muted-foreground">{workspace.name.charAt(0).toUpperCase()}</span>
          <span className="truncate text-[0.8125rem] font-semibold text-foreground">{workspace.name}</span>
          <ChevronExpandY className="size-4 shrink-0 text-muted-foreground/70" />
        </PopoverTrigger>
      ) : (
      <Tip label={collapsed ? workspace.name : undefined} side="right">
        <PopoverTrigger
          render={
            <Button variant="ghost" className="h-auto min-w-0 shrink items-baseline justify-start gap-[7px] rounded-none border-0 p-0 hover:bg-transparent aria-expanded:bg-transparent dark:hover:bg-transparent" aria-label={`Workspace: ${workspace.name}`} />
          }
        >
          <span className="truncate text-[1.0625rem] font-bold tracking-[-0.02em] text-foreground">{collapsed ? workspace.name.charAt(0) : workspace.name}</span>
          {!collapsed ? <ChevronDown className="size-[13px] shrink-0 text-muted-foreground/70" /> : null}
        </PopoverTrigger>
      </Tip>
      )}
      {/* Base UI unmounts the panel after its exit, so the create form resets each time it closes. */}
      <PopoverContent align="start" className="max-h-(--available-height) w-auto min-w-[13rem] gap-0 overflow-y-auto overscroll-contain p-1">
        <WorkspaceMenu createWorkspace={createWorkspace} onSelect={() => setOpen(false)} />
      </PopoverContent>
    </Popover>
  )
}

function WorkspaceMenu({ onSelect, createWorkspace }: { onSelect: () => void; createWorkspace: ReturnType<typeof useCreateWorkspace> }) {
  const { workspace, workspaces, selectWorkspace } = useWorkspace()
  const [creating, setCreating] = useState(false)
  const [name, setName] = useState('')
  const active = useRef(true)
  useEffect(() => {
    active.current = true
    return () => { active.current = false }
  }, [])
  const inputId = useId()

  const create = async () => {
    if (!name.trim() || createWorkspace.isPending) return
    try {
      const created = await createWorkspace.mutateAsync(name.trim())
      if (!active.current) return
      selectWorkspace(created.id)
      onSelect()
    } catch {
      // Keep the draft visible so the user can retry.
    }
  }

  if (creating) return (
    <form className="flex w-60 max-w-full flex-col gap-2 p-2" onSubmit={(event) => { event.preventDefault(); void create() }}>
      <Label className="mb-1.5 h-4 gap-1 text-[0.8125rem] leading-4 font-medium text-muted-foreground" htmlFor={inputId}>New workspace</Label>
      <Input id={inputId} autoFocus required value={name} disabled={createWorkspace.isPending} onChange={(event) => setName(event.target.value)} />
      {createWorkspace.isError ? <p role="alert" className="text-xs text-destructive">{createWorkspace.error instanceof ApiProblem ? createWorkspace.error.detail : 'Workspace creation failed. Try again.'}</p> : null}
      <Button type="submit" disabled={!name.trim() || createWorkspace.isPending}>{createWorkspace.isPending ? 'Creating…' : 'Create workspace'}</Button>
      <Button type="button" variant="ghost" disabled={createWorkspace.isPending} onClick={() => { setCreating(false); setName(''); createWorkspace.reset() }}>Cancel</Button>
    </form>
  )

  return <>
    {workspaces.map((item) => (
      <WorkspaceOption
        key={item.id}
        data-selected={item.id === workspace.id || undefined}
        aria-current={item.id === workspace.id ? 'true' : undefined}
        onClick={() => { selectWorkspace(item.id); onSelect() }}
      >
        {item.name}
      </WorkspaceOption>
    ))}
    <Separator className="my-1" />
    <WorkspaceOption disabled={createWorkspace.isPending} onClick={() => { createWorkspace.reset(); setCreating(true) }}><Plus className="size-[15px]" />Create workspace</WorkspaceOption>
  </>
}

/** A full-width row in the workspace panel; long names wrap on phones instead of clipping. */
function WorkspaceOption({ className, ...props }: ComponentProps<typeof Button>) {
  return (
    <Button
      data-slot="workspace-option"
      variant="ghost"
      className={cn(
        'h-auto min-h-8 w-full justify-start px-2 py-1.5 text-left font-normal data-selected:bg-muted data-selected:font-medium dark:hover:bg-sidebar-tint/10 dark:data-selected:bg-sidebar-tint/10 max-[899px]:min-h-11 max-[899px]:whitespace-normal max-[899px]:[overflow-wrap:anywhere]',
        className,
      )}
      {...props}
    />
  )
}
