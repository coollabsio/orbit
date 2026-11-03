import { Pressable, View } from 'react-native'

import { alpha, cssColors, mix, useButtonStyle, useTheme } from '../theme'
import { Icon, type IconName } from './Icon'
import { Spinner } from './Spinner'
import { Text } from './Text'

export type ButtonVariant = 'primary' | 'outline' | 'secondary' | 'ghost' | 'destructive'
/** The web's `default` (32), `sm` (28) and `xs` (24) heights; `lg` (44) is the touch size of forms like sign-in. */
export type ButtonSize = 'xs' | 'sm' | 'default' | 'lg'

const SIZE: Record<ButtonSize, { box: string; text: string }> = {
  xs: { box: 'h-6 rounded-md px-2', text: 'text-[12px] leading-[17px]' },
  sm: { box: 'h-7 rounded-md px-2.5', text: 'text-[12.8px] leading-[18px]' },
  default: { box: 'h-8 rounded-lg px-2.5', text: 'text-[14px] leading-[20px]' },
  lg: { box: 'h-11 rounded-lg px-2.5', text: 'text-[15px] leading-[21px]' },
}

const VARIANT: Record<ButtonVariant, { box: string; text: string }> = {
  primary: { box: 'border-transparent bg-primary active:bg-primary/80', text: 'text-primary-foreground' },
  outline: { box: 'border-border bg-background active:bg-muted dark:border-input dark:bg-input/30 dark:active:bg-input/50', text: 'text-foreground' },
  // pressed: `color-mix` of secondary and foreground, a computed color (see the style)
  secondary: { box: 'border-transparent bg-secondary', text: 'text-secondary-foreground' },
  ghost: { box: 'border-transparent active:bg-muted dark:active:bg-muted/50', text: 'text-foreground' },
  destructive: { box: 'border-transparent bg-destructive/10 active:bg-destructive/20 dark:bg-destructive/20 dark:active:bg-destructive/30', text: 'text-destructive' },
}

export interface ButtonProps {
  label: string
  onPress?: () => void
  variant?: ButtonVariant
  size?: ButtonSize
  icon?: IconName
  loading?: boolean
  disabled?: boolean
  /** Stretch to the row, as a form's main action. */
  block?: boolean
  /** A quiet ghost button: muted text at 70%, like the web's "Add icon" or "Add relation". */
  muted?: boolean
}

/**
 * The web `Button`: the same variants, and the physical 2px edge under every filled one that drops when pressed
 * (none in the Flat style).
 * A disabled button is at half opacity, without its edge.
 */
export function Button({ label, onPress, variant = 'primary', size = 'default', icon, loading = false, disabled = false, block = false, muted = false }: ButtonProps) {
  const { colors, scheme } = useTheme()
  const dark = scheme === 'dark'
  const flat = useButtonStyle() === 'flat'
  const inactive = disabled || loading || !onPress
  const edge = flat ? undefined : {
    primary: mix(colors.primary, 0.75, cssColors.black),
    outline: dark ? alpha(cssColors.white, 0.08) : alpha(cssColors.black, 0.22),
    secondary: alpha(dark ? cssColors.white : cssColors.black, dark ? 0.08 : 0.22),
    ghost: undefined,
    destructive: alpha(colors.destructive, 0.4),
  }[variant]
  // the icon and spinner take a color prop, not a class
  const iconColor = muted
    ? alpha(colors.mutedForeground, 0.7)
    : { primary: colors.primaryForeground, outline: colors.foreground, secondary: colors.secondaryForeground, ghost: colors.foreground, destructive: colors.destructive }[variant]
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ disabled: inactive, busy: loading }}
      disabled={inactive}
      onPress={onPress}
      className={`flex-row items-center justify-center gap-1.5 border ${SIZE[size].box} ${VARIANT[variant].box} ${block ? 'self-stretch' : ''} ${disabled && !loading ? 'opacity-50' : ''}`}
      style={({ pressed }) => ({
        // only when set: a key with `undefined` would replace the class's background (`bg-primary`) in the merged style
        ...(variant === 'secondary' && pressed ? { backgroundColor: mix(colors.secondary, 0.95, colors.foreground) } : {}),
        boxShadow: edge && !pressed && !disabled ? `0 2px 0 ${edge}` : undefined,
        // Flat has no edge to sink into: the press scales down a little instead
        transform: flat ? [{ scale: pressed && variant !== 'ghost' ? 0.97 : 1 }] : [{ translateY: pressed && edge ? 2 : 0 }],
      })}
    >
      {loading ? <Spinner color={iconColor} /> : icon ? <View><Icon name={icon} size={size === 'default' || size === 'lg' ? 16 : 14} color={iconColor} /></View> : null}
      <Text className={`font-noto-medium ${SIZE[size].text} ${muted ? 'text-muted-foreground/70' : VARIANT[variant].text}`}>{label}</Text>
    </Pressable>
  )
}
