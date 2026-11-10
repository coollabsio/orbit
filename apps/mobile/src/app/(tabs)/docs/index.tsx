import type { PageKind } from '@orbit/api/generated/types.gen'
import { router } from 'expo-router'
import { type ReactNode, useMemo, useState } from 'react'
import { Pressable, ScrollView, View } from 'react-native'

import { IconDialog } from '../../../components/docs/IconDialog'
import { useMoveSheet } from '../../../components/docs/MoveSheet'
import { ActionSheet, ConfirmDialog, type ConfirmRequest, PromptDialog, type PromptRequest, type SheetAction } from '../../../components/settings/Dialog'
import { descendantsOf, docSpaces, type FavoriteRow, favoritePages, pageTitle, trashPrompt, type TreeRow } from '../../../data/docs'
import { descendants } from '../../../data/docs-api'
import * as api from '../../../data/orbit'
import type { PageSummary, Teamspace } from '../../../data/orbit'
import { isFavorite, useCreatePage, useDuplicatePage, useFavorites, useTeamspaceWrites, useToggleFavorite, useTrashPage } from '../../../hooks/docs'
import { useOrbit } from '../../../orbit-context'
import { usePages, useTeamspaces } from '../../../queries'
import { errorMessage } from '../../../session-context'
import { alpha, useTheme } from '../../../theme'
import { ErrorView, HeaderTitle, Icon, type IconName, IconButton, LoadingView, PaneHeader, PullRefresh, SafeAreaView, Text } from '../../../ui'

type Menu = { title: string; actions: SheetAction[] }

/** The web's docs sidebar at phone size: Favorites, Teamspaces with their page trees, Private, and Trash at the bottom. */
export default function DocsScreen() {
  const { colors } = useTheme()
  const { client, workspace, workspaceId } = useOrbit()
  const pages = usePages()
  const teamspaces = useTeamspaces()
  const favorites = useFavorites()
  const create = useCreatePage()
  const duplicate = useDuplicatePage()
  const trash = useTrashPage()
  const toggleFavorite = useToggleFavorite()
  const teamspaceWrites = useTeamspaceWrites()
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set())
  const [menu, setMenu] = useState<Menu | null>(null)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const [prompt, setPrompt] = useState<PromptRequest | null>(null)
  const [iconFor, setIconFor] = useState<Teamspace | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const spaces = useMemo(() => (pages.data && teamspaces.data ? docSpaces(pages.data, teamspaces.data, collapsed) : null), [collapsed, pages.data, teamspaces.data])
  const favoriteRows = useMemo(() => favoritePages(pages.data ?? [], favorites.data ?? []), [favorites.data, pages.data])
  const sortedTeamspaces = useMemo(() => [...(teamspaces.data ?? [])].sort((a, b) => a.position - b.position), [teamspaces.data])
  const refresh = () => Promise.all([pages.refetch(), teamspaces.refetch(), favorites.refetch()])
  const toggle = (key: string) =>
    setCollapsed((current) => {
      const next = new Set(current)
      if (!next.delete(key)) next.add(key)
      return next
    })
  const muted = alpha(colors.mutedForeground, 0.7)
  const failed = (what: string) => (error: unknown) => setNotice(`${what} ${errorMessage(error)}`)
  const moveSheet = useMoveSheet(setConfirm, failed)

  const newPage = (where: { space?: string | null; parentId?: string; kind?: PageKind }) => {
    setNotice(null)
    create.mutate(where, {
      onSuccess: (page) => {
        if (where.parentId) setCollapsed((current) => new Set([...current].filter((key) => key !== where.parentId)))
        router.push(`/docs/${page.id}`)
      },
      onError: failed(where.kind === 'database' ? 'Could not create the database.' : 'Could not create the page.'),
    })
  }
  /** The web tree's "+" menu: a page or a database in the same place. */
  const createMenu = (title: string, where: { space?: string | null; parentId?: string }) =>
    setMenu({
      title,
      actions: [
        { label: 'New page', icon: 'docs', onPress: () => newPage(where) },
        { label: 'New database', icon: 'grid', onPress: () => newPage({ ...where, kind: 'database' }) },
      ],
    })
  const newTeamspace = () =>
    setPrompt({ title: 'New teamspace', label: 'Name', placeholder: 'Teamspace name', confirmLabel: 'Create', onSubmit: (name) => teamspaceWrites.create.mutateAsync(name) })

  const favoriteAction = (page: Pick<PageSummary, 'id' | 'title' | 'icon' | 'kind'>): SheetAction => {
    const favorite = isFavorite(favorites.data, page.id)
    return {
      label: favorite ? 'Remove from favorites' : 'Add to favorites',
      icon: 'star',
      onPress: () => toggleFavorite.mutate({ pageId: page.id, favorite: !favorite, page }, { onError: failed('Could not change the favorites.') }),
    }
  }
  /** A favorite outside the tree (a database row or one of its sub-pages): the server has its parent, version and sub-pages. */
  const favoriteMenu = (row: FavoriteRow) => {
    if (row.page) return pageMenu(row.page)
    const confirmTrash = () =>
      Promise.all([api.page(client, workspaceId, row.id), descendants(client, workspaceId, row.id)]).then(([page, below]) =>
        setConfirm({ ...trashPrompt(page, below.length), confirmLabel: 'Move to trash', danger: true, onConfirm: () => trash.mutate(page, { onError: failed('Could not move the page to the trash.') }) }),
        failed('Could not move the page to the trash.'),
      )
    setMenu({
      title: pageTitle(row),
      actions: [
        { label: 'Open', icon: 'arrowUpRight', onPress: () => router.push(`/docs/${row.id}`) },
        favoriteAction(row),
        { label: 'Move to', icon: 'arrowRight', onPress: () => void api.page(client, workspaceId, row.id).then(moveSheet.open, failed('Could not load the page.')) },
        { label: 'Move to trash', icon: 'trash', destructive: true, onPress: () => void confirmTrash() },
      ],
    })
  }
  const pageMenu = (page: PageSummary) => {
    const hasChildren = (pages.data ?? []).some((item) => item.parent_id === page.id)
    setMenu({
      title: pageTitle(page),
      actions: [
        favoriteAction(page),
        { label: 'Duplicate', icon: 'copy', onPress: () => duplicate.mutate({ pageId: page.id, withChildren: false }, { onError: failed('Could not duplicate the page.') }) },
        ...(hasChildren ? [{ label: 'Duplicate with sub-pages', icon: 'copy' as const, onPress: () => duplicate.mutate({ pageId: page.id, withChildren: true }, { onError: failed('Could not duplicate the page.') }) }] : []),
        { label: 'Move to', icon: 'arrowRight', onPress: () => moveSheet.open(page) },
        {
          label: 'Move to trash',
          icon: 'trash',
          destructive: true,
          onPress: () => setConfirm({ ...trashPrompt(page, descendantsOf(pages.data ?? [], page.id).length), confirmLabel: 'Move to trash', danger: true, onConfirm: () => trash.mutate(page, { onError: failed('Could not move the page to the trash.') }) }),
        },
      ],
    })
  }

  const teamspaceMenu = (teamspace: Teamspace) => {
    const index = sortedTeamspaces.findIndex((item) => item.id === teamspace.id)
    const move = (position: number) => teamspaceWrites.move.mutate({ teamspace, position }, { onError: failed('Could not reorder the teamspaces.') })
    setMenu({
      title: teamspace.name,
      actions: [
        {
          label: 'Rename',
          icon: 'edit',
          onPress: () =>
            setPrompt({ title: 'Rename teamspace', label: 'Name', initial: teamspace.name, confirmLabel: 'Save', onSubmit: (name) => teamspaceWrites.save.mutateAsync({ teamspace, name }) }),
        },
        { label: 'Change icon', icon: 'emojiHappy', onPress: () => setIconFor(teamspace) },
        { label: 'Move up', icon: 'arrowUp', disabled: index <= 0, onPress: () => move(index - 1) },
        { label: 'Move down', icon: 'arrowDown', disabled: index >= sortedTeamspaces.length - 1, onPress: () => move(index + 1) },
        ...(workspace.permissions.includes('teamspaces.delete')
          ? [
              {
                label: 'Delete teamspace',
                icon: 'trash' as const,
                destructive: true,
                onPress: () =>
                  setConfirm({
                    title: `Delete “${teamspace.name}”?`,
                    description: 'Only an empty teamspace can be deleted. Pages in its trash are deleted with it.',
                    confirmLabel: 'Delete teamspace',
                    danger: true,
                    onConfirm: () => teamspaceWrites.remove.mutate(teamspace, { onError: failed('Could not delete the teamspace.') }),
                  }),
              },
            ]
          : []),
      ],
    })
  }

  return (
    <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
      <PaneHeader>
        <View className="flex-1">
          <HeaderTitle>Documents</HeaderTitle>
        </View>
        <IconButton icon="search" label="Search docs" onPress={() => router.push('/docs/search')} />
        {/* the web's menu has Import from Notion, a desktop upload: the app offers the other docs actions */}
        <IconButton
          icon="more"
          label="Documents options"
          onPress={() =>
            setMenu({
              title: 'Documents',
              actions: [
                { label: 'New teamspace', icon: 'people', onPress: newTeamspace },
                { label: 'Trash', icon: 'trash', onPress: () => router.push('/docs/trash') },
              ],
            })
          }
        />
        <IconButton icon="plus" label="New page" size={32} variant="primary" disabled={create.isPending} onPress={() => createMenu('New', { space: null })} />
      </PaneHeader>
      {notice ? (
        <Pressable accessibilityRole="alert" accessibilityLabel={notice} onPress={() => setNotice(null)} className="mx-3 mb-1 p-2.5 rounded-md bg-destructive/10">
          <Text className="text-[13px] text-destructive">{notice}</Text>
        </Pressable>
      ) : null}
      {!spaces ? (
        pages.error || teamspaces.error ? (
          <ErrorView error={pages.error ?? teamspaces.error} onRetry={() => void refresh()} />
        ) : (
          <LoadingView />
        )
      ) : (
        <ScrollView contentContainerClassName="px-2 pt-2 pb-4 gap-3.5" refreshControl={<PullRefresh onRefresh={() => refresh()} />}>
          {favoriteRows.length > 0 ? (
            <View>
              <GroupHeader label="Favorites" icon="star" />
              {favoriteRows.map((row) => (
                <TreeItem
                  key={row.id}
                  label={pageTitle(row)}
                  hint={row.hint}
                  paddingLeft={28}
                  leading={<PageIcon page={row} color={muted} />}
                  onPress={() => router.push(`/docs/${row.id}`)}
                  actions={<IconButton icon="more" label={`${pageTitle(row)} options`} size={22} iconSize={13} color={muted} onPress={() => favoriteMenu(row)} />}
                />
              ))}
            </View>
          ) : null}
          <View>
            <GroupHeader label="Teamspaces" action={<IconButton icon="plus" label="New teamspace" size={22} iconSize={13} color={muted} onPress={newTeamspace} />} />
            {spaces.teamspaces.map((space) => {
              const open = !collapsed.has(space.key)
              const teamspace = sortedTeamspaces.find((item) => `teamspace:${item.id}` === space.key)
              return (
                <View key={space.key}>
                  <TreeItem
                    label={space.name}
                    leading={teamspace?.icon ? <Text className="text-[14px]">{teamspace.icon}</Text> : <Icon name="people" size={15} color={muted} />}
                    paddingLeft={8}
                    onPress={() => toggle(space.key)}
                    actions={
                      <>
                        {teamspace ? <IconButton icon="more" label={`${space.name} options`} size={22} iconSize={13} color={muted} onPress={() => teamspaceMenu(teamspace)} /> : null}
                        <IconButton icon="plus" label={`New page in ${space.name}`} size={22} iconSize={13} color={muted} onPress={() => createMenu(`New in ${space.name}`, { space: space.key })} />
                      </>
                    }
                  />
                  {open ? <PageRows rows={space.rows} collapsed={collapsed} onToggle={toggle} onCreateChild={(page) => createMenu(`New in ${pageTitle(page)}`, { parentId: page.id })} onMenu={pageMenu} /> : null}
                </View>
              )
            })}
          </View>
          <View>
            <GroupHeader label="Private" icon="lock" action={<IconButton icon="plus" label="New private page" size={22} iconSize={13} color={muted} onPress={() => createMenu('New private', { space: 'private' })} />} />
            {spaces.private.rows.length === 0 ? (
              <View className="h-8 justify-center pl-10">
                <Text className="text-[13px] text-muted-foreground/60">No pages inside</Text>
              </View>
            ) : (
              <PageRows rows={spaces.private.rows} collapsed={collapsed} onToggle={toggle} onCreateChild={(page) => createMenu(`New in ${pageTitle(page)}`, { parentId: page.id })} onMenu={pageMenu} />
            )}
          </View>
        </ScrollView>
      )}
      <Pressable
        accessibilityRole="link"
        accessibilityLabel="Trash"
        onPress={() => router.push('/docs/trash')}
        className="h-[49px] flex-row items-center gap-2 px-4 border-t border-border active:bg-muted"
      >
        <Icon name="trash" size={15} color={colors.mutedForeground} />
        <Text className="font-noto-medium text-[13px] text-muted-foreground">Trash</Text>
      </Pressable>

      <ActionSheet visible={menu !== null} title={menu?.title} actions={menu?.actions ?? []} onClose={() => setMenu(null)} />
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
      <PromptDialog request={prompt} onClose={() => setPrompt(null)} />
      {moveSheet.sheet}
      <IconDialog
        visible={iconFor !== null}
        title={`Icon for ${iconFor?.name ?? ''}`}
        current={iconFor?.icon ?? null}
        onPick={(icon) => iconFor && teamspaceWrites.save.mutate({ teamspace: iconFor, icon }, { onError: failed('Could not change the icon.') })}
        onClose={() => setIconFor(null)}
      />
    </SafeAreaView>
  )
}

function GroupHeader({ label, icon, action }: { label: string; icon?: IconName; action?: ReactNode }) {
  const { colors } = useTheme()
  return (
    <View className="h-7 flex-row items-center gap-1.5 px-2">
      {icon ? <Icon name={icon} size={14} color={colors.mutedForeground} /> : null}
      <Text className="font-noto-medium text-[12px] leading-[16px] flex-1 text-muted-foreground">{label}</Text>
      {action}
    </View>
  )
}

function PageRows({
  rows,
  collapsed,
  onToggle,
  onCreateChild,
  onMenu,
}: {
  rows: TreeRow[]
  collapsed: ReadonlySet<string>
  onToggle: (key: string) => void
  onCreateChild: (page: PageSummary) => void
  onMenu: (page: PageSummary) => void
}) {
  const { colors } = useTheme()
  const muted = alpha(colors.mutedForeground, 0.7)
  return rows.map((row) => (
    <TreeItem
      key={row.page.id}
      label={pageTitle(row.page)}
      paddingLeft={28 + row.depth * 18}
      chevron={row.hasChildren ? { open: !collapsed.has(row.page.id), onPress: () => onToggle(row.page.id) } : null}
      leading={<PageIcon page={row.page} color={muted} />}
      onPress={() => router.push(`/docs/${row.page.id}`)}
      actions={
        <>
          <IconButton icon="more" label={`${pageTitle(row.page)} options`} size={22} iconSize={13} color={muted} onPress={() => onMenu(row.page)} />
          {row.page.kind === 'database' ? null : <IconButton icon="plus" label={`New page in ${pageTitle(row.page)}`} size={22} iconSize={13} color={muted} onPress={() => onCreateChild(row.page)} />}
        </>
      }
    />
  ))
}

/** One 32px tree row: an optional chevron slot, the icon, the name, and the row actions. */
function TreeItem({
  label,
  hint,
  leading,
  paddingLeft,
  chevron,
  onPress,
  actions,
}: {
  label: string
  /** Muted text after the label (the database of a favorite row). */
  hint?: string | null
  leading: ReactNode
  paddingLeft: number
  chevron?: { open: boolean; onPress: () => void } | null
  onPress: () => void
  actions: ReactNode
}) {
  const { colors } = useTheme()
  return (
    <View className="h-8 mt-px flex-row items-center">
      <Pressable
        accessibilityRole="link"
        accessibilityLabel={label}
        onPress={onPress}
        className="flex-1 h-8 flex-row items-center rounded-md active:bg-muted" style={{ paddingLeft }}
      >
        {chevron !== undefined ? (
          <Pressable accessibilityRole="button" accessibilityLabel={chevron?.open ? `Collapse ${label}` : `Expand ${label}`} disabled={!chevron} onPress={chevron?.onPress} className="w-4 mr-1.5 items-center">
            {chevron ? <Icon name={chevron.open ? 'chevronDown' : 'chevronRight'} size={12} color={colors.mutedForeground} /> : null}
          </Pressable>
        ) : null}
        <View className={`w-[15px] items-center ${chevron !== undefined ? 'mr-[7px]' : 'mr-2'}`}>{leading}</View>
        <Text numberOfLines={1} className="font-noto-medium text-[13px] flex-1">
          {label}
          {hint ? <Text className="font-noto text-[13px] text-muted-foreground/70">{`  ${hint}`}</Text> : null}
        </Text>
      </Pressable>
      <View className="flex-row gap-0.5 pl-1 pr-1.5">{actions}</View>
    </View>
  )
}

/** The page's emoji, else a table for a database and a document for a page. */
function PageIcon({ page, color }: { page: Pick<PageSummary, 'icon' | 'kind'>; color: string }) {
  return page.icon ? <Text className="text-[14px]">{page.icon}</Text> : <Icon name={page.kind === 'database' ? 'grid' : 'docs'} size={15} color={color} />
}
