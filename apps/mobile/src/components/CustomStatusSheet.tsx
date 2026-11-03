import { useState } from 'react'
import { Pressable, View } from 'react-native'

import { canSaveCustomStatus, type ClearAfter, clearAfterOptions, customStatusBody, hasCustomStatus, withoutCustomStatus, type UserStatus } from '../data/status'
import { useOwnStatus, useSetStatus } from '../hooks/account'
import { errorMessage } from '../session-context'
import { alpha, useTheme } from '../theme'
import { Button, Icon, Input, PickerSheet, Text } from '../ui'
import { Emoji, EmojiPicker } from './chat/EmojiPicker'
import { Dialog } from './settings/Dialog'

const formatEnd = (iso: string) => new Date(iso).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })

/**
 * The web's `CustomStatusDialog`: sets or clears the signed-in user's custom status (emoji, text and when it ends). The
 * presence stays as it is. Callers mount it while it is open; a failure keeps it open with what was typed.
 */
export function CustomStatusSheet({ onClose }: { onClose: () => void }) {
  const { colors } = useTheme()
  const status = useOwnStatus()
  const setStatus = useSetStatus({ alertOnError: false })
  const [emoji, setEmoji] = useState(status.emoji)
  const [text, setText] = useState(status.text ?? '')
  // A status that already has an end keeps it unless the user picks another one.
  const [options] = useState(() => clearAfterOptions(status.expiresAt, formatEnd))
  const [clearAfter, setClearAfter] = useState<ClearAfter>(options[0]?.value ?? 'never')
  const [picking, setPicking] = useState<'emoji' | 'clear-after' | null>(null)
  const hadCustom = hasCustomStatus(status)
  const busy = setStatus.isPending

  const save = (body: UserStatus) => {
    if (busy) return
    setStatus.mutate(body, { onSuccess: onClose })
  }
  const close = () => {
    if (!busy) onClose()
  }

  return (
    <Dialog
      visible
      title="Custom status"
      onClose={close}
      footer={
        <>
          <Button label="Clear status" variant="outline" disabled={!hadCustom || busy} onPress={() => save(withoutCustomStatus(status))} />
          <Button label="Save" loading={busy} disabled={busy || !canSaveCustomStatus(status, emoji, text)} onPress={() => save(customStatusBody(status, emoji, text, clearAfter))} />
        </>
      }
    >
      <View className="gap-2">
        <Text className="font-noto-medium text-[14px] leading-[19.25px]">Status</Text>
        <View className="flex-row items-center gap-2">
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={emoji ? 'Change emoji' : 'Add emoji'}
            onPress={() => setPicking('emoji')}
            className="size-8 items-center justify-center rounded-lg border border-input active:bg-muted dark:bg-input/30"
          >
            {emoji ? <Emoji value={emoji} size={18} /> : <Icon name="smile" size={16} color={colors.mutedForeground} />}
          </Pressable>
          <View className="min-w-0 flex-1">
            <Input
              accessibilityLabel="Status"
              autoFocus
              maxLength={100}
              placeholder="What's your status?"
              value={text}
              onChangeText={setText}
              returnKeyType="done"
            />
          </View>
        </View>
        {emoji ? (
          <View className="items-start">
            <Button label="Remove emoji" variant="ghost" size="xs" muted onPress={() => setEmoji(null)} />
          </View>
        ) : null}
      </View>

      <View className="gap-2">
        <Text className="font-noto-medium text-[14px] leading-[19.25px]">Clear after</Text>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={`Clear after: ${options.find((option) => option.value === clearAfter)?.label ?? ''}`}
          onPress={() => setPicking('clear-after')}
          className="min-h-8 flex-row items-center gap-2 rounded-lg border border-input px-2.5 active:bg-muted dark:bg-input/30"
        >
          <Text numberOfLines={1} className="flex-1 text-[14px]">
            {options.find((option) => option.value === clearAfter)?.label}
          </Text>
          <Icon name="chevronDown" size={14} color={alpha(colors.mutedForeground, 0.7)} />
        </Pressable>
      </View>

      {setStatus.isError ? (
        <Text accessibilityRole="alert" className="text-[13px] leading-[19px] text-destructive">
          {`Could not update your status. ${errorMessage(setStatus.error)}`}
        </Text>
      ) : null}

      <EmojiPicker
        visible={picking === 'emoji'}
        onPick={(picked) => {
          setEmoji(picked)
          setPicking(null)
        }}
        onClose={() => setPicking(null)}
      />
      <PickerSheet
        visible={picking === 'clear-after'}
        title="Clear after"
        options={options.map((option) => ({ key: option.value, label: option.label, selected: option.value === clearAfter }))}
        onSelect={(key) => {
          setClearAfter(key as ClearAfter)
          setPicking(null)
        }}
        onClose={() => setPicking(null)}
      />
    </Dialog>
  )
}
