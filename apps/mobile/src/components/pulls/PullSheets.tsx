import type { GithubPullDetail, GithubReviewThread } from '@orbit/api/generated/types.gen'
import { type CommentTarget, draftKey, mergeDisabledReason, pickMergeMethod, type PullRef, replyDraftKey, squashDefaults, type MergeMethod } from '@orbit/api/github'
import { ApiProblem } from '@orbit/api/problem'
import { type ReactNode, useState } from 'react'
import { Alert, Modal, Pressable, ScrollView, View } from 'react-native'

import { clearDraft, readDraft, writeDraft } from '../../data/pull-drafts'
import { githubMessage } from '../../data/pulls-api'
import { markdownToText } from '../../data/markdown'
import { relativeAgo } from '../../data/format'
import { useQueryClient } from '@tanstack/react-query'

import { pullKeys } from '../../data/pulls-api'
import { useAddComment, useDeleteComment, useDeleteBranch, useDiscardReview, useMerge, useReply, useResolveThread, useSubmitReview } from '../../hooks/pulls'
import { useOrbit } from '../../orbit-context'
import { useTheme } from '../../theme'
import { Button, IconButton, KeyboardSpace, PaneHeader, SafeAreaView, Text } from '../../ui'
import { TextInput } from '../../ui/scaled'
import { GithubAvatar } from './PullBits'

/** A page sheet with a title and a close button (the app's pattern, see `docs/Comments.tsx`). */
function PageSheet({ title, visible, onClose, children }: { title: string; visible: boolean; onClose: () => void; children: ReactNode }) {
  const { colors } = useTheme()
  return (
    <Modal visible={visible} animationType="slide" presentationStyle="pageSheet" onRequestClose={onClose}>
      <SafeAreaView edges={['top', 'bottom', 'left', 'right']} className="flex-1 bg-background">
        <PaneHeader className="border-b border-border">
          <Text numberOfLines={1} className="flex-1 pl-1 font-noto-medium text-[13.5px]">
            {title}
          </Text>
          <IconButton icon="close" label="Close" color={colors.mutedForeground} onPress={onClose} />
        </PaneHeader>
        <KeyboardSpace className="flex-1">{children}</KeyboardSpace>
      </SafeAreaView>
    </Modal>
  )
}

function Field({ label, value, onChangeText, minHeight = 96, autoFocus = false }: { label: string; value: string; onChangeText: (value: string) => void; minHeight?: number; autoFocus?: boolean }) {
  const { colors } = useTheme()
  return (
    <TextInput
      accessibilityLabel={label}
      placeholder={label}
      placeholderTextColor={colors.mutedForeground}
      value={value}
      onChangeText={onChangeText}
      autoFocus={autoFocus}
      multiline
      textAlignVertical="top"
      style={{ minHeight }}
      className="rounded-lg border border-input px-3 py-2 font-noto text-[15px] leading-[1.45] text-foreground"
    />
  )
}

/** A draft that stays (in memory) until it is posted or discarded. */
function useDraft(key: string): [string, (value: string) => void, () => void] {
  const [value, setValue] = useState(() => readDraft(key))
  return [
    value,
    (next) => {
      setValue(next)
      writeDraft(key, next)
    },
    () => {
      setValue('')
      clearDraft(key)
    },
  ]
}

const lines = (target: CommentTarget) => (target.start_line != null && target.start_line !== target.line ? `lines ${target.start_line} to ${target.line}` : `line ${target.line}`)

/** The web's comment box: Add to review (pending) or Comment now. */
export function CommentSheet({ pullRef, headSha, reviewing, target, onClose }: { pullRef: PullRef; headSha: string; reviewing: boolean; target: CommentTarget | null; onClose: () => void }) {
  const { workspaceId } = useOrbit()
  return (
    <PageSheet title={target ? `Comment on ${lines(target)}` : 'Comment'} visible={target !== null} onClose={onClose}>
      {target ? <CommentForm key={draftKey(workspaceId, pullRef, target)} pullRef={pullRef} headSha={headSha} reviewing={reviewing} target={target} onDone={onClose} /> : null}
    </PageSheet>
  )
}

function CommentForm({ pullRef, headSha, reviewing, target, onDone }: { pullRef: PullRef; headSha: string; reviewing: boolean; target: CommentTarget; onDone: () => void }) {
  const { workspaceId } = useOrbit()
  const [text, setText, clear] = useDraft(draftKey(workspaceId, pullRef, target))
  const add = useAddComment(pullRef)
  const post = (mode: 'pending' | 'single') =>
    add.mutate(
      { target, body: text, mode, headSha },
      {
        onSuccess: () => {
          clear()
          onDone()
        },
      },
    )
  return (
    <View className="gap-3 p-4">
      <Text className="text-[12px] text-muted-foreground">{target.path}</Text>
      <Field label={`Comment on ${lines(target)}`} value={text} onChangeText={setText} autoFocus />
      {add.isError ? <Text className="text-[12px] text-destructive">{githubMessage(add.error, 'GitHub did not take the comment. Try again.')}</Text> : null}
      <View className="flex-row flex-wrap gap-2">
        <Button label="Add to review" disabled={!text.trim()} loading={add.isPending} onPress={() => post('pending')} />
        {/* GitHub allows one pending review: while it is open, comments go into it */}
        {reviewing ? null : <Button label="Comment now" variant="outline" disabled={!text.trim() || add.isPending} onPress={() => post('single')} />}
        {text ? (
          <Button
            label="Discard"
            variant="ghost"
            onPress={() => {
              clear()
              onDone()
            }}
          />
        ) : null}
      </View>
    </View>
  )
}

/** One thread: its comments, reply, resolve, and delete for own comments. */
export function ThreadSheet({ pullRef, thread, onClose }: { pullRef: PullRef; thread: GithubReviewThread | null; onClose: () => void }) {
  return (
    <PageSheet title={thread ? thread.path.slice(thread.path.lastIndexOf('/') + 1) : 'Thread'} visible={thread !== null} onClose={onClose}>
      {thread ? <ThreadBody key={thread.id} pullRef={pullRef} thread={thread} /> : null}
    </PageSheet>
  )
}

function ThreadBody({ pullRef, thread }: { pullRef: PullRef; thread: GithubReviewThread }) {
  const { workspaceId } = useOrbit()
  const [text, setText, clear] = useDraft(replyDraftKey(workspaceId, pullRef, thread.id))
  const reply = useReply(pullRef)
  const resolve = useResolveThread(pullRef)
  const remove = useDeleteComment(pullRef)
  return (
    <>
      <ScrollView className="flex-1" contentContainerClassName="gap-3 p-4" keyboardShouldPersistTaps="handled">
        {thread.is_resolved ? <Text className="text-[12px] text-muted-foreground">Resolved</Text> : null}
        {thread.comments.map((comment) => (
          <View key={comment.id} className="gap-1 rounded-lg border border-border p-3">
            <View className="flex-row items-center gap-1.5">
              <GithubAvatar actor={comment.author} size={16} />
              <Text className="font-noto-medium text-[12px]">{comment.author.login}</Text>
              <Text className="text-[12px] text-muted-foreground">{relativeAgo(new Date(comment.created_at).toISOString())}</Text>
              {comment.pending ? <Text className="text-[11px] text-amber-700 dark:text-amber-400">Pending</Text> : null}
              <View className="flex-1" />
              {comment.can_edit ? (
                <Pressable
                  accessibilityRole="button"
                  accessibilityLabel="Delete comment"
                  hitSlop={8}
                  onPress={() =>
                    Alert.alert('Delete this comment?', 'It is deleted on GitHub too.', [
                      { text: 'Cancel', style: 'cancel' },
                      { text: 'Delete', style: 'destructive', onPress: () => remove.mutate(comment.id) },
                    ])
                  }
                >
                  <Text className="text-[12px] text-destructive">Delete</Text>
                </Pressable>
              ) : null}
            </View>
            <Text className="text-[14px] leading-[20px]">{markdownToText(comment.body)}</Text>
          </View>
        ))}
        {remove.isError || resolve.isError ? <Text className="text-[12px] text-destructive">{githubMessage(remove.error ?? resolve.error, 'GitHub did not take the change.')}</Text> : null}
      </ScrollView>
      <View className="gap-2 border-t border-border p-3">
        {thread.can_reply ? <Field label="Reply to the thread" value={text} onChangeText={setText} minHeight={56} /> : null}
        {reply.isError ? <Text className="text-[12px] text-destructive">{githubMessage(reply.error, 'GitHub did not take the reply. Try again.')}</Text> : null}
        <View className="flex-row gap-2">
          {thread.can_reply ? <Button label="Send reply" disabled={!text.trim()} loading={reply.isPending} onPress={() => reply.mutate({ threadId: thread.id, body: text }, { onSuccess: clear })} /> : null}
          <View className="flex-1" />
          {thread.can_resolve ? <Button label={thread.is_resolved ? 'Unresolve' : 'Resolve'} variant="outline" loading={resolve.isPending} onPress={() => resolve.mutate({ threadId: thread.id, resolved: !thread.is_resolved })} /> : null}
        </View>
      </View>
    </>
  )
}

type Event = 'COMMENT' | 'APPROVE' | 'REQUEST_CHANGES'
const EVENTS: { value: Event; label: string; hint: string }[] = [
  { value: 'COMMENT', label: 'Comment', hint: 'Feedback without approval.' },
  { value: 'APPROVE', label: 'Approve', hint: 'Give your approval to merge.' },
  { value: 'REQUEST_CHANGES', label: 'Request changes', hint: 'Feedback that must be addressed first.' },
]

/** The web's Review panel. */
export function ReviewSheet({ pullRef, pull, pending, visible, onClose }: { pullRef: PullRef; pull: GithubPullDetail; pending: number; visible: boolean; onClose: () => void }) {
  const { colors } = useTheme()
  const [event, setEvent] = useState<Event>('COMMENT')
  const [body, setBody] = useState('')
  const submit = useSubmitReview(pullRef)
  const discard = useDiscardReview(pullRef)
  const events = EVENTS.filter((item) => !pull.viewer.is_author || item.value === 'COMMENT')
  const empty = event !== 'APPROVE' && !body.trim() && pending === 0
  return (
    <PageSheet title="Review" visible={visible} onClose={onClose}>
      <ScrollView contentContainerClassName="gap-3 p-4" keyboardShouldPersistTaps="handled">
        <Field label="Review summary" value={body} onChangeText={setBody} />
        {events.map((item) => (
          <Pressable key={item.value} accessibilityRole="radio" accessibilityState={{ checked: event === item.value }} accessibilityLabel={item.label} onPress={() => setEvent(item.value)} className="flex-row items-start gap-2.5 rounded-lg p-2 active:bg-muted">
            <View className={`mt-0.5 size-4 items-center justify-center rounded-full border ${event === item.value ? 'border-primary' : 'border-border'}`}>{event === item.value ? <View className="size-2 rounded-full bg-primary" /> : null}</View>
            <View className="flex-1">
              <Text className="font-noto-medium text-[14px]">{item.label}</Text>
              <Text className="text-[12px] text-muted-foreground">{item.hint}</Text>
            </View>
          </Pressable>
        ))}
        {pending > 0 ? <Text className="text-[12px] text-muted-foreground">{`${pending} pending ${pending === 1 ? 'comment goes' : 'comments go'} with the review.`}</Text> : null}
        {empty ? <Text className="text-[12px] text-muted-foreground">Write a summary or add comments first.</Text> : null}
        {submit.isError ? <Text className="text-[12px] text-destructive">{githubMessage(submit.error, 'GitHub did not take the review. Try again.')}</Text> : null}
        <Button
          label="Submit review"
          disabled={empty}
          loading={submit.isPending}
          onPress={() =>
            submit.mutate(
              { event, body },
              {
                onSuccess: () => {
                  setBody('')
                  onClose()
                },
              },
            )
          }
        />
        {pending > 0 ? (
          <Pressable
            accessibilityRole="button"
            onPress={() =>
              Alert.alert('Discard your pending review?', 'Your pending comments are deleted on GitHub too.', [
                { text: 'Cancel', style: 'cancel' },
                { text: 'Discard', style: 'destructive', onPress: () => discard.mutate(undefined, { onSuccess: onClose }) },
              ])
            }
            className="items-center py-2"
          >
            <Text className="text-[13px]" style={{ color: colors.destructive }}>
              Discard pending review
            </Text>
          </Pressable>
        ) : null}
      </ScrollView>
    </PageSheet>
  )
}

const LABEL: Record<MergeMethod, string> = { merge: 'Create a merge commit', squash: 'Squash and merge', rebase: 'Rebase and merge' }

/** The web's Merge button as a sheet: the allowed methods, the commit for merge and squash, then merge. */
export function MergeSheet({ pullRef, pull, visible, onClose }: { pullRef: PullRef; pull: GithubPullDetail; visible: boolean; onClose: () => void }) {
  const [method, setMethod] = useState<MergeMethod | null>(() => pickMergeMethod(pull.merge, null))
  const [commit, setCommit] = useState(() => squashDefaults(pull))
  const merge = useMerge(pullRef)
  const deleteBranch = useDeleteBranch(pullRef)
  const queryClient = useQueryClient()
  const { workspaceId } = useOrbit()
  const reason = mergeDisabledReason(pull)
  const methods = pull.merge.methods.filter((item): item is MergeMethod => item === 'merge' || item === 'squash' || item === 'rebase')
  const headChanged = merge.error instanceof ApiProblem && merge.error.code === 'pull_head_changed'
  const run = () => {
    if (!method) return
    const message = method === 'rebase' ? {} : { commit_title: commit.title, commit_message: commit.message }
    merge.mutate(
      { method, ...message, expected_head_sha: pull.head_sha },
      {
        onSuccess: () => {
          onClose()
          Alert.alert('Merged', `${pull.head_ref} is merged into ${pull.base_ref}.`, [
            { text: 'Keep branch', style: 'cancel' },
            { text: 'Delete branch', style: 'destructive', onPress: () => deleteBranch.mutate(undefined, { onError: (error) => Alert.alert('Branch not deleted', githubMessage(error, 'GitHub did not delete the branch.')) }) },
          ])
        },
      },
    )
  }
  return (
    <PageSheet title="Merge" visible={visible} onClose={onClose}>
      <ScrollView contentContainerClassName="gap-3 p-4" keyboardShouldPersistTaps="handled">
        {reason ? <Text className="text-[13px] text-muted-foreground">{reason}</Text> : null}
        {methods.map((item) => (
          <Pressable key={item} accessibilityRole="radio" accessibilityState={{ checked: method === item }} accessibilityLabel={LABEL[item]} onPress={() => { setMethod(item); if (item === 'squash') setCommit(squashDefaults(pull)) }} className="flex-row items-center gap-2.5 rounded-lg p-2 active:bg-muted">
            <View className={`size-4 items-center justify-center rounded-full border ${method === item ? 'border-primary' : 'border-border'}`}>{method === item ? <View className="size-2 rounded-full bg-primary" /> : null}</View>
            <Text className="text-[14px]">{LABEL[item]}</Text>
          </Pressable>
        ))}
        {method && method !== 'rebase' ? (
          <>
            <Field label="Commit title" value={commit.title} onChangeText={(title) => setCommit({ ...commit, title })} minHeight={40} />
            <Field label="Commit message" value={commit.message} onChangeText={(message) => setCommit({ ...commit, message })} />
          </>
        ) : null}
        {merge.isError ? <Text className="text-[12px] text-destructive">{headChanged ? 'New commits were pushed. Refresh before you merge.' : githubMessage(merge.error, 'GitHub did not merge. Try again.')}</Text> : null}
        {headChanged ? (
          <Button
            label="Refresh"
            variant="outline"
            onPress={() => {
              merge.reset()
              void queryClient.invalidateQueries({ queryKey: pullKeys.pull(workspaceId, pullRef) })
              onClose()
            }}
          />
        ) : null}
        <Button label={method ? LABEL[method] : 'Merge'} disabled={reason !== null || !method || (method !== 'rebase' && !commit.title.trim())} loading={merge.isPending} onPress={run} />
      </ScrollView>
    </PageSheet>
  )
}
