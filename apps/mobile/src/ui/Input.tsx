import { type TextInputProps, View } from 'react-native'
import { TextInput } from './scaled'

import { useTheme } from '../theme'
import { Text } from './Text'

export interface InputProps extends Omit<TextInputProps, 'style'> {
  label?: string
  /** The pink `*` after the label. */
  required?: boolean
  /** Quiet help under the field; `error` replaces it. */
  description?: string
  error?: string
}

/** The web `Input` at phone size: a 32px field (16px text, so iOS does not zoom) under a 14px label. */
export function Input({ label, required = false, description, error, multiline, ...props }: InputProps) {
  const { colors, scheme } = useTheme()
  return (
    <View className="gap-2">
      {label ? (
        <Text className="font-noto-medium text-[14px] leading-[19.25px]">
          {label}
          {required ? <Text className="font-noto-semibold text-[14px] leading-[19.25px] text-primary">{'  *'}</Text> : null}
        </Text>
      ) : null}
      <TextInput
        placeholderTextColor={colors.mutedForeground}
        selectionColor={colors.primary}
        keyboardAppearance={scheme}
        accessibilityLabel={label}
        multiline={multiline}
        {...props}
        // `dark:bg-input/30`: the web's faint fill, so the field reads on the dark canvas
        className={`rounded-lg border px-2.5 font-noto text-[16px] leading-[24px] text-foreground dark:bg-input/30 ${multiline ? 'min-h-[76px] py-1.5' : 'min-h-8 py-[3px]'} ${error ? 'border-destructive' : 'border-input focus:border-ring'}`}
        style={{ textAlignVertical: multiline ? 'top' : 'center' }}
      />
      {error || description ? <Text className={`text-[14px] leading-[21px] ${error ? 'text-destructive' : 'text-muted-foreground'}`}>{error ?? description}</Text> : null}
    </View>
  )
}
