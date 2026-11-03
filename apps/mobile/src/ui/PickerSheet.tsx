import type { ReactNode } from 'react'
import { Modal, Pressable, ScrollView, View } from 'react-native'
import { useSafeAreaInsets } from 'react-native-safe-area-context'

import { useTheme } from '../theme'
import { Icon } from './Icon'
import { ListRow } from './ListRow'
import { Text } from './Text'

export interface PickerOption {
  key: string
  label: string
  subtitle?: string
  leading?: ReactNode
  selected?: boolean
}

/** The sheet's background: `sidebarPopover` for a sheet opened from the drawer, like the web sidebar's menus. */
export type PickerSurface = 'popover' | 'sidebarPopover'

const SURFACE: Record<PickerSurface, string> = { popover: 'bg-popover', sidebarPopover: 'bg-sidebar-popover' }

/** A bottom sheet with one choice per row. Tapping outside closes it. */
export function PickerSheet({
  visible,
  title,
  options,
  onSelect,
  onClose,
  surface = 'popover',
}: {
  visible: boolean
  title: string
  options: readonly PickerOption[]
  onSelect: (key: string) => void
  onClose: () => void
  surface?: PickerSurface
}) {
  const { colors } = useTheme()
  const insets = useSafeAreaInsets()
  return (
    <Modal visible={visible} transparent animationType="slide" onRequestClose={onClose}>
      <Pressable accessibilityLabel="Close" onPress={onClose} className="flex-1 bg-foreground opacity-40 dark:bg-background" />
      <View className={`max-h-[70%] rounded-t-xl border border-border ${SURFACE[surface]}`} style={{ paddingBottom: insets.bottom + 8 }}>
        <Text className="p-4 pb-2 font-noto-semibold text-[17px] leading-[22px]">{title}</Text>
        <ScrollView>
          {options.map((option) => (
            <ListRow
              key={option.key}
              title={option.label}
              subtitle={option.subtitle}
              leading={option.leading}
              trailing={option.selected ? <Icon name="check" size={18} color={colors.primary} /> : undefined}
              onPress={() => onSelect(option.key)}
            />
          ))}
        </ScrollView>
      </View>
    </Modal>
  )
}
