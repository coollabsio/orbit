import { useEffect, useId, useRef, useState } from 'react'
import { Plus, ChevronDown } from 'lucide-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Separator } from '@/components/ui/separator'
import { useCreateWorkspace } from '@/features/workspaces/api'
import { useWorkspace } from '@/features/workspaces/workspaceContext'

const optionClass =
  'h-auto min-h-8 w-full justify-start gap-2 rounded-md border-0 px-2 py-1.5 text-left font-normal text-foreground hover:bg-accent hover:text-accent-foreground active:not-aria-[haspopup]:translate-y-0 disabled:pointer-events-auto disabled:cursor-not-allowed disabled:opacity-40 dark:hover:bg-accent max-[899px]:min-h-11 max-[899px]:whitespace-normal max-[899px]:[overflow-wrap:anywhere]'

/** The panel is a Popover rather than a DropdownMenu: picking a workspace swaps the
    panel for a "create workspace" form, and a menu's typeahead would swallow typing. */
export function WorkspaceSwitcher({ collapsed = false, onSelect }: { collapsed?: boolean; onSelect?: () => void }) {
  const { workspace } = useWorkspace()
  const createWorkspace = useCreateWorkspace()
  const [open, setOpen] = useState(false)

  return (
    <Popover open={open} onOpenChange={setOpen} modal={false}>
      <PopoverTrigger
        render={
          <Button variant="ghost" className="h-auto min-w-0 items-baseline justify-start gap-[7px] rounded-none border-0 p-0 hover:bg-transparent aria-expanded:bg-transparent dark:hover:bg-transparent" aria-label={`Workspace: ${workspace.name}`} title={workspace.name} />
        }
      >
        <span className="truncate text-[17px] font-bold tracking-[-0.02em] text-foreground">{collapsed ? workspace.name.charAt(0) : workspace.name}</span>
        {!collapsed ? <ChevronDown className="size-[13px] shrink-0 text-muted-foreground/70" /> : null}
      </PopoverTrigger>
      {open ? (
        <PopoverContent align="start" className="max-h-(--available-height) w-auto min-w-[13rem] overflow-y-auto p-1">
          <WorkspaceMenu createWorkspace={createWorkspace} onSelect={() => { setOpen(false); onSelect?.() }} />
        </PopoverContent>
      ) : null}
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
      <Label className="mb-1.5 h-4 gap-1 text-[13px] leading-4 font-medium text-muted-foreground" htmlFor={inputId}>New workspace</Label>
      <Input id={inputId} autoFocus required value={name} disabled={createWorkspace.isPending} onChange={(event) => setName(event.target.value)} />
      {createWorkspace.isError ? <p role="alert" className="text-xs text-destructive">Workspace creation failed. Try again.</p> : null}
      <Button type="submit" disabled={!name.trim() || createWorkspace.isPending}>{createWorkspace.isPending ? 'Creating…' : 'Create workspace'}</Button>
      <Button type="button" variant="ghost" disabled={createWorkspace.isPending} onClick={() => { setCreating(false); setName(''); createWorkspace.reset() }}>Cancel</Button>
    </form>
  )

  return <>
    {workspaces.map((item) => (
      <Button
        key={item.id}
        variant="ghost"
        className={cn(optionClass, item.id === workspace.id && 'bg-accent font-medium')}
        data-selected={item.id === workspace.id || undefined}
        aria-current={item.id === workspace.id ? 'true' : undefined}
        onClick={() => { selectWorkspace(item.id); onSelect() }}
      >
        {item.name}
      </Button>
    ))}
    <Separator className="my-1" />
    <Button variant="ghost" className={optionClass} disabled={createWorkspace.isPending} onClick={() => { createWorkspace.reset(); setCreating(true) }}><Plus className="size-[15px] shrink-0" />Create workspace</Button>
  </>
}
