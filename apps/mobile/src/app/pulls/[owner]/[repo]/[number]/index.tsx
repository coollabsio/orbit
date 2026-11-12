import type { GithubPullDetail, GithubPullFile, GithubReviewThread } from '@orbit/api/generated/types.gen'
import { asideReason, type AsideReason, reviewSummary, threadsOutsideDiff, threadWaitsFor } from '@orbit/api/github'
import { openURL } from 'expo-linking'
import { router, useLocalSearchParams } from 'expo-router'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Pressable, ScrollView, View } from 'react-native'

import { ConnectGithubPrompt } from '../../../../../components/github/ConnectGithubPrompt'
import { GithubAvatar, PullStateBadge } from '../../../../../components/pulls/PullBits'
import { MergeSheet, ReviewSheet } from '../../../../../components/pulls/PullSheets'
import { PullSummary } from '../../../../../components/pulls/PullSummary'
import { relativeAgo } from '../../../../../data/format'
import { markdownToText } from '../../../../../data/markdown'
import { fileHref } from '../../../../../data/pull-route'
import { githubPromptStatus, isNotFound } from '../../../../../data/pulls-api'
import { usePull, usePullConversation, usePullFiles, usePullThreads, useSetViewed } from '../../../../../hooks/pulls'
import { useTheme } from '../../../../../theme'
import { Button, ErrorView, Icon, IconButton, LoadingView, PaneHeader, PullRefresh, SafeAreaView, Text } from '../../../../../ui'

const REASON: Record<AsideReason, string> = { lockfile: 'Lockfile', generated: 'Generated', snapshot: 'Snapshot' }

/** The web's pull request overlay on a phone, a full-screen modal over the tab it was opened from: the header, the
 *  summary, then the changed files or the conversation. */
export default function PullScreen() {
  const params = useLocalSearchParams<{ owner: string; repo: string; number: string; comment?: string }>()
  const ref = useMemo(() => ({ owner: params.owner, repo: params.repo, number: Number(params.number) }), [params.owner, params.repo, params.number])
  const { colors } = useTheme()
  const pull = usePull(ref)
  const prompt = githubPromptStatus(pull.error)
  return (
    <SafeAreaView edges={['top', 'left', 'right']} className="flex-1 bg-background">
      <PaneHeader className="min-h-12 px-3">
        <IconButton icon="close" label="Close" color={colors.mutedForeground} onPress={() => router.back()} />
        <Text numberOfLines={1} className="flex-1 text-[13px] text-muted-foreground">
          {ref.owner}/{ref.repo} #{ref.number}
        </Text>
        {pull.data ? <IconButton icon="arrowUpRightSquare" label="Open on GitHub" color={colors.mutedForeground} onPress={() => void openURL(pull.data.html_url)} /> : null}
      </PaneHeader>
      {prompt ? (
        <ConnectGithubPrompt status={prompt} />
      ) : isNotFound(pull.error) ? (
        <Text className="p-6 text-center text-[14px] text-muted-foreground">Pull request not found, or your GitHub account cannot see it.</Text>
      ) : pull.isError ? (
        <ErrorView error={pull.error} onRetry={() => void pull.refetch()} />
      ) : !pull.data ? (
        <LoadingView />
      ) : (
        <PullBody pull={pull.data} refetch={pull.refetch} comment={Number(params.comment) || null} />
      )}
    </SafeAreaView>
  )
}

function PullBody({ pull, refetch, comment }: { pull: GithubPullDetail; refetch: () => Promise<unknown>; comment: number | null }) {
  const { colors } = useTheme()
  const ref = useMemo(() => { const [owner = '', repo = ''] = pull.repository.split('/'); return { owner, repo, number: pull.number } }, [pull.repository, pull.number])
  const [tab, setTab] = useState<'files' | 'overview'>('files')
  const [sheet, setSheet] = useState<'review' | 'merge' | null>(null)
  const files = usePullFiles(ref)
  const threads = usePullThreads(ref)
  const conversation = usePullConversation(ref)
  const setViewed = useSetViewed(ref)
  const [asideOpen, setAsideOpen] = useState(false)
  const pending = threads.data?.pending_review?.comment_count ?? 0
  const open = pull.state === 'open' || pull.state === 'draft'
  const allThreads = useMemo(() => threads.data?.threads ?? [], [threads.data])
  const allFiles = useMemo(() => files.data ?? [], [files.data])
  const outside = threadsOutsideDiff(allThreads, allFiles)
  const review = allFiles.filter((file) => asideReason(file.path) === null)
  const aside = allFiles.filter((file) => asideReason(file.path) !== null)
  const summary = useMemo(() => reviewSummary({ files: allFiles, threads: allThreads, timeline: conversation.data ?? [], viewer: pull.viewer.login, author: pull.author.login }), [allFiles, allThreads, conversation.data, pull.viewer.login, pull.author.login])
  // the web's thread count: open threads on the file's lines
  const countOf = (path: string) => allThreads.filter((thread) => thread.path === path && thread.line != null && !thread.is_resolved).length
  const openThread = (thread: GithubReviewThread) => {
    if (threadsOutsideDiff([thread], allFiles).length > 0) setTab('overview')
    else router.push(fileHref(pull, thread.path, thread.id) as never)
  }
  // "N open": the first thread that waits for the member, else the first open one
  const nextThread = () => {
    const unresolved = allThreads.filter((thread) => !thread.is_resolved)
    const next = unresolved.find((thread) => threadWaitsFor(thread, pull.viewer.login, pull.author.login)) ?? unresolved[0]
    if (next) openThread(next)
  }
  // a notification opens its thread's file once
  const opened = useRef(false)
  useEffect(() => {
    if (!comment || opened.current || !threads.data) return
    const thread = threads.data.threads.find((item) => item.comments.some((entry) => entry.database_id === comment))
    if (!thread) return
    opened.current = true
    openThread(thread)
  }, [comment, threads.data, pull])

  return (
    <ScrollView refreshControl={<PullRefresh onRefresh={() => Promise.all([refetch(), files.refetch(), threads.refetch(), conversation.refetch()])} />} contentContainerClassName="gap-4 p-4 pb-10">
      <View className="gap-2">
        <Text className="font-noto-semibold text-[18px] leading-[24px]">{pull.title}</Text>
        <View className="flex-row flex-wrap items-center gap-2">
          <PullStateBadge state={pull.state} />
          <GithubAvatar actor={pull.author} size={16} />
          <Text className="text-[12px] text-muted-foreground">{pull.author.login}</Text>
          <Text className="text-[12px] text-muted-foreground">
            {pull.head_ref} → {pull.base_ref}
          </Text>
        </View>
        <Text className="text-[12px]">
          <Text className="text-[12px] text-emerald-700 dark:text-emerald-400">+{pull.additions}</Text> <Text className="text-[12px] text-red-700 dark:text-red-400">−{pull.deletions}</Text>
        </Text>
        {pull.tasks.length > 0 ? (
          <View className="flex-row flex-wrap gap-1.5">
            {pull.tasks.map((task) => (
              <Pressable key={task.task_id} accessibilityRole="link" onPress={() => router.dismissTo(`/tasks/${task.task_id}`)} className="h-7 flex-row items-center rounded-md border border-border px-2 active:bg-muted">
                <Text numberOfLines={1} className="max-w-56 text-[12px]">
                  {task.title}
                </Text>
              </Pressable>
            ))}
          </View>
        ) : null}
      </View>
      <PullSummary pull={pull} summary={summary} onNextThread={nextThread} />
      {open && (pull.viewer.can_review || pending > 0 || pull.viewer.can_merge) ? (
        <View className="flex-row gap-2">
          {pull.viewer.can_review || pending > 0 ? <Button label={pending > 0 ? `Review (${pending} pending)` : 'Review'} variant={pending > 0 ? 'primary' : 'outline'} onPress={() => setSheet('review')} /> : null}
          {pull.viewer.can_merge ? <Button label="Merge" variant="outline" onPress={() => setSheet('merge')} /> : null}
        </View>
      ) : null}
      <ReviewSheet pullRef={ref} pull={pull} pending={pending} visible={sheet === 'review'} onClose={() => setSheet(null)} />
      <MergeSheet pullRef={ref} pull={pull} visible={sheet === 'merge'} onClose={() => setSheet(null)} />
      <View className="flex-row rounded-lg bg-muted p-0.5">
        {(['files', 'overview'] as const).map((value) => (
          <Pressable key={value} accessibilityRole="tab" accessibilityState={{ selected: tab === value }} onPress={() => setTab(value)} className={`h-8 flex-1 items-center justify-center rounded-md ${tab === value ? 'bg-background' : ''}`}>
            <Text className="font-noto-medium text-[13px]">{value === 'files' ? `Files${files.data ? ` ${files.data.length}` : ''}` : 'Overview'}</Text>
          </Pressable>
        ))}
      </View>
      {tab === 'files' ? (
        files.isError ? (
          <ErrorView error={files.error} onRetry={() => void files.refetch()} />
        ) : !files.data ? (
          <Text className="text-[13px] text-muted-foreground">Loading files…</Text>
        ) : (
          <View className="overflow-hidden rounded-lg border border-border">
            {review.map((file, index) => (
              <FileRow key={file.path} file={file} first={index === 0} threads={countOf(file.path)} onOpen={() => router.push(fileHref(pull, file.path) as never)} onViewed={() => setViewed.mutate({ file: file.path, viewed: !file.viewed })} />
            ))}
            {aside.length > 0 ? (
              <>
                <Pressable accessibilityRole="button" accessibilityState={{ expanded: asideOpen }} accessibilityLabel={`Set aside, ${aside.length} files`} onPress={() => setAsideOpen(!asideOpen)} className={`min-h-10 flex-row items-center gap-1.5 border-border px-3 active:bg-muted ${review.length > 0 ? 'border-t' : ''}`}>
                  <Icon name={asideOpen ? 'chevronDown' : 'chevronRight'} size={12} color={colors.mutedForeground} />
                  <Text className="text-[12px] text-muted-foreground">{`Set aside ${aside.length}`}</Text>
                </Pressable>
                {asideOpen
                  ? aside.map((file) => (
                      <FileRow key={file.path} file={file} first={false} note={REASON[asideReason(file.path) ?? 'generated']} threads={countOf(file.path)} onOpen={() => router.push(fileHref(pull, file.path) as never)} onViewed={() => setViewed.mutate({ file: file.path, viewed: !file.viewed })} />
                    ))
                  : null}
              </>
            ) : null}
          </View>
        )
      ) : (
        <Overview pull={pull} timeline={conversation.data ?? []} outside={outside} />
      )}
    </ScrollView>
  )
}

function FileRow({ file, first, note, threads, onOpen, onViewed }: { file: GithubPullFile; first: boolean; note?: string; threads: number; onOpen: () => void; onViewed: () => void }) {
  const { colors } = useTheme()
  const name = file.path.slice(file.path.lastIndexOf('/') + 1)
  const folder = file.path.includes('/') ? file.path.slice(0, file.path.lastIndexOf('/')) : ''
  return (
    <View className={`min-h-12 flex-row items-center gap-2 border-border px-3 ${first ? '' : 'border-t'}`}>
      <Pressable accessibilityRole="checkbox" accessibilityState={{ checked: file.viewed }} accessibilityLabel={`Viewed ${file.path}`} onPress={onViewed} hitSlop={8} className={`size-5 items-center justify-center rounded border ${file.viewed ? 'border-primary bg-primary' : 'border-border'}`}>
        {file.viewed ? <Icon name="check" size={12} color={colors.primaryForeground} /> : null}
      </Pressable>
      <Pressable accessibilityRole="link" accessibilityLabel={file.path} onPress={onOpen} className="min-w-0 flex-1 py-2 active:opacity-60">
        <Text numberOfLines={1} className={`font-noto-medium text-[13px] ${file.viewed ? 'text-muted-foreground' : 'text-foreground'}`}>
          {name}
        </Text>
        {folder ? (
          <Text numberOfLines={1} className="text-[11px] text-muted-foreground">
            {folder}
          </Text>
        ) : null}
      </Pressable>
      {note ? <Text className="text-[11px] text-muted-foreground">{note}</Text> : null}
      {threads > 0 ? (
        <View accessibilityLabel={`${threads} open ${threads === 1 ? 'thread' : 'threads'}`} className="flex-row items-center gap-0.5">
          <Icon name="messages2" size={12} color={colors.primary} />
          <Text className="text-[11px] text-primary">{threads}</Text>
        </View>
      ) : null}
      <Text className="text-[11px]">
        <Text className="text-[11px] text-emerald-700 dark:text-emerald-400">+{file.additions}</Text> <Text className="text-[11px] text-red-700 dark:text-red-400">−{file.deletions}</Text>
      </Text>
    </View>
  )
}

const REVIEW: Record<string, string> = { approved: 'approved', changes_requested: 'requested changes', commented: 'reviewed' }

function Overview({ pull, timeline, outside }: { pull: GithubPullDetail; timeline: { kind: string; author: { login: string; avatar_url: string }; body: string; created_at: number; review_state?: string | null }[]; outside: GithubReviewThread[] }) {
  return (
    <View className="gap-3">
      <Card login={pull.author.login} avatar={pull.author} meta={null} body={pull.body.trim() ? markdownToText(pull.body) : 'No description.'} />
      {timeline.map((item, index) => (
        <Card key={`${item.created_at}-${index}`} login={item.author.login} avatar={item.author} meta={[item.review_state ? REVIEW[item.review_state] ?? 'reviewed' : null, relativeAgo(new Date(item.created_at).toISOString())].filter(Boolean).join(' · ')} body={markdownToText(item.body)} />
      ))}
      {outside.length > 0 ? <Text className="mt-2 font-noto-semibold text-[12px] text-muted-foreground">Comments not in the diff</Text> : null}
      {outside.map((thread) => (
        <Card key={thread.id} login={thread.comments[0]?.author.login ?? 'ghost'} avatar={thread.comments[0]?.author ?? { login: 'ghost', avatar_url: '' }} meta={thread.path} body={thread.comments.map((entry) => `${entry.author.login}: ${markdownToText(entry.body)}`).join('\n\n')} />
      ))}
    </View>
  )
}

function Card({ login, avatar, meta, body }: { login: string; avatar: { login: string; avatar_url: string }; meta: string | null; body: string }) {
  return (
    <View className="gap-1.5 rounded-lg border border-border p-3">
      <View className="flex-row items-center gap-1.5">
        <GithubAvatar actor={avatar} size={16} />
        <Text className="font-noto-medium text-[12px]">{login}</Text>
        {meta ? <Text className="text-[12px] text-muted-foreground">{meta}</Text> : null}
      </View>
      {body ? <Text className="text-[14px] leading-[20px]">{body}</Text> : null}
    </View>
  )
}
