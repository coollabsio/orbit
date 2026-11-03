import type { ReactNode } from 'react'
import { Pressable, View } from 'react-native'

import { cssColors, mix, useTheme } from '../theme'
import { Icon, type IconName } from './Icon'
import { Text } from './Text'

/** The 44px pane header (the web's Tasks, Docs and Settings; Home and Activity are 48px there) on a phone (`pane-header` in global.css); `className` utilities override it. */
export function PaneHeader({ children, className }: { children: ReactNode; className?: string }) {
  return <View className={className ? `pane-header ${className}` : 'pane-header'}>{children}</View>
}

/** `PaneTitle`: 13px semibold. Home's topbar crumb is 14px. */
export function HeaderTitle({ children, size = 13 }: { children: ReactNode; size?: 13 | 14 }) {
  return (
    <Text numberOfLines={1} className={`shrink font-noto-semibold ${size === 14 ? 'text-[14px]' : 'text-[13px]'}`}>
      {children}
    </Text>
  )
}

export interface IconButtonProps {
  icon: IconName
  label: string
  onPress?: () => void
  /** 28 is the web's `icon-sm`, 32 its `icon`, 22 the row pickers. */
  size?: number
  iconSize?: number
  color?: string
  /** `primary` is the pink square with the web button's 2px pressed-edge. */
  variant?: 'ghost' | 'primary'
  disabled?: boolean
}

export function IconButton({ icon, label, onPress, size = 28, iconSize = 16, color, variant = 'ghost', disabled }: IconButtonProps) {
  const { colors } = useTheme()
  const primary = variant === 'primary'
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      disabled={disabled || !onPress}
      onPress={onPress}
      hitSlop={size < 32 ? (32 - size) / 2 + 4 : 4}
      className={`items-center justify-center ${size >= 32 ? 'rounded-lg' : 'rounded-md'} ${primary ? 'bg-primary active:bg-primary/80' : 'active:bg-muted'} ${disabled ? 'opacity-50' : ''}`}
      // the size is a prop, and the primary edge a computed color
      style={({ pressed }) => ({
        width: size,
        height: size,
        ...(primary && { boxShadow: pressed ? undefined : `0 2px 0 ${mix(colors.primary, 0.75, cssColors.black)}`, transform: [{ translateY: pressed ? 2 : 0 }] }),
      })}
    >
      <Icon name={icon} size={iconSize} color={color ?? (primary ? colors.primaryForeground : colors.foreground)} />
    </Pressable>
  )
}
