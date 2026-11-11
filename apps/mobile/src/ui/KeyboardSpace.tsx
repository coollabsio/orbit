import { type ReactNode, useEffect, useRef, useState } from 'react'
import { Keyboard, Platform, type StyleProp, View, type ViewInstance, type ViewProps, type ViewStyle } from 'react-native'

/**
 * Keeps its content above the iOS keyboard: its bottom padding is the part of it that the keyboard covers. Unlike
 * `KeyboardAvoidingView` it starts no `LayoutAnimation`, which slides every other layout change of that moment up and
 * down too. The padding changes in one step.
 */
export function KeyboardSpace({ className, style, pointerEvents, children }: { className?: string; style?: StyleProp<ViewStyle>; pointerEvents?: ViewProps['pointerEvents']; children: ReactNode }) {
  const view = useRef<ViewInstance>(null)
  // where the keyboard starts on the screen; null while it is hidden
  const [keyboardTop, setKeyboardTop] = useState<number | null>(null)
  // where this view ends on the screen, without the padding
  const [bottom, setBottom] = useState<number | null>(null)

  const measure = () => view.current?.measureInWindow((_x, y, _width, height) => setBottom(y + height))

  useEffect(() => {
    if (Platform.OS !== 'ios') return
    const change = Keyboard.addListener('keyboardWillChangeFrame', (event) => {
      const top = event.endCoordinates.screenY
      setKeyboardTop(event.endCoordinates.height > 0 ? top : null)
      measure()
    })
    const hide = Keyboard.addListener('keyboardWillHide', () => setKeyboardTop(null))
    return () => {
      change.remove()
      hide.remove()
    }
  }, [])

  const padding = keyboardTop !== null && bottom !== null ? Math.max(0, bottom - keyboardTop) : 0
  return (
    <View ref={view} onLayout={measure} className={className} pointerEvents={pointerEvents} style={[style, { paddingBottom: padding }]}>
      {children}
    </View>
  )
}
