import { router } from 'expo-router'
import { type ReactNode, useMemo, useState } from 'react'
import { Pressable, RefreshControl, ScrollView, View } from 'react-native'

import { IconDialog } from '../../../components/docs/IconDialog'
import { MenuButton } from '../../../components/Drawer'
import { ActionSheet, ConfirmDialog, type ConfirmRequest, PromptDialog, type PromptRequest, type SheetAction } from '../../../components/settings/Dialog'
import { docSpaces, favoritePages, pageTitle, trashPrompt, type TreeRow } from '../../../data/docs'
import type { PageSummary, Teamspace } from '../../../data/orbit'
import { useCreatePage, useDuplicatePage, useFavorites, useTeamspaceWrites, useToggleFavorite, useTrashPage } from '../../../hooks/docs'
import { useOrbit } from '../../../orbit-context'
import { usePages, useTeamspaces } from '../../../queries'
import { errorMessage } from '../../../session-context'
import { alpha, useTheme } from '../../../theme'
import { ErrorView, HeaderTitle, Icon, type IconName, IconButton, LoadingView, PaneHeader, SafeAreaView, Text } from '../../../ui'

type Menu = { title: string; actions: SheetAction[] }

/** The web's docs sidebar at phone size: Favorites, Teamspaces with their page trees, Private, and Trash at the bottom. */
export default function DocsScreen() {
  const { colors } = useTheme()
  const { workspace } = useOrbit()
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

  const newPage = (where: { space?: string | null; parentId?: string }) => {
    setNotice(null)
    create.mutate(where, {
      onSuccess: (page) => {
        if (where.parentId) setCollapsed((current) => new Set([...current].filter((key) => key !== where.parentId)))
        router.push(`/docs/${page.id}`)
      },
      onError: failed('Could not create the page.'),
    })
  }
  const newTeamspace = () =>
    setPrompt({ title: 'New teamspace', label: 'Name', placeholder: 'Teamspace name', confirmLabel: 'Create', onSubmit: (name) => teamspaceWrites.create.mutateAsync(name) })

  const pageMenu = (page: PageSummary) => {
    const favorite = (favorites.data ?? []).includes(page.id)
    const hasChildren = (pages.data ?? []).some((item) => item.parent_id === page.id)
    setMenu({
      title: pageTitle(page),
      actions: [
        { label: favorite ? 'Remove from favorites' : 'Add to favorites', icon: 'star', onPress: () => toggleFavorite.mutate({ pageId: page.id, favorite: !favorite }, { onError: failed('Could not change the favorites.') }) },
        { label: 'Duplicate', icon: 'copy', onPress: () => duplicate.mutate({ pageId: page.id, withChildren: false }, { onError: failed('Could not duplicate the page.') }) },
        ...(hasChildren ? [{ label: 'Duplicate with sub-pages', icon: 'copy' as const, onPress: () => duplicate.mutate({ pageId: page.id, withChildren: true }, { onError: failed('Could not duplicate the page.') }) }] : []),
        {
          label: 'Move to trash',
          icon: 'trash',
          destructive: true,
          onPress: () => setConfirm({ ...trashPrompt(pages.data ?? [], page), confirmLabel: 'Move to trash', danger: true, onConfirm: () => trash.mutate(page, { onError: failed('Could not move the page to the trash.') }) }),
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
    <SafeAreaView edges={['top', 'left', 'right']} className="flex-1 bg-background">
      <PaneHeader>
        <MenuButton />
        <View className="flex-1">
          <HeaderTitle>Documents</HeaderTitle>
        </View>
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
        <IconButton icon="plus" label="New page" size={32} variant="primary" disabled={create.isPending} onPress={() => newPage({ space: null })} />
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
        <ScrollView contentContainerClassName="px-2 pt-2 pb-4 gap-3.5" refreshControl={<RefreshControl refreshing={pages.isRefetching} onRefresh={() => void refresh()} tintColor={colors.mutedForeground} />}>
          {favoriteRows.length > 0 ? (
            <View>
              <GroupHeader label="Favorites" icon="star" />
              {favoriteRows.map((page) => (
                <TreeItem
                  key={page.id}
                  label={pageTitle(page)}
                  paddingLeft={28}
                  leading={page.icon ? <Text className="text-[14px]">{page.icon}</Text> : <Icon name="docs" size={15} color={muted} />}
                  onPress={() => router.push(`/docs/${page.id}`)}
                  actions={<IconButton icon="more" label={`${pageTitle(page)} options`} size={22} iconSize={13} color={muted} onPress={() => pageMenu(page)} />}
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
                        <IconButton icon="plus" label={`New page in ${space.name}`} size={22} iconSize={13} color={muted} onPress={() => newPage({ space: space.key })} />
                      </>
                    }
                  />
                  {open ? <PageRows rows={space.rows} collapsed={collapsed} onToggle={toggle} onCreateChild={(parentId) => newPage({ parentId })} onMenu={pageMenu} /> : null}
                </View>
              )
            })}
          </View>
          <View>
            <GroupHeader label="Private" icon="lock" action={<IconButton icon="plus" label="New private page" size={22} iconSize={13} color={muted} onPress={() => newPage({ space: 'private' })} />} />
            {spaces.private.rows.length === 0 ? (
              <View className="h-8 justify-center pl-10">
                <Text className="text-[13px] text-muted-foreground/60">No pages inside</Text>
              </View>
            ) : (
              <PageRows rows={spaces.private.rows} collapsed={collapsed} onToggle={toggle} onCreateChild={(parentId) => newPage({ parentId })} onMenu={pageMenu} />
            )}
          </View>
        </ScrollView>
      )}
      <Pressable
        accessibilityRole="link"
        accessibilityLabel="Trash"
        onPress={() => router.push('/docs/trash')}
        className="h-[49px] flex-row items-center gap-2.5 px-4 border-t border-border active:bg-muted"
      >
        <Icon name="trash" size={15} color={colors.mutedForeground} />
        <Text className="font-noto-medium text-[13px] text-foreground">Trash</Text>
      </Pressable>

      <ActionSheet visible={menu !== null} title={menu?.title} actions={menu?.actions ?? []} onClose={() => setMenu(null)} />
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
      <PromptDialog request={prompt} onClose={() => setPrompt(null)} />
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
  onCreateChild: (parentId: string) => void
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
      leading={row.page.icon ? <Text className="text-[14px]">{row.page.icon}</Text> : <Icon name="docs" size={15} color={muted} />}
      onPress={() => router.push(`/docs/${row.page.id}`)}
      actions={
        <>
          <IconButton icon="plus" label={`New page in ${pageTitle(row.page)}`} size={22} iconSize={13} color={muted} onPress={() => onCreateChild(row.page.id)} />
          <IconButton icon="more" label={`${pageTitle(row.page)} options`} size={22} iconSize={13} color={muted} onPress={() => onMenu(row.page)} />
        </>
      }
    />
  ))
}

/** One 32px tree row: an optional chevron slot, the icon, the name, and the row actions. */
function TreeItem({
  label,
  leading,
  paddingLeft,
  chevron,
  onPress,
  actions,
}: {
  label: string
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
        </Text>
      </Pressable>
      <View className="flex-row gap-0.5 pl-1 pr-1.5">{actions}</View>
    </View>
  )
}
