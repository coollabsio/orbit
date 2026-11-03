import type { ReactNode } from 'react'
import { KeyboardAvoidingView, Modal, Platform, Pressable, ScrollView, View } from 'react-native'
import { useSafeAreaInsets } from 'react-native-safe-area-context'

import { useTheme } from '../../theme'
import { Button, Icon, type IconName, SafeAreaView, Text } from '../../ui'

export interface SheetItem {
  key: string
  label: string
  icon?: IconName
  destructive?: boolean
  /** A check at the end: the current choice of a radio list. */
  selected?: boolean
  disabled?: boolean
  onPress: () => void
}

/**
 * The web's phone action sheet (`MessageActionSheet`): it rises from the bottom on the popover surface, with an optional
 * block above the rows (the quick reactions). A row closes the sheet, then runs.
 */
export function ActionSheet({ visible, title, header, items, onClose }: { visible: boolean; title?: string; header?: ReactNode; items: readonly SheetItem[]; onClose: () => void }) {
  const { colors } = useTheme()
  const insets = useSafeAreaInsets()
  return (
    <Modal visible={visible} transparent animationType="slide" onRequestClose={onClose}>
      <Pressable accessibilityLabel="Close" onPress={onClose} className="flex-1 bg-foreground/40 dark:bg-black/40" />
      <View
        accessibilityViewIsModal
        className="max-h-[80%] bg-popover rounded-t-[12px] border-t border-border p-2" style={{ paddingBottom: Math.max(8, insets.bottom) }}
      >
        {title ? (
          <Text numberOfLines={1} className="font-noto-semibold text-[13px] px-3 pt-1 pb-2 text-muted-foreground">
            {title}
          </Text>
        ) : null}
        {header}
        <ScrollView bounces={false}>
          <View className={`${header ? 'pt-1' : 'pt-0'}`}>
            {items.map((item) => (
              <Pressable
                key={item.key}
                accessibilityRole="button"
                accessibilityLabel={item.label}
                accessibilityState={{ disabled: item.disabled, selected: item.selected }}
                disabled={item.disabled}
                onPress={() => {
                  onClose()
                  item.onPress()
                }}
                className={`h-11 flex-row items-center gap-3 px-3 rounded-md active:bg-muted ${item.disabled ? 'opacity-50' : 'opacity-100'}`}
              >
                {item.icon ? <Icon name={item.icon} size={20} color={item.destructive ? colors.destructive : colors.foreground} /> : null}
                <Text className={`text-[14px] flex-1 ${item.destructive ? 'text-destructive' : 'text-popover-foreground'}`}>{item.label}</Text>
                {item.selected ? <Icon name="check" size={18} color={colors.primary} /> : null}
              </Pressable>
            ))}
          </View>
        </ScrollView>
      </View>
    </Modal>
  )
}

/**
 * A dialog of the web as a phone sheet: Cancel, the title and the main action in a header, the form below. iOS shows it
 * as a page sheet that slides down to close.
 */
export function FormSheet({
  visible,
  title,
  description,
  submitLabel,
  canSubmit = true,
  busy = false,
  onSubmit,
  onClose,
  children,
}: {
  visible: boolean
  title: string
  description?: string
  /** Without it the sheet has only Done (Close). */
  submitLabel?: string
  canSubmit?: boolean
  busy?: boolean
  onSubmit?: () => void
  onClose: () => void
  children: ReactNode
}) {
  return (
    <Modal visible={visible} animationType="slide" presentationStyle="pageSheet" onRequestClose={onClose}>
      <SafeAreaView edges={Platform.OS === 'ios' ? ['bottom', 'left', 'right'] : ['top', 'bottom', 'left', 'right']} className="flex-1 bg-popover">
        <View className="h-13 flex-row items-center gap-2 px-3 border-b border-border">
          <View className="flex-1 items-start">
            <Button label={submitLabel ? 'Cancel' : 'Close'} variant="ghost" onPress={onClose} />
          </View>
          <Text numberOfLines={1} accessibilityRole="header" className="font-noto-semibold text-[15px] shrink">
            {title}
          </Text>
          <View className="flex-1 items-end">
            {submitLabel && onSubmit ? <Button label={submitLabel} size="sm" loading={busy} disabled={!canSubmit} onPress={onSubmit} /> : null}
          </View>
        </View>
        <KeyboardAvoidingView className="flex-1" behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
          <ScrollView keyboardShouldPersistTaps="handled" contentContainerClassName="p-4 gap-4">
            {description ? <Text className="text-[14px] leading-[20px] text-muted-foreground">{description}</Text> : null}
            {children}
          </ScrollView>
        </KeyboardAvoidingView>
      </SafeAreaView>
    </Modal>
  )
}

/** A label over a row of choices, each with a line under its name: the web's Visibility toggle group. */
export function ChoiceGroup<T extends string>({ label, value, options, onChange }: { label: string; value: T; options: readonly { value: T; label: string; description: string; icon: IconName }[]; onChange: (value: T) => void }) {
  const { colors } = useTheme()
  return (
    <View className="gap-2">
      <Text className="font-noto-medium text-[14px] leading-[19.25px]">{label}</Text>
      <View className="rounded-lg border border-input overflow-hidden">
        {options.map((option, index) => {
          const selected = option.value === value
          return (
            <Pressable
              key={option.value}
              accessibilityRole="radio"
              accessibilityState={{ selected }}
              accessibilityLabel={`${option.label}. ${option.description}`}
              onPress={() => onChange(option.value)}
              className={`px-3 py-2 gap-0.5 border-input ${index === 0 ? 'border-t-0' : 'border-t'} ${selected ? 'bg-muted' : ''}`}
            >
              <View className="flex-row items-center gap-1.5">
                <Icon name={option.icon} size={16} color={colors.foreground} />
                <Text className="font-noto-medium text-[14px] flex-1">{option.label}</Text>
                {selected ? <Icon name="check" size={16} color={colors.primary} /> : null}
              </View>
              <Text className="text-[12px] leading-[16px] text-muted-foreground">{option.description}</Text>
            </Pressable>
          )
        })}
      </View>
    </View>
  )
}
