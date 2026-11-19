import type { ColorValue } from 'react-native'
import { SvgXml } from 'react-native-svg'

import { brandIcons } from './brand-icons'
import { icons as reicons } from './icons.gen'
import { useTheme } from '../theme'

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
  return <SvgXml xml={icons[name]} width={size} height={size} color={color ?? colors.foreground} />
}
