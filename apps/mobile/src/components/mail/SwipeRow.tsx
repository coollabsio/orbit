import { type ReactNode, useEffect, useRef, useState } from 'react'
import { Animated, Easing, PanResponder, type PanResponderGestureState, View } from 'react-native'

import type { SwipeStep } from '../../data/mail-swipe'
import { Icon, Text } from '../../ui'

type Side = 'left' | 'right'

/** The part of the row's width a swipe must pass to act. */
const THRESHOLD = 0.3

const sideOf = (dx: number): Side => (dx < 0 ? 'left' : 'right')

/**
 * A list row that slides under the finger and shows the action behind it. Past the threshold, letting go runs the
 * action: a row that leaves the folder slides out, any other slides back. `resetKey` changes when the row's data
 * changes, and puts a row that slid out but stayed (an error, or a list that still holds it) back in place.
 */
export function SwipeRow({
  left,
  right,
  onSwipe,
  resetKey,
  children,
}: {
  left: SwipeStep | null
  right: SwipeStep | null
  onSwipe: (step: SwipeStep) => void
  resetKey: unknown
  children: ReactNode
}) {
  const x = useRef(new Animated.Value(0)).current
  const [width, setWidth] = useState(0)
  const [side, setSide] = useState<Side | null>(null)
  const live = useRef({ left, right, width, onSwipe })
  live.current = { left, right, width, onSwipe }

  useEffect(() => {
    x.setValue(0)
    setSide(null)
  }, [resetKey, x])

  const responder = useRef(
    PanResponder.create({
      // a clear horizontal move toward a side with an action, so taps, long presses and scrolling stay with the list
      onMoveShouldSetPanResponder: (_, gesture) => Math.abs(gesture.dx) > 10 && Math.abs(gesture.dx) > Math.abs(gesture.dy) * 1.5 && live.current[sideOf(gesture.dx)] !== null,
      onPanResponderTerminationRequest: () => false,
      onPanResponderMove: (_, gesture) => {
        const next = sideOf(gesture.dx)
        x.setValue(live.current[next] ? gesture.dx : 0)
        setSide(next)
      },
      onPanResponderRelease: (_, gesture: PanResponderGestureState) => {
        const step = live.current[sideOf(gesture.dx)]
        const rowWidth = live.current.width
        if (!step || Math.abs(gesture.dx) < rowWidth * THRESHOLD) {
          Animated.spring(x, {
            toValue: 0,
            useNativeDriver: true,
            bounciness: 0,
          }).start()
          return
        }
        if (step.leaves) {
          Animated.timing(x, {
            toValue: Math.sign(gesture.dx) * rowWidth,
            duration: 160,
            easing: Easing.out(Easing.cubic),
            useNativeDriver: true,
          }).start(() => live.current.onSwipe(step))
        } else {
          Animated.spring(x, {
            toValue: 0,
            useNativeDriver: true,
            bounciness: 0,
          }).start()
          live.current.onSwipe(step)
        }
      },
      onPanResponderTerminate: () =>
        Animated.spring(x, {
          toValue: 0,
          useNativeDriver: true,
          bounciness: 0,
        }).start(),
    }),
  ).current

  const step = side === 'left' ? left : side === 'right' ? right : null
  const limit = Math.max(width * THRESHOLD, 1)
  // the action shows faded until the swipe passes the threshold
  const opacity = x.interpolate({
    inputRange: [-limit, -limit * 0.6, 0, limit * 0.6, limit],
    outputRange: [1, 0.55, 0, 0.55, 1],
    extrapolate: 'clamp',
  })
  return (
    <View onLayout={(event) => setWidth(event.nativeEvent.layout.width)} className="overflow-hidden">
      {step ? (
        <Animated.View style={{ backgroundColor: step.color, opacity }} className={`absolute inset-0 flex-row items-center gap-2 px-5 ${side === 'left' ? 'justify-end' : 'justify-start'}`}>
          <Icon name={step.icon} size={18} color="#ffffff" />
          <Text className="font-noto-medium text-[13px] text-white">{step.label}</Text>
        </Animated.View>
      ) : null}
      <Animated.View style={{ transform: [{ translateX: x }] }} className="bg-background" {...responder.panHandlers}>
        {children}
      </Animated.View>
    </View>
  )
}
