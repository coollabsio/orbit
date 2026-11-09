import type { CollabProblem } from '@orbit/editor/collab/session'
import { isSafeLinkHref } from '@orbit/editor/links'
import { useQuery } from '@tanstack/react-query'
import { router, useLocalSearchParams } from 'expo-router'
import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { Image, KeyboardAvoidingView, Linking, Platform, ScrollView, View } from 'react-native'
import { TextInput } from '../../../ui/scaled'

import { CommentsSheet } from '../../../components/docs/Comments'
import { EditorToolbar } from '../../../components/docs/EditorToolbar'
import { IconDialog } from '../../../components/docs/IconDialog'
import { DatabaseView } from '../../../components/database/DatabaseView'
import { PropertyList } from '../../../components/database/PropertyList'
import PageEditor, { type PageCardInfo, type PageEditorProps, type PageEditorRef } from '../../../components/docs/PageEditor.dom'
import { ActionSheet, ConfirmDialog, type ConfirmRequest, PromptDialog, type PromptRequest, type SheetAction } from '../../../components/settings/Dialog'
import { absoluteUrl, pageTitle, subPagesOf, trashPrompt, validCoverUrl, visibleThreads } from '../../../data/docs'
import { collabTicket, descendants, fileDataUrl, fileDeleted } from '../../../data/docs-api'
import { keys } from '../../../data/keys'
import * as api from '../../../data/orbit'
import { BLOCK_KINDS, type BlockKind, type EditorCommand, type FormatState, IDLE_FORMAT, PROBLEM_RESPONSES, type ProblemResponse, whileMounted } from '../../../data/page-editor'
import { pickImage } from '../../../hooks/account'
import { isFavorite, useFavorites, usePageThreads, usePageTrash, useSavePage, useToggleFavorite, useTrashPage, useUploadPageImage } from '../../../hooks/docs'
import { useOrbit } from '../../../orbit-context'
import { useMembers, usePages } from '../../../queries'
import { errorMessage } from '../../../session-context'
import { alpha, useTheme } from '../../../theme'
import { Button, CountBadge, ErrorView, Icon, IconButton, LoadingView, PaneHeader, PullRefresh, SafeAreaView, Text } from '../../../ui'

/** Room kept between the caret and the toolbar or the top of the screen while typing. */
const CARET_MARGIN = 24

/**
 * A page: its cover, icon and title (native), and its body in the page editor (BlockNote in a web view), which
 * co-edits the page live with everyone else. A formatting bar sits above the keyboard while the body has focus.
 */
export default function DocScreen() {
  const { id } = useLocalSearchParams<{ id: string }>()
  const { client, workspaceId, session } = useOrbit()
  const { colors } = useTheme()
  const page = useQuery({ queryKey: keys.page(workspaceId, id), queryFn: () => api.page(client, workspaceId, id) })
  const pages = usePages()
  const favorites = useFavorites()
  const threads = usePageThreads(id)
  const save = useSavePage(id)
  const uploadImage = useUploadPageImage(id)
  const toggleFavorite = useToggleFavorite()
  const trash = useTrashPage()
  const [title, setTitle] = useState<string | null>(null)
  const [menu, setMenu] = useState<SheetAction[] | null>(null)
  const [confirm, setConfirm] = useState<ConfirmRequest | null>(null)
  const [prompt, setPrompt] = useState<PromptRequest | null>(null)
  const [choosingIcon, setChoosingIcon] = useState(false)
  const [commentsOpen, setCommentsOpen] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const muted = alpha(colors.mutedForeground, 0.7)
  const favorite = isFavorite(favorites.data, id)
  const openThreads = visibleThreads(threads.data ?? []).filter((thread) => !thread.resolved).length
  const locked = Boolean(page.data?.locked_at)
  const failed = (what: string) => (error: unknown) => setNotice(`${what} ${errorMessage(error)}`)

  const editor = useRef<PageEditorRef>(null)
  const [format, setFormat] = useState<FormatState>(IDLE_FORMAT)
  const [choosingBlock, setChoosingBlock] = useState(false)
  const [problem, setProblem] = useState<ProblemResponse | null>(null)
  const [generation, setGeneration] = useState(0)
  const scroll = useRef<React.ComponentRef<typeof ScrollView>>(null)
  const scrollY = useRef(0)
  const viewportHeight = useRef(0)
  const editorTop = useRef(0)
  const editable = !locked && !problem?.readOnly

  // Keep the caret between the top of the screen and the toolbar while typing.
  useEffect(() => {
    if (!format.focused || !format.caret) return
    const top = editorTop.current + format.caret.top
    const bottom = editorTop.current + format.caret.bottom
    const visibleTop = scrollY.current + CARET_MARGIN
    const visibleBottom = scrollY.current + viewportHeight.current - CARET_MARGIN
    if (bottom > visibleBottom) scroll.current?.scrollTo({ y: bottom - viewportHeight.current + CARET_MARGIN, animated: true })
    else if (top < visibleTop) scroll.current?.scrollTo({ y: Math.max(0, top - CARET_MARGIN), animated: true })
  }, [format])

  const run = (command: EditorCommand) => editor.current?.run(command)
  const addImage = () =>
    pickImage({ square: false }).then(
      async (file) => {
        if (!file) return
        const url = await uploadImage.mutateAsync(file)
        run({ type: 'image', url, name: (file as { name?: string }).name ?? 'image' })
      },
      failed('Could not read the image.'),
    ).catch(failed('Could not upload the image.'))

  const onProblem = (next: CollabProblem) => {
    const response = PROBLEM_RESPONSES[next]
    setProblem(response.notice || response.readOnly || response.stop ? response : null)
    if (!response.refetch) return
    void page.refetch().then(() => {
      if (!response.reconnect) return
      if (!response.readOnly) setProblem(null)
      setGeneration((value) => value + 1)
    })
  }
  // The socket handshake keeps failing: the page may be gone or out of reach.
  const onProbe = () =>
    void page.refetch().then(({ error }) => {
      const status = (error as { status?: unknown } | null)?.status
      if (status === 404) onProblem('gone')
      else if (status === 403) onProblem('forbidden')
    })

  // a new page from another device or a save: show the server's title unless one is being typed
  useEffect(() => setTitle(null), [page.data?.title])

  const saveTitle = () => {
    // Return and the blur that follows both land here: one save is enough
    if (save.isPending) return
    if (title === null || !page.data || title.trim() === page.data.title) return setTitle(null)
    save.mutate({ title: title.trim() }, { onSuccess: () => setTitle(null), onError: failed('Could not rename the page.') })
  }
  const saveIcon = (icon: string | null) => save.mutate({ icon }, { onError: failed('Could not change the icon.') })
  const coverFromLibrary = () =>
    pickImage({ square: false }).then(
      async (file) => {
        if (!file) return
        const url = await uploadImage.mutateAsync(file)
        await save.mutateAsync({ cover_url: url })
      },
      failed('Could not read the image.'),
    ).catch(failed('Could not upload the cover.'))
  const coverActions = (): SheetAction[] => [
    { label: 'Upload an image', icon: 'import', onPress: () => void coverFromLibrary() },
    {
      label: 'Paste an image URL',
      icon: 'link2',
      onPress: () =>
        setPrompt({
          title: 'Cover image',
          label: 'Image URL',
          placeholder: 'https://…',
          keyboard: 'url',
          confirmLabel: 'Use image',
          onSubmit: async (value) => {
            const url = validCoverUrl(value)
            if (!url) throw new Error('Enter an http(s) image URL.')
            await save.mutateAsync({ cover_url: url })
          },
        }),
    },
    ...(page.data?.cover_url ? [{ label: 'Remove cover', icon: 'trash' as const, destructive: true, onPress: () => save.mutate({ cover_url: null }, { onError: failed('Could not remove the cover.') }) }] : []),
  ]
  /** A database row and its sub-pages are not in the tree: the server counts their sub-pages. */
  const confirmTrash = async () => {
    const target = page.data
    if (!target) return
    const below = await subPagesOf(pages.data ?? [], target.id, (pageId) => descendants(client, workspaceId, pageId)).catch(failed('Could not move the page to the trash.'))
    if (!below) return
    setConfirm({
      ...trashPrompt(target, below.length),
      confirmLabel: 'Move to trash',
      danger: true,
      onConfirm: () => trash.mutate(target, { onSuccess: () => (router.canGoBack() ? router.back() : router.navigate('/docs')), onError: failed('Could not move the page to the trash.') }),
    })
  }
  const pageMenu = (): SheetAction[] =>
    page.data
      ? [
          { label: favorite ? 'Remove from favorites' : 'Add to favorites', icon: 'star', onPress: () => toggleFavorite.mutate({ pageId: id, favorite: !favorite, page: page.data }) },
          { label: page.data.icon ? 'Change icon' : 'Add icon', icon: 'emojiHappy', disabled: locked, onPress: () => setChoosingIcon(true) },
          { label: page.data.cover_url ? 'Change cover' : 'Add cover', icon: 'sticker', disabled: locked, onPress: () => setMenu(coverActions()) },
          { label: 'Move to trash', icon: 'trash', destructive: true, onPress: () => void confirmTrash() },
        ]
      : []

  /** The cover, icon and title of the page, above the body (a page) or the views (a database). */
  const head = page.data ? (
    <>
      {page.data.cover_url ? (
        <View>
          <Image
            accessibilityLabel="Cover"
            source={{ uri: absoluteUrl(session.server, page.data.cover_url), headers: page.data.cover_url.startsWith('/') ? { Authorization: `Bearer ${session.token}` } : undefined }}
            className="h-35 w-full bg-muted"
            resizeMode="cover"
          />
          {!locked ? (
            <View className="absolute right-3 bottom-2">
              <Button label="Change cover" size="xs" variant="secondary" onPress={() => setMenu(coverActions())} />
            </View>
          ) : null}
        </View>
      ) : null}
      <View className="px-3 pt-6">
        {page.data.icon ? (
          <Text accessibilityRole="button" accessibilityLabel="Change icon" onPress={locked ? undefined : () => setChoosingIcon(true)} className="text-[40px] leading-[48px] mb-2 self-start">
            {page.data.icon}
          </Text>
        ) : null}
        {!locked && (!page.data.icon || !page.data.cover_url) ? (
          <View className="flex-row gap-2 mb-2.5">
            {page.data.icon ? null : <Button label="😀 Add icon" variant="ghost" size="sm" muted onPress={() => setChoosingIcon(true)} />}
            {page.data.cover_url ? null : <Button label="🖼️ Add cover" variant="ghost" size="sm" muted onPress={() => setMenu(coverActions())} />}
          </View>
        ) : null}
        {locked ? (
          <View className="flex-row items-center gap-1.5 mb-2.5">
            <Icon name="lock" size={13} color={colors.mutedForeground} />
            <Text className="text-[12px] text-muted-foreground">Locked. Unlock it on the web to edit.</Text>
          </View>
        ) : null}
        <TextInput
          accessibilityLabel="Page title"
          value={title ?? page.data.title}
          placeholder="Untitled"
          placeholderTextColor={muted}
          editable={!locked}
          onChangeText={setTitle}
          onBlur={saveTitle}
          onSubmitEditing={saveTitle}
          returnKeyType="done"
          className="font-noto-bold text-[16px] leading-[20px] mb-[17px] p-0 text-foreground"
        />
      </View>
    </>
  ) : null

  return (
    <SafeAreaView edges={['left', 'right']} className="flex-1 bg-background">
      <PaneHeader className="min-h-12 px-3">
        <IconButton icon="back" label="Back to docs" color={muted} onPress={() => (router.canGoBack() ? router.back() : router.navigate('/docs'))} />
        <Text numberOfLines={1} className="font-noto-medium text-[13px] flex-1">
          {page.data ? pageTitle(page.data) : ''}
        </Text>
        <View>
          <IconButton icon="comment" label={openThreads > 0 ? `Comments, ${openThreads} open` : 'Comments'} color={muted} onPress={() => setCommentsOpen(true)} />
          {openThreads > 0 ? (
            <View pointerEvents="none" className="absolute -top-1 -right-1.5">
              <CountBadge count={openThreads} />
            </View>
          ) : null}
        </View>
        <IconButton icon="star" label={favorite ? 'Remove from favorites' : 'Add to favorites'} color={favorite ? colors.primary : muted} onPress={() => toggleFavorite.mutate({ pageId: id, favorite: !favorite, page: page.data })} />
        <IconButton icon="more" label="Page options" color={muted} onPress={() => setMenu(pageMenu())} />
      </PaneHeader>
      {problem?.notice ? (
        <Text accessibilityRole="alert" className="text-[12px] mx-3 mb-1 text-muted-foreground">
          {problem.notice}
        </Text>
      ) : null}
      {notice ? (
        <Text accessibilityRole="alert" onPress={() => setNotice(null)} className="text-[13px] mx-3 mb-1 p-2.5 rounded-md overflow-hidden text-destructive bg-destructive/10">
          {notice}
        </Text>
      ) : null}
      {!page.data ? (
        page.isError ? (
          <ErrorView error={page.error} onRetry={() => void page.refetch()} />
        ) : (
          <LoadingView />
        )
      ) : page.data.kind === 'database' ? (
        <DatabaseView databaseId={id} header={head!} notify={setNotice} />
      ) : (
        <KeyboardAvoidingView className="flex-1" behavior={Platform.OS === 'ios' ? 'padding' : undefined} keyboardVerticalOffset={0}>
          <ScrollView
            ref={scroll}
            contentContainerClassName="pb-8"
            keyboardShouldPersistTaps="handled"
            scrollEventThrottle={16}
            onScroll={(event) => (scrollY.current = event.nativeEvent.contentOffset.y)}
            onLayout={(event) => (viewportHeight.current = event.nativeEvent.layout.height)}
            refreshControl={<PullRefresh onRefresh={() => page.refetch()} />}
          >
            {head}
            {page.data.database_id ? <PropertyList databaseId={page.data.database_id} rowId={id} notify={setNotice} /> : null}
            <View onLayout={(event) => (editorTop.current = event.nativeEvent.layout.y)}>
              {problem?.stop ? null : (
                <EditorView
                  ref={editor}
                  pageId={id}
                  epoch={page.data.collab_epoch}
                  generation={generation}
                  editable={editable}
                  onProblem={onProblem}
                  onProbe={onProbe}
                  onFormat={setFormat}
                />
              )}
            </View>
          </ScrollView>
          {format.focused && editable ? <EditorToolbar format={format} onCommand={run} onBlockMenu={() => setChoosingBlock(true)} onImage={() => void addImage()} /> : null}
        </KeyboardAvoidingView>
      )}
      <ActionSheet visible={menu !== null} actions={menu ?? []} onClose={() => setMenu(null)} />
      <ConfirmDialog request={confirm} onClose={() => setConfirm(null)} />
      <PromptDialog request={prompt} onClose={() => setPrompt(null)} />
      <ActionSheet
        visible={choosingBlock}
        title="Turn into"
        actions={(Object.keys(BLOCK_KINDS) as BlockKind[]).map((kind) => ({ label: BLOCK_KINDS[kind].label, onPress: () => run({ type: 'block', kind }) }))}
        onClose={() => setChoosingBlock(false)}
      />
      <IconDialog visible={choosingIcon} title="Page icon" current={page.data?.icon ?? null} onPick={saveIcon} onClose={() => setChoosingIcon(false)} />
      <CommentsSheet pageId={id} visible={commentsOpen} onClose={() => setCommentsOpen(false)} />
    </SafeAreaView>
  )
}

type EditorViewProps = Pick<PageEditorProps, 'pageId' | 'epoch' | 'generation' | 'editable'> & {
  ref: React.Ref<PageEditorRef>
  onProblem: (problem: CollabProblem) => void
  onProbe: () => void
  onFormat: (state: FormatState) => void
}

/**
 * The page editor with what it needs from the app: the session (tickets, files), the theme, page and member names.
 * Every render sends all props to the web view, so it renders only when they change: the screen re-renders on every
 * selection change (the toolbar), and the callbacks are read through a ref.
 */
const EditorView = memo(function EditorView({ ref, pageId, epoch, generation, editable, ...callbacks }: EditorViewProps) {
  const { client, workspaceId, session, user } = useOrbit()
  const { scheme, colors } = useTheme()
  const pages = usePages()
  const trash = usePageTrash()
  const members = useMembers()
  const [height, setHeight] = useState(120)
  const latest = useRef(callbacks)
  useEffect(() => {
    latest.current = callbacks
  })
  const mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])

  const cards = useMemo(() => {
    const out: Record<string, PageCardInfo> = {}
    for (const item of trash.data ?? []) out[item.id] = { title: item.title, icon: item.icon, trashed: true }
    for (const item of pages.data ?? []) out[item.id] = { title: item.title, icon: item.icon, trashed: false }
    return out
  }, [pages.data, trash.data])
  const names = useMemo(() => Object.fromEntries((members.data ?? []).map((member) => [member.user_id, member.display_name])), [members.data])
  const editorColors = useMemo(
    () => ({ background: colors.background, foreground: colors.foreground, mutedForeground: colors.mutedForeground, muted: colors.muted, border: colors.border, primary: colors.primary }),
    [colors],
  )
  // the server shows everyone else this session's name and a color of its own
  const me = useMemo(() => ({ name: user.display_name, color: colors.mutedForeground }), [user.display_name, colors.mutedForeground])
  const actions = useMemo(
    () => ({
      getTicket: whileMounted(mounted, () => collabTicket(client, pageId)),
      loadFile: whileMounted(mounted, (url: string) => fileDataUrl(session.server, session.token, url)),
      fileDeleted: whileMounted(mounted, (url: string) => fileDeleted(session.server, session.token, url)),
      openPage: whileMounted(mounted, async (target: string) => router.push(`/docs/${target}`)),
      // the editor already filters links; the app checks again before it hands one to the OS
      openUrl: whileMounted(mounted, async (url: string) => {
        if (isSafeLinkHref(url)) void Linking.openURL(url).catch(() => {})
      }),
      onProblem: whileMounted(mounted, async (problem: CollabProblem) => latest.current.onProblem(problem)),
      onProbe: whileMounted(mounted, async () => latest.current.onProbe()),
      onFormat: whileMounted(mounted, async (state: FormatState) => latest.current.onFormat(state)),
      onHeight: whileMounted(mounted, async (next: number) => setHeight(next)),
    }),
    [client, pageId, session.server, session.token],
  )
  const dom = useMemo(() => ({ scrollEnabled: false, style: { height } }), [height])

  return (
    <PageEditor
      ref={ref}
      dom={dom}
      server={session.server}
      workspaceId={workspaceId}
      pageId={pageId}
      epoch={epoch}
      generation={generation}
      editable={editable}
      theme={scheme}
      colors={editorColors}
      user={me}
      pages={cards}
      members={names}
      {...actions}
    />
  )
})
