import { type RefObject, useRef, useState } from 'react'
import { Pressable, ScrollView, type TextInputInstance, type TextInputSelectionChangeEvent, View } from 'react-native'

import {
  applyMention,
  insertTrigger,
  type MentionSuggestion,
  type MentionTarget,
  mentionQuery,
  mentionSuggestions,
  mixSuggestions,
  rankTaskSuggestions,
  taskSearchText,
} from '../data/mentions'
import { identifier, taskCandidates } from '../data/tasks'
import { useTaskSearch } from '../hooks/tasks'
import { useProjects, useStatuses } from '../queries'
import { useTheme } from '../theme'
import { Avatar, Icon, Spinner, StatusIcon, Text } from '../ui'

/** Task hits shown for one query. */
const TASK_LIMIT = 8
const SECTION: Record<MentionSuggestion['kind'], string> = { user: 'People', channel: 'Channels', task: 'Tasks' }

/**
 * `@` (and `#` when `channels` is set) autocomplete for a plain `TextInput`, as the web's `useMentionAutocomplete`.
 * Spread `inputProps` on the field, show `suggestions` with `MentionSuggestions`, and call `mention` from an `@` or `#` button.
 */
export function useMentionField({
  text,
  setText,
  people,
  channels = [],
  inputRef,
  tasks = false,
}: {
  text: string
  setText: (next: string) => void
  people: readonly MentionTarget[]
  channels?: readonly MentionTarget[]
  /** The field's ref when the caller also needs it (to focus it). */
  inputRef?: RefObject<TextInputInstance | null>
  /** "#" also finds tasks by title or identifier (recently updated ones first), and a pick inserts the identifier (`ENG-12`). */
  tasks?: boolean
}) {
  const ownRef = useRef<TextInputInstance>(null)
  const ref = inputRef ?? ownRef
  const [cursor, setCursor] = useState(text.length)
  // A picked mention ends in `@Name `, which would still match: it stays closed until the query changes.
  const [closed, setClosed] = useState<{ start: number; query: string } | null>(null)
  const query = mentionQuery(text, cursor, channels.length > 0 || tasks)
  const shown = query && !(closed && closed.start === query.start && closed.query === query.query) ? query : null
  const taskText = tasks && shown?.trigger === '#' ? taskSearchText(shown.query) : null
  const search = useTaskSearch(taskText ?? '', taskText !== null)
  // only a field that offers tasks needs them
  const projects = useProjects(tasks).data ?? []
  const statuses = useStatuses(tasks).data ?? []
  // the last hits stay while the next search loads, but never once the "#" is gone
  const taskSuggestions = taskText !== null
    ? rankTaskSuggestions(
        taskCandidates(search.data ?? [], projects, { query: taskText.replace(/^#/, ''), excludeIds: [], limit: 100 }).map((task) => ({
          id: `task_${task.id}`,
          label: task.title || 'Untitled',
          detail: identifier(task, projects),
          kind: 'task' as const,
          status: statuses.find((status) => status.id === task.status_id),
          project: projects.find((project) => project.id === task.project_id)?.name,
        })),
        taskText,
        // rank all the hits first: an older task with the word in its title comes before newer description hits
      ).slice(0, TASK_LIMIT)
    : []
  // a task search is still on its way: the list says so, also before anything matches
  const searchingTasks = taskText !== null && (search.isPending || search.isPlaceholderData)
  const suggestions = shown ? mixSuggestions(mentionSuggestions(shown, people, channels), taskSuggestions, shown.query) : []

  const place = (next: { text: string; cursor: number }) => {
    setText(next.text)
    setCursor(next.cursor)
    // The field takes the new value first, then the cursor.
    requestAnimationFrame(() => {
      ref.current?.focus()
      ref.current?.setSelection(next.cursor, next.cursor)
    })
  }

  return {
    inputProps: {
      ref,
      onSelectionChange: (event: TextInputSelectionChangeEvent) => setCursor(event.nativeEvent.selection.end),
    },
    suggestions,
    searchingTasks,
    pick: (suggestion: MentionSuggestion) => {
      if (!shown) return
      const next = applyMention(text, shown, suggestion)
      setClosed({ start: shown.start, query: next.text.slice(shown.start + 1, next.cursor) })
      place(next)
    },
    /** The `@` and `#` buttons: the trigger at the cursor, which opens the list. */
    mention: (trigger: '@' | '#') => {
      setClosed(null)
      place(insertTrigger(text, cursor, trigger))
    },
  }
}

/**
 * The web's mention popup as a list above the field: people (avatar, name, handle), tasks (status, ID, title, project)
 * and channels, each under its heading.
 */
export function MentionSuggestions({
  suggestions,
  onPick,
  loading = false,
}: {
  suggestions: readonly MentionSuggestion[]
  onPick: (suggestion: MentionSuggestion) => void
  /** A task search is still running: the rows (the last hits) dim under a spinner, so the list keeps its size. */
  loading?: boolean
}) {
  const { colors } = useTheme()
  if (suggestions.length === 0 && !loading) return null
  return (
    <View className="max-h-56 border-b border-border py-1.5">
      <ScrollView keyboardShouldPersistTaps="always" nestedScrollEnabled style={{ opacity: loading && suggestions.length > 0 ? 0.6 : 1 }}>
        {suggestions.map((suggestion, index) => (
          <View key={suggestion.id}>
            {index === 0 || SECTION[suggestions[index - 1]!.kind] !== SECTION[suggestion.kind] ? (
              <View className={`flex-row items-center gap-1.5 px-3 pb-1${index > 0 ? ' pt-1.5' : ''}`}>
                <Text className="text-[11px] font-noto-bold tracking-wider uppercase text-muted-foreground">{SECTION[suggestion.kind]}</Text>
                {/* the spinner sits by the "Tasks" heading (by the first heading while no task is listed) */}
                {loading && (suggestion.kind === 'task' || (index === 0 && !suggestions.some((item) => item.kind === 'task'))) ? (
                  // scaled to the heading's size, so the heading does not grow while it spins
                  <View className="h-3.5 justify-center" style={{ transform: [{ scale: 0.6 }] }}>
                    <Spinner />
                  </View>
                ) : null}
              </View>
            ) : null}
            <Pressable
              accessibilityRole="button"
              accessibilityLabel={suggestion.kind === 'task' ? `${suggestion.detail} ${suggestion.label}` : `${suggestion.kind === 'channel' ? '#' : '@'}${suggestion.label}`}
              onPress={() => onPick(suggestion)}
              className="flex-row items-center gap-3 px-3 py-2 active:bg-muted"
            >
              {suggestion.kind === 'task' ? (
                <>
                  <View className="size-7 items-center justify-center">
                    <StatusIcon status={suggestion.status} size={14} />
                  </View>
                  <Text className="text-[12px] font-noto-medium text-muted-foreground">{suggestion.detail}</Text>
                  <Text numberOfLines={1} className="flex-1 text-[14px] font-noto-semibold">
                    {suggestion.label}
                  </Text>
                  {suggestion.project ? (
                    <Text numberOfLines={1} className="max-w-[35%] text-[12px] text-muted-foreground">
                      {suggestion.project}
                    </Text>
                  ) : null}
                </>
              ) : suggestion.kind === 'user' && !suggestion.id.startsWith('!') ? (
                <Avatar id={suggestion.id} name={suggestion.label} size={28} />
              ) : (
                <View className="size-7 rounded-full items-center justify-center bg-muted">
                  {suggestion.kind === 'channel' ? (
                    <Icon name="hashtag" size={13} color={colors.mutedForeground} />
                  ) : (
                    <Text className="text-[12px] font-noto-bold text-muted-foreground">{suggestion.label.charAt(0).toUpperCase()}</Text>
                  )}
                </View>
              )}
              {suggestion.kind === 'task' ? null : (
                <Text numberOfLines={1} className="flex-1 text-[14px] font-noto-semibold">
                  {suggestion.kind === 'channel' ? suggestion.label : `@${suggestion.label}`}
                </Text>
              )}
              {suggestion.kind === 'user' && suggestion.detail ? (
                <View className="flex-row items-center gap-1 shrink">
                  <Icon name="user" size={12} color={colors.mutedForeground} />
                  <Text numberOfLines={1} className="text-[12px] font-noto-medium text-muted-foreground">
                    {suggestion.detail}
                  </Text>
                </View>
              ) : null}
            </Pressable>
          </View>
        ))}
        {loading && suggestions.length === 0 ? (
          <View className="flex-row items-center gap-2 px-3 py-1.5">
            <Spinner />
            <Text className="text-[12px] text-muted-foreground">Searching tasks…</Text>
          </View>
        ) : null}
      </ScrollView>
    </View>
  )
}
