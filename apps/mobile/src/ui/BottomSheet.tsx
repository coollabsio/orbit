import { type ReactNode, useEffect, useRef, useState } from 'react'
import { Animated, Easing, KeyboardAvoidingView, Modal, Platform, Pressable, type StyleProp, useWindowDimensions, View, type ViewStyle } from 'react-native'

import { alpha, cssColors } from '../theme'

/**
 * A sheet that rises from the bottom over a dimmed screen: the dim fades and only the panel slides, as the web's phone
 * sheets do. While it closes it keeps showing what it showed when open, so the panel does not empty mid-slide.
 */
export function BottomSheet({
  visible,
  onClose,
  className,
  style,
  maxHeight = 0.8,
  avoidKeyboard = false,
  statusBarTranslucent,
  children,
}: {
  visible: boolean
  onClose: () => void
  /** The panel's look. */
  className?: string
  style?: StyleProp<ViewStyle>
  /** The panel's highest share of the screen. */
  maxHeight?: number
  avoidKeyboard?: boolean
  statusBarTranslucent?: boolean
  children: ReactNode
}) {
  const { height } = useWindowDimensions()
  const progress = useRef(new Animated.Value(0)).current
  const [mounted, setMounted] = useState(visible)
  const shown = useRef(children)
  if (visible) shown.current = children
  if (visible && !mounted) setMounted(true)

  useEffect(() => {
    if (!mounted) return
    Animated.timing(progress, { toValue: visible ? 1 : 0, duration: visible ? 220 : 180, easing: Easing.out(Easing.cubic), useNativeDriver: true }).start(({ finished }) => {
      if (finished && !visible) setMounted(false)
    })
  }, [visible, mounted, progress])

  const panel = (
    <>
      <Animated.View style={{ position: 'absolute', inset: 0, backgroundColor: alpha(cssColors.black, 0.4), opacity: progress }}>
        <Pressable accessibilityLabel="Close" className="flex-1" onPress={onClose} />
      </Animated.View>
      <Animated.View
        accessibilityViewIsModal
        pointerEvents={visible ? 'auto' : 'none'}
        style={{ maxHeight: height * maxHeight, transform: [{ translateY: progress.interpolate({ inputRange: [0, 1], outputRange: [height, 0] }) }] }}
      >
        {/* Animated: only the slide; the look is the caller's classes on this view */}
        <View className={`shrink ${className ?? ''}`} style={style}>
          {shown.current}
        </View>
      </Animated.View>
    </>
  )
  return (
    <Modal visible={mounted} transparent animationType="none" onRequestClose={onClose} statusBarTranslucent={statusBarTranslucent}>
      {avoidKeyboard ? (
        <KeyboardAvoidingView className="flex-1 justify-end" behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
          {panel}
        </KeyboardAvoidingView>
      ) : (
        <View className="flex-1 justify-end">{panel}</View>
      )}
    </Modal>
  )
}
