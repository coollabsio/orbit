import Svg, { Circle, Path } from 'react-native-svg'

import { useTheme } from '../theme'

/**
 * The web's Linear-style status glyph: the shape follows the category (○ ◐ ✓ ⊘ =), the color the status. Without a
 * status, a dashed circle.
 */
export function StatusIcon({ status, size = 16 }: { status: { category: string; color: string } | undefined; size?: number }) {
  const { colors } = useTheme()
  const color = status?.color ?? colors.mutedForeground
  const mark = { stroke: colors.card, strokeWidth: 1.5, strokeLinecap: 'round' as const, strokeLinejoin: 'round' as const, fill: 'none' }
  return (
    <Svg width={size} height={size} viewBox="0 0 14 14">
      {!status ? <Circle cx={7} cy={7} r={5.5} stroke={color} strokeWidth={1.5} strokeDasharray="2 2" fill="none" /> : null}
      {status?.category === 'unstarted' ? <Circle cx={7} cy={7} r={5.5} stroke={color} strokeWidth={1.5} fill="none" /> : null}
      {status?.category === 'started' ? (
        <>
          <Circle cx={7} cy={7} r={5.5} stroke={color} strokeWidth={1.5} fill="none" />
          <Path d="M7 3.5 A3.5 3.5 0 0 1 7 10.5 Z" fill={color} />
        </>
      ) : null}
      {status && ['completed', 'cancelled', 'duplicate'].includes(status.category) ? <Circle cx={7} cy={7} r={6} fill={color} /> : null}
      {status?.category === 'completed' ? <Path d="M4.4 7.2 L6.2 9 L9.6 5.4" {...mark} /> : null}
      {status?.category === 'cancelled' ? <Path d="M4.8 4.8 L9.2 9.2 M9.2 4.8 L4.8 9.2" {...mark} /> : null}
      {status?.category === 'duplicate' ? <Path d="M4.5 5.6 H9.5 M4.5 8.4 H9.5" {...mark} /> : null}
    </Svg>
  )
}
