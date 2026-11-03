import { type ReactNode, useState } from 'react'
import { KeyboardAvoidingView, Modal, Platform, Pressable, ScrollView, View } from 'react-native'
import { TextInput } from '../../ui/scaled'
import { useSafeAreaInsets } from 'react-native-safe-area-context'

import { useTheme } from '../../theme'
import { Icon, type IconName, ListRow, Text } from '../../ui'

/**
 * Which sheet a screen shows. A screen has one sheet whose content changes (a menu row that opens a picker swaps the
 * content in place), so iOS never has to present a modal while another one is closing. The content stays while the
 * sheet slides out.
 */
export function useSheet<S>() {
  const [sheet, setSheet] = useState<S | null>(null)
  const [visible, setVisible] = useState(false)
  return {
    sheet,
    visible,
    open: (next: S) => {
      setSheet(next)
      setVisible(true)
    },
    close: () => setVisible(false),
  }
}

/** The bottom sheet of every task picker (the `PickerSheet` look): a title over the content. Tapping outside closes it. */
export function Sheet({ visible, title, onClose, children }: { visible: boolean; title: string; onClose: () => void; children: ReactNode }) {
  const insets = useSafeAreaInsets()
  return (
    <Modal visible={visible} transparent animationType="slide" onRequestClose={onClose}>
      <KeyboardAvoidingView className="flex-1" behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
        <Pressable accessibilityLabel="Close" onPress={onClose} className="flex-1 bg-foreground dark:bg-background opacity-40" />
        <View
          accessibilityViewIsModal
          className="max-h-[80%] rounded-t-xl border border-border bg-popover" style={{ paddingBottom: insets.bottom + 8 }}
        >
          <Text numberOfLines={1} className="p-4 pb-2 font-noto-semibold text-[17px] leading-[22px]">
            {title}
          </Text>
          {children}
        </View>
      </KeyboardAvoidingView>
    </Modal>
  )
}

/** The rows of a sheet, scrolling under its title and search. */
export function SheetList({ children }: { children: ReactNode }) {
  return (
    <ScrollView keyboardShouldPersistTaps="handled" className="shrink">
      {children}
    </ScrollView>
  )
}

export function SheetSearch({ value, onChange, placeholder, autoFocus = false }: { value: string; onChange: (text: string) => void; placeholder: string; autoFocus?: boolean }) {
  const { colors, scheme } = useTheme()
  return (
    <View className="mx-4 mb-2 flex-row items-center gap-2 h-9 px-2.5 rounded-lg border border-input">
      <Icon name="search" size={16} color={colors.mutedForeground} />
      <TextInput
        value={value}
        onChangeText={onChange}
        placeholder={placeholder}
        accessibilityLabel={placeholder}
        placeholderTextColor={colors.mutedForeground}
        selectionColor={colors.primary}
        keyboardAppearance={scheme}
        autoFocus={autoFocus}
        autoCorrect={false}
        autoCapitalize="none"
        className="flex-1 font-noto text-[16px] leading-[22px] p-0 text-foreground"
      />
    </View>
  )
}

/** One sheet row. `checked` draws the check of a chosen row (single or multi choice). */
export function SheetRow({ label, subtitle, leading, icon, checked, onPress }: { label: string; subtitle?: string; leading?: ReactNode; icon?: IconName; checked?: boolean; onPress: () => void }) {
  const { colors } = useTheme()
  return (
    <ListRow
      title={label}
      subtitle={subtitle}
      leading={leading ?? (icon ? <Icon name={icon} size={18} color={colors.mutedForeground} /> : undefined)}
      trailing={checked ? <Icon name="check" size={18} color={colors.primary} /> : undefined}
      onPress={onPress}
    />
  )
}

/** Quiet text in a sheet: an empty list, a loading search, an error. */
export function SheetNote({ children, tone = 'muted' }: { children: ReactNode; tone?: 'muted' | 'destructive' }) {
  return <Text className={`text-[13px] px-4 py-3 ${tone === 'destructive' ? 'text-destructive' : 'text-muted-foreground'}`}>{children}</Text>
}
