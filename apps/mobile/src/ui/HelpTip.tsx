import { type ReactNode, useRef, useState } from 'react'
import { Modal, Pressable, useWindowDimensions, View, type ViewInstance } from 'react-native'

import { Text } from './Text'

const GAP = 6
const EDGE = 8
const MAX_WIDTH = 320

type Anchor = { x: number; y: number; width: number; height: number }

/**
 * The web `HelpTip`: a tap on `children` shows `text` in a small popover next to it (above, or below when the trigger
 * is near the top of the screen). A tap anywhere closes it.
 */
export function HelpTip({ text, label, className, children }: { text: string; /** For screen readers: what the trigger is. */ label?: string; className?: string; children: ReactNode }) {
  const trigger = useRef<ViewInstance>(null)
  const [anchor, setAnchor] = useState<Anchor | null>(null)
  const { width: screenWidth, height: screenHeight } = useWindowDimensions()

  const show = () => trigger.current?.measureInWindow((x, y, width, height) => setAnchor({ x, y, width, height }))
  const close = () => setAnchor(null)

  // The bubble starts at the trigger's left edge (or ends at its right edge, on the right half of the screen).
  const fromLeft = anchor ? anchor.x + anchor.width / 2 < screenWidth / 2 : true
  const left = anchor ? (fromLeft ? Math.max(EDGE, anchor.x - EDGE) : Math.max(EDGE, anchor.x + anchor.width + EDGE - MAX_WIDTH)) : 0
  const right = anchor ? (fromLeft ? Math.min(screenWidth - EDGE, left + MAX_WIDTH) : Math.min(screenWidth - EDGE, anchor.x + anchor.width + EDGE)) : 0
  const below = anchor ? anchor.y < screenHeight / 3 : false

  return (
    <>
      <Pressable ref={trigger} accessibilityRole="button" accessibilityLabel={label} accessibilityHint={text} hitSlop={10} onPress={show} className={className}>
        {children}
      </Pressable>
      <Modal visible={anchor !== null} transparent animationType="fade" statusBarTranslucent navigationBarTranslucent onRequestClose={close}>
        <Pressable accessibilityLabel="Close" onPress={close} className="flex-1">
          {anchor ? (
            <View
              style={[{ position: 'absolute', left, width: right - left, alignItems: fromLeft ? 'flex-start' : 'flex-end' }, below ? { top: anchor.y + anchor.height + GAP } : { bottom: screenHeight - anchor.y + GAP }]}
            >
              <View className="rounded-md border border-border bg-popover px-3 py-1.5 shadow-md">
                <Text className="text-[12px] leading-[17px] text-popover-foreground">{text}</Text>
              </View>
            </View>
          ) : null}
        </Pressable>
      </Modal>
    </>
  )
}
