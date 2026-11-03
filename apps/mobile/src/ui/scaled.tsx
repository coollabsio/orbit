import { Text as RNText, TextInput as RNTextInput, type TextInputProps, type TextProps, useWindowDimensions } from 'react-native'

/**
 * Text follows iOS/Android Text Size only upward. Safari ignores a smaller system text size, so the web keeps its
 * sizes; shrinking here (a phone at 0.82) made the app look smaller than the web. Larger sizes still grow, to 1.5×.
 */
function useFontScaling(): Pick<TextProps, 'allowFontScaling' | 'maxFontSizeMultiplier'> {
  const { fontScale } = useWindowDimensions()
  return { allowFontScaling: fontScale > 1, maxFontSizeMultiplier: 1.5 }
}

/** React Native's `Text` with the app's font scaling. Use it (or `ui/Text`) instead of importing `Text` from react-native. */
export function NativeText(props: TextProps) {
  return <RNText {...useFontScaling()} {...props} />
}

/**
 * React Native's `TextInput` with the app's font scaling, at 16px like every web field on a phone
 * (`max-md:text-[16px]!` in the web's index.css). The style wins over a size in `className`, as `!` does there.
 */
export function TextInput({ style, ...props }: TextInputProps) {
  return <RNTextInput {...useFontScaling()} {...props} style={[style, { fontSize: 16 }]} />
}
