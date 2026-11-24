import { useRef, useState } from 'react'
import { Modal, Pressable, useWindowDimensions, View, type ViewInstance } from 'react-native'

import type { SheetAction } from '../components/settings/Dialog'
import { useTheme } from '../theme'
import { IconButton, type IconButtonProps } from './Header'
import { Icon } from './Icon'
import { Text } from './Text'

const GAP = 4
const EDGE = 8

/**
 * The web `DropdownMenu` of an icon button: a small menu under the button, its right edge on the button's. For a
 * short menu of a header button; a row's own long-press menu stays an `ActionSheet`. A row closes the menu, then runs.
 */
export function DropdownMenu({ actions, ...trigger }: Omit<IconButtonProps, 'onPress' | 'onLongPress'> & { actions: readonly SheetAction[] }) {
  const { colors } = useTheme()
  const anchorRef = useRef<ViewInstance>(null)
  const [anchor, setAnchor] = useState<{ y: number; right: number } | null>(null)
  const { width: screenWidth } = useWindowDimensions()
  const open = () => anchorRef.current?.measureInWindow((x, y, width, height) => setAnchor({ y: y + height + GAP, right: Math.max(EDGE, screenWidth - x - width) }))
  const close = () => setAnchor(null)
  return (
    <>
      <View ref={anchorRef} collapsable={false}>
        <IconButton {...trigger} onPress={open} />
      </View>
      <Modal visible={anchor !== null} transparent animationType="fade" statusBarTranslucent navigationBarTranslucent onRequestClose={close}>
        <Pressable accessibilityLabel="Close" onPress={close} className="flex-1">
          {anchor ? (
            <View accessibilityRole="menu" className="min-w-44 rounded-lg border border-border bg-popover p-1 shadow-md" style={{ position: 'absolute', top: anchor.y, right: anchor.right }}>
              {actions.map((action) => (
                <Pressable
                  key={action.label}
                  accessibilityRole="menuitem"
                  accessibilityState={{ disabled: action.disabled }}
                  disabled={action.disabled}
                  onPress={() => {
                    close()
                    action.onPress()
                  }}
                  className={`h-9 flex-row items-center gap-2 rounded-md px-2 active:bg-muted ${action.disabled ? 'opacity-50' : ''}`}
                >
                  {action.icon ? <Icon name={action.icon} size={16} color={action.destructive ? colors.destructive : colors.mutedForeground} /> : null}
                  <Text className={`text-[14px] ${action.destructive ? 'text-destructive' : 'text-popover-foreground'}`}>{action.label}</Text>
                </Pressable>
              ))}
            </View>
          ) : null}
        </Pressable>
      </Modal>
    </>
  )
}
