import { useEffect, useRef } from 'react'
import { Animated, Easing, View } from 'react-native'

import { useReduceMotion } from './useReduceMotion'

/** One pixel of the 11 × 11 logo grid. */
const PX = 6
const CYCLE = 1600
/** The logo's colors, the same in both themes (as on the web). */
const DIM = '#742f4d'
const LIT = '#f2458f'
const SHADOW = [[1, 3, 2, 6], [3, 1, 6, 2], [9, 3, 2, 6], [3, 9, 6, 2]]
/** The lit sides, clockwise from the top. */
const SIDES = [[2, 0, 6, 2], [8, 2, 2, 6], [2, 8, 6, 2], [0, 2, 2, 6]]
const ease = Easing.bezier(0.42, 0, 0.58, 1)
const STEPS = 64
const INPUT = Array.from({ length: STEPS + 1 }, (_, step) => step / STEPS)

/** The web's `orbit-chase` keyframes: 0.3, up to 1 at 20%, back to 0.3 at 60%, then rest. */
function chase(at: number): number {
  if (at < 0.2) return 0.3 + 0.7 * ease(at / 0.2)
  if (at < 0.6) return 1 - 0.7 * ease((at - 0.2) / 0.4)
  return 0.3
}

/**
 * The pixel "O" of the logo as a loader (the web's `OrbitLoader`): its four sides light up one after the other,
 * clockwise, over the shadow of the mark. The chase runs on the wall clock, so the next loader (the next loading
 * stage) goes on where the last one was.
 */
export function OrbitLoader() {
  const reduceMotion = useReduceMotion()
  const clock = useRef(new Animated.Value(0)).current
  const start = useRef((Date.now() % CYCLE) / CYCLE).current

  useEffect(() => {
    if (reduceMotion) return
    const loop = Animated.loop(Animated.timing(clock, { toValue: 1, duration: CYCLE, easing: Easing.linear, useNativeDriver: true }))
    loop.start()
    return () => loop.stop()
  }, [clock, reduceMotion])

  return (
    <View accessibilityRole="progressbar" accessibilityLabel="Loading" style={{ width: 11 * PX, height: 11 * PX }}>
      {SHADOW.map(([x, y, w, h]) => <View key={`${x}-${y}`} style={[rect(x, y, w, h), { backgroundColor: DIM }]} />)}
      {SIDES.map(([x, y, w, h], index) => {
        const phase = (at: number) => chase((((at + start - index / 4) % 1) + 1) % 1)
        const opacity = reduceMotion ? phase(0) : clock.interpolate({ inputRange: INPUT, outputRange: INPUT.map(phase) })
        return <Animated.View key={index} style={[rect(x, y, w, h), { backgroundColor: LIT, opacity }]} />
      })}
    </View>
  )
}

function rect(x: number, y: number, w: number, h: number) {
  return { position: 'absolute' as const, left: x * PX, top: y * PX, width: w * PX, height: h * PX }
}
