import Svg, { Circle, Path, Rect } from 'react-native-svg'

import { useTheme } from '../theme'

const LEVEL: Record<string, number> = { low: 1, medium: 2, high: 3 }

/** The web's priority glyph: bars for low to high, three dashes for none, a red square with `!` for urgent. */
export function PriorityIcon({ priority, size = 14 }: { priority: string; size?: number }) {
  const { colors } = useTheme()
  if (priority === 'urgent') {
    return (
      <Svg width={size} height={size} viewBox="0 0 14 14" accessibilityLabel="Urgent">
        <Rect x={1} y={1} width={12} height={12} rx={3} fill={colors.destructive} />
        <Path d="M7 3.8 V8" stroke={colors.primaryForeground} strokeWidth={1.6} strokeLinecap="round" />
        <Circle cx={7} cy={10.4} r={0.9} fill={colors.primaryForeground} />
      </Svg>
    )
  }
  const level = LEVEL[priority]
  if (!level) {
    return (
      <Svg width={size} height={size} viewBox="0 0 14 14" accessibilityLabel="No priority">
        {[1.5, 5.7, 9.9].map((x) => (
          <Rect key={x} x={x} y={6.2} width={2.6} height={1.6} rx={0.8} fill={colors.mutedForeground} opacity={0.6} />
        ))}
      </Svg>
    )
  }
  const bars = [
    { x: 1.5, y: 8, height: 4.5 },
    { x: 5.7, y: 5.5, height: 7 },
    { x: 9.9, y: 2.5, height: 10 },
  ]
  return (
    <Svg width={size} height={size} viewBox="0 0 14 14" accessibilityLabel={priority}>
      {bars.map((bar, index) => (
        <Rect key={bar.x} x={bar.x} y={bar.y} width={2.6} height={bar.height} rx={1} fill={index < level ? colors.mutedForeground : colors.muted} />
      ))}
    </Svg>
  )
}
