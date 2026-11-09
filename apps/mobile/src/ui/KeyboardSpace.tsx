import { type ReactNode, useEffect, useState } from 'react'
import { Dimensions, Keyboard, Platform, View } from 'react-native'
import { useSafeAreaInsets } from 'react-native-safe-area-context'

/**
 * Keeps its content above the iOS keyboard by setting its bottom padding to the keyboard's height. Unlike
 * `KeyboardAvoidingView` it starts no `LayoutAnimation`, which animates every other layout change of that moment too
 * (the message rows, the composer). The screen's safe area already pads the home indicator, so that part is left out.
 */
export function KeyboardSpace({ children }: { children: ReactNode }) {
  const insets = useSafeAreaInsets()
  const [keyboard, setKeyboard] = useState(0)
  useEffect(() => {
    if (Platform.OS !== 'ios') return
    // the part of the screen the keyboard will cover: 0 when it leaves (it ends below the screen)
    const change = Keyboard.addListener('keyboardWillChangeFrame', (event) => setKeyboard(Math.max(0, Dimensions.get('window').height - event.endCoordinates.screenY)))
    return () => change.remove()
  }, [])
  return <View className="flex-1" style={{ paddingBottom: Math.max(0, keyboard - insets.bottom) }}>{children}</View>
}
