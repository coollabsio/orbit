import { droppedValuesText, moveDestinations, type MovablePage } from '@orbit/database/moves'
import { useMemo, useState } from 'react'
import { Platform } from 'react-native'

import { type MoveDestination, pageTitle, spaceLabel } from '../../data/docs'
import { useMovePageTo } from '../../hooks/docs'
import { usePages, useTeamspaces } from '../../queries'
import { alpha, useTheme } from '../../theme'
import { Icon, ListRow, Text } from '../../ui'
import type { ConfirmRequest } from '../settings/Dialog'
import { Sheet, SheetList, SheetNote, SheetSearch } from '../tasks/Sheet'

/**
 * "Move to" on a screen: `open(page)` shows the sheet; a pick moves the page, after the screen's confirmation when the
 * move deletes property values (a row leaving its database).
 */
export function useMoveSheet(confirm: (request: ConfirmRequest) => void, failed: (what: string) => (error: unknown) => void) {
  const moveTo = useMovePageTo()
  const [moving, setMoving] = useState<{ page: MovablePage; opened: number } | null>(null)
  const [visible, setVisible] = useState(false)
  const pick = (destination: MoveDestination) => {
    if (!moving) return
    setVisible(false)
    moveTo.prepare(moving.page.id, destination).then(({ dropped, move }) => {
      const run = () => void move().catch(failed('Could not move the page.'))
      if (dropped === 0) return run()
      confirm({ title: 'Move this row out of its database?', description: droppedValuesText(dropped), confirmLabel: 'Move', danger: true, onConfirm: run })
    }, failed('Could not move the page.'))
  }
  const open = (page: MovablePage) => {
    setMoving((current) => ({ page, opened: (current?.opened ?? 0) + 1 }))
    setVisible(true)
  }
  return { open, sheet: <MoveSheet visible={visible} moving={moving?.page ?? null} opened={moving?.opened ?? 0} onPick={pick} onClose={() => setVisible(false)} /> }
}

/**
 * The web's `MoveToDialog` as a sheet: search, then a space (the end of its root pages) or a page or database (its
 * last sub-page, or a row of the database).
 */
function MoveSheet({ visible, moving, opened, onPick, onClose }: { visible: boolean; moving: MovablePage | null; opened: number; onPick: (destination: MoveDestination) => void; onClose: () => void }) {
  return (
    <Sheet visible={visible} title="Move to" onClose={onClose}>
      {moving ? <MoveChoices key={opened} moving={moving} onPick={onPick} /> : null}
    </Sheet>
  )
}

function MoveChoices({ moving, onPick }: { moving: MovablePage; onPick: (destination: MoveDestination) => void }) {
  const { colors } = useTheme()
  const pages = usePages()
  const teamspaces = useTeamspaces()
  const [query, setQuery] = useState('')
  const q = query.trim().toLowerCase()
  const muted = alpha(colors.mutedForeground, 0.7)

  const spaces = useMemo(() => {
    const all = [...[...(teamspaces.data ?? [])].sort((a, b) => a.position - b.position).map((teamspace) => `teamspace:${teamspace.id}`), 'private']
    return all.filter((space) => !q || spaceLabel(space, teamspaces.data).toLowerCase().includes(q))
  }, [teamspaces.data, q])
  const targets = useMemo(() => moveDestinations(pages.data ?? [], moving).filter((page) => !q || pageTitle(page).toLowerCase().includes(q)), [pages.data, moving, q])
  const parentTitle = (parentId: string | null) => {
    const parent = parentId ? pages.data?.find((page) => page.id === parentId) : undefined
    return parent ? pageTitle(parent) : undefined
  }

  return (
    <>
      <SheetSearch value={query} onChange={setQuery} placeholder="Move to a space, page or database…" autoFocus={Platform.OS !== 'web'} />
      <SheetList>
        {spaces.length > 0 ? <GroupLabel label="Spaces" /> : null}
        {spaces.map((space) => (
          <ListRow
            key={space}
            title={spaceLabel(space, teamspaces.data)}
            leading={<Icon name={space === 'private' ? 'lock' : 'people'} size={18} color={muted} />}
            onPress={() => onPick({ kind: 'space', space })}
          />
        ))}
        {targets.length > 0 ? <GroupLabel label="Pages and databases" /> : null}
        {targets.map((page) => (
          <ListRow
            key={page.id}
            title={pageTitle(page)}
            subtitle={parentTitle(page.parent_id)}
            leading={page.icon ? <Text className="w-[18px] text-center text-[16px]">{page.icon}</Text> : <Icon name={page.kind === 'database' ? 'grid' : 'docs'} size={18} color={muted} />}
            trailing={page.kind === 'database' ? <Text className="text-[12px] text-muted-foreground/70">As a row</Text> : undefined}
            onPress={() => onPick({ kind: 'page', page })}
          />
        ))}
        {spaces.length === 0 && targets.length === 0 ? <SheetNote>{pages.isPending ? 'Loading…' : 'No pages found'}</SheetNote> : null}
      </SheetList>
    </>
  )
}

function GroupLabel({ label }: { label: string }) {
  return <Text className="px-4 pt-2 pb-1 font-noto-medium text-[12px] text-muted-foreground">{label}</Text>
}
