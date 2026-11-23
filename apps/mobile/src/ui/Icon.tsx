import type { ColorValue } from 'react-native'
import { SvgXml } from 'react-native-svg'

import { brandIcons } from './brand-icons'
import { icons as reicons } from './icons.gen'
import { useTheme, useUiScale } from '../theme'

// A sign-in provider's `icon={provider}`: any OpenID Connect provider gets a neutral key, not a brand mark.
const icons = { ...reicons, ...brandIcons, oidc: reicons.key }

export type IconName = keyof typeof icons

export interface IconProps {
  name: IconName
  size?: number
  /** Fills `currentColor` in the icon markup. Defaults to the theme foreground. */
  color?: ColorValue
}

export function Icon({ name, size = 20, color }: IconProps) {
  const { colors } = useTheme()
  const scaled = size * useUiScale()
  return <SvgXml xml={icons[name]} width={scaled} height={scaled} color={color ?? colors.foreground} />
}
