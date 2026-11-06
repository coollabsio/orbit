import { type ReactNode, useState } from 'react'
import { Pressable, type TextInputProps, View } from 'react-native'
import { TextInput } from './scaled'

import { useTheme } from '../theme'
import { Icon } from './Icon'
import { Text } from './Text'

export interface InputProps extends Omit<TextInputProps, 'style'> {
  label?: string
  /** The pink `*` after the label. */
  required?: boolean
  /** Quiet help under the field; `error` replaces it. */
  description?: string
  error?: string
  /** A control beside the field, like the web's `flex gap-2` row (Profile's email and Change). */
  accessory?: ReactNode
}

/**
 * The web `Input` at phone size: a 32px field (16px text, so iOS does not zoom) under a 14px label. A `secureTextEntry`
 * field gets the web `PasswordInput`'s eye button, which shows or hides the password.
 */
export function Input({ label, required = false, description, error, multiline, accessory, secureTextEntry, ...props }: InputProps) {
  const { colors, scheme } = useTheme()
  const [visible, setVisible] = useState(false)
  const input = (
    <TextInput
      placeholderTextColor={colors.mutedForeground}
      selectionColor={colors.primary}
      keyboardAppearance={scheme}
      accessibilityLabel={label}
      multiline={multiline}
      {...props}
      secureTextEntry={secureTextEntry && !visible}
      // `dark:bg-input/30`: the web's faint fill, so the field reads on the dark canvas
      className={`rounded-lg border px-2.5 font-noto text-[16px] leading-[24px] text-foreground dark:bg-input/30 ${accessory || secureTextEntry ? 'flex-1 min-w-0' : ''} ${secureTextEntry ? 'pr-8' : ''} ${multiline ? 'min-h-[76px] py-1.5' : 'min-h-8 py-[3px]'} ${error ? 'border-destructive' : 'border-input focus:border-ring'}`}
      style={{ textAlignVertical: multiline ? 'top' : 'center' }}
    />
  )
  const field = secureTextEntry ? (
    <View className={`flex-row items-center ${accessory ? 'flex-1 min-w-0' : ''}`}>
      {input}
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={visible ? 'Hide password' : 'Show password'}
        accessibilityState={{ selected: visible, disabled: props.editable === false }}
        disabled={props.editable === false}
        onPress={() => setVisible((shown) => !shown)}
        hitSlop={6}
        className="absolute right-1 size-6 items-center justify-center rounded-md active:bg-muted"
      >
        <Icon name={visible ? 'eyeSlash' : 'eye'} size={14} color={colors.mutedForeground} />
      </Pressable>
    </View>
  ) : (
    input
  )
  return (
    <View className="gap-2">
      {label ? (
        <Text className="font-noto-medium text-[14px] leading-[19.25px]">
          {label}
          {required ? <Text className="font-noto-semibold text-[14px] leading-[19.25px] text-primary">{'  *'}</Text> : null}
        </Text>
      ) : null}
      {accessory ? (
        <View className="flex-row items-center gap-2">
          {field}
          {accessory}
        </View>
      ) : (
        field
      )}
      {error || description ? <Text className={`text-[14px] leading-[21px] ${error ? 'text-destructive' : 'text-muted-foreground'}`}>{error ?? description}</Text> : null}
    </View>
  )
}
