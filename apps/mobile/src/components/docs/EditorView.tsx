import type { CollabProblem } from '@orbit/editor/collab/session'
import { isSafeLinkHref } from '@orbit/editor/links'
import { type Href, router } from 'expo-router'
import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { Linking } from 'react-native'

import { collabTicket, fileDataUrl, fileDeleted } from '../../data/docs-api'
import type { PageSummary } from '../../data/orbit'
import { type FormatState, whileMounted } from '../../data/page-editor'
import { usePageTrash } from '../../hooks/docs'
import { useOrbit } from '../../orbit-context'
import { useMembers, usePages } from '../../queries'
import { useTheme } from '../../theme'
import PageEditor, { type PageCardInfo, type PageEditorProps, type PageEditorRef } from './PageEditor.dom'

const defaultHref = (pageId: string): Href => `/docs/${pageId}`

type EditorViewProps = Pick<PageEditorProps, 'pageId' | 'epoch' | 'generation' | 'editable' | 'content'> & {
  ref?: React.Ref<PageEditorRef>
  onProblem: (problem: CollabProblem) => void
  onProbe: () => void
  onFormat: (state: FormatState) => void
  /** Where a page card or a page link goes; the page screen by default. */
  pageHref?: (pageId: string) => Href
  /** More page cards that open (the sub-pages trashed with a previewed page). */
  extraPages?: readonly Pick<PageSummary, 'id' | 'title' | 'icon'>[]
}

/**
 * The page editor with what it needs from the app: the session (tickets, files), the theme, page and member names.
 * Every render sends all props to the web view, so it renders only when they change: the screen re-renders on every
 * selection change (the toolbar), and the callbacks are read through a ref.
 */
export const EditorView = memo(function EditorView({ ref, pageId, epoch, generation, editable, content, pageHref = defaultHref, extraPages, ...callbacks }: EditorViewProps) {
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
    for (const item of [...(pages.data ?? []), ...(extraPages ?? [])]) out[item.id] = { title: item.title, icon: item.icon, trashed: false }
    return out
  }, [pages.data, trash.data, extraPages])
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
      openPage: whileMounted(mounted, async (target: string) => router.push(pageHref(target))),
      // the editor already filters links; the app checks again before it hands one to the OS
      openUrl: whileMounted(mounted, async (url: string) => {
        if (isSafeLinkHref(url)) void Linking.openURL(url).catch(() => {})
      }),
      onProblem: whileMounted(mounted, async (problem: CollabProblem) => latest.current.onProblem(problem)),
      onProbe: whileMounted(mounted, async () => latest.current.onProbe()),
      onFormat: whileMounted(mounted, async (state: FormatState) => latest.current.onFormat(state)),
      onHeight: whileMounted(mounted, async (next: number) => setHeight(next)),
    }),
    [client, pageId, session.server, session.token, pageHref],
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
      content={content}
      theme={scheme}
      colors={editorColors}
      user={me}
      pages={cards}
      members={names}
      {...actions}
    />
  )
})
