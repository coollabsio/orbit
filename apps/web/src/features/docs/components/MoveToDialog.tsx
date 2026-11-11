// "Move to": search and pick where a page goes (a space, a page, or a database, where it becomes a row).
// Same Command list as PageLinkDialog; filtering stays ours (`shouldFilter={false}`).
import { useMemo, useState } from 'react'
import { DocumentText as FileText, Grid, Lock, People as Users } from 'reicon-react'
import type { PageSummary, Teamspace } from '@orbit/api/generated/types.gen'
import { moveDestinations } from '@orbit/database/moves'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Emoji } from '@/components/common/Emoji'
import { Modal } from '@/components/common/Modal'
import { PRIVATE_SPACE, ancestorsOf, pageTitle, spaceLabel, teamspaceSpace, type SpaceKey } from '@/features/docs/pageTree'
import type { MoveDestination } from '@/features/docs/useMovePageTo'

export function MoveToDialog({
  pages,
  teamspaces,
  moving,
  onPick,
  onClose,
}: {
  pages: PageSummary[]
  teamspaces: readonly Teamspace[]
  /** The page that moves (a row is not in `pages`). */
  moving: Pick<PageSummary, 'id' | 'parent_id' | 'kind'>
  onPick: (destination: MoveDestination) => void
  onClose: () => void
}) {
  const [query, setQuery] = useState('')
  const q = query.trim().toLowerCase()

  const spaces = useMemo(() => {
    const all: SpaceKey[] = [...teamspaces.map((teamspace) => teamspaceSpace(teamspace.id)), PRIVATE_SPACE]
    return all.filter((space) => !q || spaceLabel(space, teamspaces).toLowerCase().includes(q))
  }, [teamspaces, q])
  const targets = useMemo(
    () => moveDestinations(pages, moving).filter((page) => !q || pageTitle(page).toLowerCase().includes(q)),
    [pages, moving, q],
  )

  return (
    <Modal title="Move to" onClose={onClose}>
      <Command shouldFilter={false} label="Move to" className="gap-2 bg-transparent p-0">
        <CommandInput autoFocus placeholder="Move to a space, page or database…" value={query} onValueChange={setQuery} />
        <CommandList className="max-h-[360px]">
          <CommandEmpty className="px-3 py-7 text-[0.8125rem] text-muted-foreground/70">No pages found</CommandEmpty>
          {spaces.length > 0 ? (
            <CommandGroup heading="Spaces">
              {spaces.map((space) => (
                <CommandItem
                  key={space}
                  value={space}
                  className="gap-[9px] rounded-[7px] px-2.5 py-2 text-foreground"
                  onSelect={() => onPick({ kind: 'space', space })}
                >
                  {space === PRIVATE_SPACE ? <Lock className="size-4" /> : <Users className="size-4" />}
                  <span className="min-w-0 flex-1 truncate">{spaceLabel(space, teamspaces)}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          ) : null}
          {targets.length > 0 ? (
            <CommandGroup heading="Pages and databases">
              {targets.map((page) => {
                const parent = ancestorsOf(pages, page.id).at(-1)
                return (
                  <CommandItem
                    key={page.id}
                    value={page.id}
                    className="gap-[9px] rounded-[7px] px-2.5 py-2 text-foreground"
                    onSelect={() => onPick({ kind: 'page', page })}
                  >
                    {page.icon ? <Emoji value={page.icon} /> : page.kind === 'database' ? <Grid className="size-4" /> : <FileText className="size-4" />}
                    <span className="flex min-w-0 flex-1 flex-col">
                      <span className="truncate">{pageTitle(page)}</span>
                      {parent ? <span className="truncate text-[0.6875rem] text-muted-foreground/70">{pageTitle(parent)}</span> : null}
                    </span>
                    {page.kind === 'database' ? <span className="shrink-0 text-[0.6875rem] text-muted-foreground/70">As a row</span> : null}
                  </CommandItem>
                )
              })}
            </CommandGroup>
          ) : null}
        </CommandList>
      </Command>
    </Modal>
  )
}
