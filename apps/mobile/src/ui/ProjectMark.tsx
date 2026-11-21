import { Image, View } from 'react-native'

import { absoluteUrl } from '../data/docs'
import { useOrbitIfSignedIn } from '../orbit-context'

/**
 * The web `ProjectMark`: a project's icon image when it has one, else its colour dot. `className` shapes the dot,
 * `iconSize` sizes the image (an icon needs more room than a dot to be readable).
 */
export function ProjectMark({
  project,
  className = 'size-2 rounded-full',
  iconSize = 16,
}: {
  project: { color: string; icon_url?: string | null }
  className?: string
  iconSize?: number
}) {
  const orbit = useOrbitIfSignedIn()
  if (project.icon_url && orbit) {
    // the endpoint needs the session: native Image sends no cookie, so the bearer token goes in a header (never the URL)
    return (
      <Image
        source={{ uri: absoluteUrl(orbit.session.server, project.icon_url), headers: { Authorization: `Bearer ${orbit.session.token}` } }}
        style={{ width: iconSize, height: iconSize, borderRadius: 4 }}
        accessibilityIgnoresInvertColors
      />
    )
  }
  return <View className={className} style={{ backgroundColor: project.color }} />
}
