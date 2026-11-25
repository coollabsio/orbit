import { parseGithubPullUrl } from '@orbit/api/github'
import { proxiedImageUrl } from '@orbit/editor/outsideUrl'
import type { LinkPreview } from '@orbit/api/generated/types.gen'
import { useQuery } from '@tanstack/react-query'
import { router } from 'expo-router'
import * as WebBrowser from 'expo-web-browser'
import { useState } from 'react'
import { Image, Pressable, View } from 'react-native'
import { SvgXml } from 'react-native-svg'

import { linkPreview } from '../../data/chat-api'
import { pageTitle, spaceLabel, spaceOf } from '../../data/docs'
import { relativeAgo } from '../../data/format'
import { keys } from '../../data/keys'
import { extractLinkUrls, linkTaskIds, orbitLinkTarget } from '../../data/link-cards'
import * as api from '../../data/orbit'
import { identifier } from '../../data/tasks'
import { useServerImage } from '../../hooks/chat'
import { useIsGuest, useOrbit } from '../../orbit-context'
import { useMembers, usePages, useProjects, useStatuses, useTeamspaces } from '../../queries'
import { alpha, cssColors, useTheme } from '../../theme'
import { Avatar, Icon, type IconName, StatusIcon, Text } from '../../ui'
import { GithubPullCard } from '../pulls/GithubPullCard'

const COUNT = new Intl.NumberFormat(undefined, { notation: 'compact', maximumFractionDigits: 1 })
const POSTED = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
const X_LOGO = '<svg viewBox="0 0 24 24"><path fill="currentColor" d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z"/></svg>'

const openUrl = (url: string) => void WebBrowser.openBrowserAsync(url).catch(() => undefined)

/**
 * The web's `LinkCards`: under a message, a card for each of its first three URLs. A GitHub pull request, an Orbit task
 * or page (also a typed task identifier such as `ENG-12`), or a preview of another site. A card shows nothing while it
 * loads and when its URL has nothing to show. A guest gets no GitHub or Orbit cards, as on the web.
 */
export function LinkCards({ body }: { body: string }) {
  const { session } = useOrbit()
  const guest = useIsGuest()
  const projects = useProjects(!guest).data
  const origin = new URL(session.server).origin
  const urls = extractLinkUrls(linkTaskIds(body, guest ? [] : (projects ?? []).map((project) => project.key), origin))
  if (urls.length === 0) return null
  return (
    <View className="mt-1.5 gap-1.5">
      {urls.map((url) => {
        const pullRef = guest ? null : parseGithubPullUrl(url)
        if (pullRef) return <GithubPullCard key={url} pullRef={pullRef} />
        const target = orbitLinkTarget(url, origin)
        // a saved view has no screen in the app, and the web shows other Orbit URLs as plain links
        if (target) return guest || target.kind === 'view' ? null : target.kind === 'task' ? <TaskCard key={url} taskRef={target.id} /> : <PageCard key={url} pageId={target.id} />
        return <LinkEmbed key={url} url={url} />
      })}
    </View>
  )
}

/** The web's task card, read-only: status, title, assignee, identifier, status name and project. Tapping opens the task. */
function TaskCard({ taskRef }: { taskRef: string }) {
  const { client, workspaceId } = useOrbit()
  const task = useQuery({ queryKey: keys.task(workspaceId, taskRef), queryFn: () => api.task(client, workspaceId, taskRef), retry: false, staleTime: 30_000 }).data
  const projects = useProjects().data ?? []
  const status = useStatuses(task !== undefined).data?.find((candidate) => candidate.id === task?.status_id)
  const members = useMembers().data
  if (!task || task.deleted_at) return null
  const project = projects.find((candidate) => candidate.id === task.project_id)
  const assignee = members?.find((member) => member.user_id === task.assignee_ids[0])
  return (
    <Pressable accessibilityRole="link" accessibilityLabel={task.title} onPress={() => router.push(`/tasks/${task.id}`)} className="gap-0.5 rounded-lg border border-border bg-card py-1.5 pl-2 pr-2.5 active:bg-muted">
      <View className="flex-row items-center gap-1.5">
        <StatusIcon status={status} size={14} />
        <Text numberOfLines={1} className="flex-1 font-noto-medium text-[13px] leading-[19px]">
          {task.title}
        </Text>
        {assignee ? <Avatar id={assignee.user_id} name={assignee.display_name} size={18} /> : null}
      </View>
      <View className="flex-row items-center gap-1.5">
        <Text className="text-[12px] leading-[16px] text-muted-foreground">{identifier(task, projects)}</Text>
        {status ? <Text numberOfLines={1} className="shrink text-[12px] leading-[16px] text-muted-foreground">{status.name}</Text> : null}
        {project ? (
          <Text numberOfLines={1} className="ml-auto shrink text-[12px] leading-[16px] text-muted-foreground">
            {project.name}
          </Text>
        ) : null}
      </View>
    </Pressable>
  )
}

/** The web's page card: icon, title, space and last change. Tapping opens the page. */
function PageCard({ pageId }: { pageId: string }) {
  const { colors } = useTheme()
  const page = usePages().data?.find((candidate) => candidate.id === pageId)
  const teamspaces = useTeamspaces().data
  if (!page) return null
  return (
    <Pressable accessibilityRole="link" accessibilityLabel={pageTitle(page)} onPress={() => router.push(`/docs/${page.id}`)} className="h-9 flex-row items-center gap-2 rounded-lg border border-border bg-card px-2.5 active:bg-muted">
      {page.icon ? <Text className="text-[14px]">{page.icon}</Text> : <Icon name="docText" size={16} color={colors.mutedForeground} />}
      <Text numberOfLines={1} className="flex-1 font-noto-medium text-[13px]">
        {pageTitle(page)}
      </Text>
      <Text numberOfLines={1} className="shrink text-[12px] text-muted-foreground">
        {spaceLabel(spaceOf(page), teamspaces)} · {relativeAgo(page.updated_at)}
      </Text>
    </Pressable>
  )
}

/** The web's `LinkEmbed`: a post on X, a YouTube video, a bare image, or the page's title, description and image. */
function LinkEmbed({ url }: { url: string }) {
  const { client, workspaceId } = useOrbit()
  // not live: read once, as on the web
  const preview = useQuery({ queryKey: keys.linkPreview(workspaceId, url), queryFn: () => linkPreview(client, workspaceId, url), staleTime: Infinity, gcTime: 30 * 60_000, retry: false }).data
  if (!preview) return null
  if (preview.kind === 'x') return <PostEmbed preview={preview} />
  if (preview.kind === 'youtube') return <VideoEmbed preview={preview} />
  if (preview.kind === 'image') {
    return (
      <Pressable accessibilityRole="link" accessibilityLabel="Image" onPress={() => openUrl(preview.url)} className="w-full active:opacity-80">
        <EmbedImage uri={preview.image_url} fit className="max-h-80 rounded-lg border border-border" />
      </Pressable>
    )
  }
  return <PageEmbed preview={preview} />
}

/**
 * An image of another site, loaded through Orbit, so the site does not learn who reads the message or where they
 * are. Without a source, and when it does not load, it takes no space. `fit` keeps the image's own shape (a bare
 * image, a post's photo); otherwise the class sets the box.
 */
function EmbedImage({ uri, fit = false, className = '' }: { uri: string | null | undefined; fit?: boolean; className?: string }) {
  const { session } = useOrbit()
  const image = useServerImage()
  const [failed, setFailed] = useState(false)
  const [ratio, setRatio] = useState<number | null>(null)
  if (!uri || failed) return null
  return (
    <Image
      source={image(proxiedImageUrl(uri, new URL(session.server).origin))}
      onError={() => setFailed(true)}
      onLoad={(event) => {
        // iOS and Android give the image's size; the web preview does not, and keeps the 16:9 box
        const size = event.nativeEvent?.source
        if (fit && size?.width && size.height) setRatio(size.width / size.height)
      }}
      resizeMode="cover"
      className={`bg-muted ${className}`}
      style={fit ? { width: '100%', aspectRatio: ratio ?? 16 / 9 } : undefined}
    />
  )
}

/** The mark of a video on its poster. */
function PlayMark() {
  return (
    <View className="absolute inset-0 items-center justify-center">
      <View className="size-11 items-center justify-center rounded-full" style={{ backgroundColor: alpha(cssColors.black, 0.65) }}>
        <Icon name="play" size={20} color={cssColors.white} />
      </View>
    </View>
  )
}

function PageEmbed({ preview }: { preview: LinkPreview }) {
  return (
    <Pressable accessibilityRole="link" accessibilityLabel={preview.title ?? preview.url} onPress={() => openUrl(preview.url)} className="overflow-hidden rounded-lg border border-border bg-card active:bg-muted">
      {preview.large_image ? <EmbedImage uri={preview.image_url} className="w-full aspect-[1.91] border-b border-border" /> : null}
      <View className="flex-row items-start gap-3 p-2.5">
        <View className="min-w-0 flex-1 gap-0.5">
          <View className="flex-row items-center gap-1.5">
            <EmbedImage uri={preview.icon_url} className="size-3.5 rounded-[3px]" />
            <Text numberOfLines={1} className="shrink text-[12px] text-muted-foreground">
              {preview.site_name ?? new URL(preview.url).hostname}
            </Text>
          </View>
          {preview.title ? (
            <Text numberOfLines={2} className="font-noto-medium text-[13px] leading-[19px]">
              {preview.title}
            </Text>
          ) : null}
          {preview.description ? (
            <Text numberOfLines={2} className="text-[12px] leading-[17px] text-muted-foreground">
              {preview.description}
            </Text>
          ) : null}
        </View>
        {preview.large_image ? null : <EmbedImage uri={preview.image_url} className="size-16 rounded-md" />}
      </View>
    </Pressable>
  )
}

/** A post on X: who wrote it, what it says, its photo or video poster, and its numbers. Tapping opens the post. */
function PostEmbed({ preview }: { preview: LinkPreview }) {
  const { colors } = useTheme()
  const counts: { label: string; icon: IconName; count: number | null | undefined }[] = [
    { label: 'replies', icon: 'messageText', count: preview.replies },
    { label: 'reposts', icon: 'repeat', count: preview.reposts },
    { label: 'likes', icon: 'heart', count: preview.likes },
  ]
  return (
    <Pressable accessibilityRole="link" accessibilityLabel={`Post by ${preview.author_name ?? preview.author_handle ?? 'X'}`} onPress={() => openUrl(preview.url)} className="gap-2 rounded-lg border border-border bg-card p-3 active:bg-muted">
      <View className="flex-row items-center gap-2">
        <EmbedImage uri={preview.author_avatar_url} className="size-8 rounded-full" />
        <View className="min-w-0 flex-1">
          <Text numberOfLines={1} className="font-noto-medium text-[13px] leading-[17px]">
            {preview.author_name ?? preview.author_handle}
          </Text>
          {preview.author_handle ? <Text numberOfLines={1} className="text-[12px] leading-[16px] text-muted-foreground">@{preview.author_handle}</Text> : null}
        </View>
        <SvgXml xml={X_LOGO} width={16} height={16} color={colors.mutedForeground} />
      </View>
      {preview.description ? (
        <Text numberOfLines={10} className="text-[13px] leading-[19px]">
          {preview.description}
        </Text>
      ) : null}
      {preview.image_url ? (
        <View className="overflow-hidden rounded-md border border-border">
          <EmbedImage uri={preview.image_url} fit className="max-h-80" />
          {preview.video_url ? <PlayMark /> : null}
        </View>
      ) : null}
      <View className="flex-row items-center gap-3">
        {counts.map(({ label, icon, count }) =>
          count === null || count === undefined ? null : (
            <View key={label} accessibilityLabel={`${count} ${label}`} className="flex-row items-center gap-1">
              <Icon name={icon} size={14} color={colors.mutedForeground} />
              <Text className="text-[12px] text-muted-foreground">{COUNT.format(count)}</Text>
            </View>
          ),
        )}
        {typeof preview.created_at === 'number' ? <Text className="ml-auto text-[12px] text-muted-foreground">{POSTED.format(preview.created_at * 1000)}</Text> : null}
      </View>
    </Pressable>
  )
}

/** A YouTube video: its poster with a play mark, its title and channel. Tapping opens it in YouTube. */
function VideoEmbed({ preview }: { preview: LinkPreview }) {
  return (
    <Pressable accessibilityRole="link" accessibilityLabel={`Play ${preview.title ?? 'video'}`} onPress={() => openUrl(preview.url)} className="overflow-hidden rounded-lg border border-border bg-card active:bg-muted">
      <View className="w-full aspect-video bg-black">
        <EmbedImage uri={preview.image_url} className="size-full" />
        <PlayMark />
      </View>
      <View className="gap-0.5 border-t border-border p-2.5">
        {preview.title ? (
          <Text numberOfLines={2} className="font-noto-medium text-[13px] leading-[19px]">
            {preview.title}
          </Text>
        ) : null}
        <Text numberOfLines={1} className="text-[12px] text-muted-foreground">
          {['YouTube', preview.author_name].filter(Boolean).join(' · ')}
        </Text>
      </View>
    </Pressable>
  )
}
