import type { Ref } from 'react'
import { Text as RNText, TextInput as RNTextInput, type StyleProp, StyleSheet, type TextInputInstance, type TextInputProps, type TextProps, type TextStyle, useWindowDimensions } from 'react-native'
import { useResolveClassNames } from 'uniwind'

import { useTypography } from '../theme'
import { typographyStyle } from '../theme/typography'

/**
 * Text follows iOS/Android Text Size only upward. Safari ignores a smaller system text size, so the web keeps its
 * sizes; shrinking here (a phone at 0.82) made the app look smaller than the web. Larger sizes still grow, to 1.5×.
 */
function useFontScaling(): Pick<TextProps, 'allowFontScaling' | 'maxFontSizeMultiplier'> {
  const { fontScale } = useWindowDimensions()
  return { allowFontScaling: fontScale > 1, maxFontSizeMultiplier: 1.5 }
}

/**
 * Settings → Appearance → Font and Text size on one text: its style with the chosen font and scale on top. With the
 * default font and size it is the style as given, and the class names are not resolved a second time.
 */
function useAppearanceStyle(className: string | undefined, style: StyleProp<TextStyle>): StyleProp<TextStyle> {
  const { font, scale } = useTypography()
  const plain = font === 'noto-sans' && scale === 1
  const resolved = useResolveClassNames(plain ? '' : (className ?? ''))
  if (plain) return style
  return [style, typographyStyle(StyleSheet.flatten([resolved as TextStyle, style]) ?? {}, font, scale)]
}

/** React Native's `Text` with the app's font scaling. Use it (or `ui/Text`) instead of importing `Text` from react-native. */
export function NativeText({ style, ...props }: TextProps) {
  return <RNText {...useFontScaling()} {...props} style={useAppearanceStyle(props.className, style)} />
}

/**
 * React Native's `TextInput` with the app's font scaling, at 16px like every web field on a phone
 * (`max-md:text-[16px]!` in the web's index.css). The style wins over a size in `className`, as `!` does there.
 * `editorSize` keeps the `className` size: for a composer the web draws with its rich-text editor, not a field.
 */
export function TextInput({ style, editorSize = false, ...props }: TextInputProps & { editorSize?: boolean; ref?: Ref<TextInputInstance> }) {
  return <RNTextInput {...useFontScaling()} {...props} style={useAppearanceStyle(props.className, editorSize ? style : [style, { fontSize: 16 }])} />
}
