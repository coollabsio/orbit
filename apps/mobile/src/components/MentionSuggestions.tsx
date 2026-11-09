import { type RefObject, useRef, useState } from 'react'
import { Pressable, ScrollView, type TextInputInstance, type TextInputSelectionChangeEvent, View } from 'react-native'

import { applyMention, insertTrigger, type MentionSuggestion, type MentionTarget, mentionQuery, mentionSuggestions } from '../data/mentions'
import { useTheme } from '../theme'
import { Avatar, Icon, Text } from '../ui'

/**
 * `@` (and `#` when `channels` is set) autocomplete for a plain `TextInput`, as the web's `useMentionAutocomplete`.
 * Spread `inputProps` on the field, show `suggestions` with `MentionSuggestions`, and call `mention` from an `@` button.
 */
export function useMentionField({
  text,
  setText,
  people,
  channels = [],
  inputRef,
}: {
  text: string
  setText: (next: string) => void
  people: readonly MentionTarget[]
  channels?: readonly MentionTarget[]
  /** The field's ref when the caller also needs it (to focus it). */
  inputRef?: RefObject<TextInputInstance | null>
}) {
  const ownRef = useRef<TextInputInstance>(null)
  const ref = inputRef ?? ownRef
  const [cursor, setCursor] = useState(text.length)
  // A picked mention ends in `@Name `, which would still match: it stays closed until the query changes.
  const [closed, setClosed] = useState<{ start: number; query: string } | null>(null)
  const query = mentionQuery(text, cursor, channels.length > 0)
  const shown = query && !(closed && closed.start === query.start && closed.query === query.query) ? query : null
  const suggestions = shown ? mentionSuggestions(shown, people, channels) : []

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
    pick: (suggestion: MentionSuggestion) => {
      if (!shown) return
      const next = applyMention(text, shown, suggestion)
      setClosed({ start: shown.start, query: next.text.slice(shown.start + 1, next.cursor) })
      place(next)
    },
    /** The `@` button: an `@` at the cursor, which opens the list. */
    mention: () => {
      setClosed(null)
      place(insertTrigger(text, cursor))
    },
  }
}

/** The web's mention popup as a list above the field: avatar (or `#`), the name, and the handle. */
export function MentionSuggestions({ suggestions, onPick }: { suggestions: readonly MentionSuggestion[]; onPick: (suggestion: MentionSuggestion) => void }) {
  const { colors } = useTheme()
  if (suggestions.length === 0) return null
  const channels = suggestions[0].kind === 'channel'
  return (
    <View className="max-h-56 border-b border-border py-1.5">
      <Text className="px-3 pb-1 text-[11px] font-noto-bold tracking-wider uppercase text-muted-foreground">{channels ? 'Channels' : 'Mentions'}</Text>
      <ScrollView keyboardShouldPersistTaps="always" nestedScrollEnabled>
        {suggestions.map((suggestion) => (
          <Pressable
            key={suggestion.id}
            accessibilityRole="button"
            accessibilityLabel={`${suggestion.kind === 'channel' ? '#' : '@'}${suggestion.label}`}
            onPress={() => onPick(suggestion)}
            className="flex-row items-center gap-3 px-3 py-2 active:bg-muted"
          >
            {suggestion.kind === 'user' && !suggestion.id.startsWith('!') ? (
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
            <Text numberOfLines={1} className="flex-1 text-[14px] font-noto-semibold">
              {suggestion.kind === 'channel' ? suggestion.label : `@${suggestion.label}`}
            </Text>
            {suggestion.kind === 'user' && suggestion.detail ? (
              <View className="flex-row items-center gap-1 shrink">
                <Icon name="user" size={12} color={colors.mutedForeground} />
                <Text numberOfLines={1} className="text-[12px] font-noto-medium text-muted-foreground">
                  {suggestion.detail}
                </Text>
              </View>
            ) : null}
          </Pressable>
        ))}
      </ScrollView>
    </View>
  )
}
