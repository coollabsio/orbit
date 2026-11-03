import { useState } from 'react'
import { Image } from 'react-native'

import { absoluteUrl } from '../../data/docs'
import { useOrbit } from '../../orbit-context'
import { Avatar } from '../../ui'

/** The person's picture when they have one (it needs the session's bearer token), else the initials avatar. */
export function ProfilePicture({ id, name, url, size }: { id: string; name: string; url?: string | null; size: number }) {
  const { session } = useOrbit()
  const [failed, setFailed] = useState<string | null>(null)
  if (!url || failed === url) return <Avatar id={id} name={name} size={size} />
  return (
    <Image
      accessibilityLabel={name}
      source={{ uri: absoluteUrl(session.server, url), headers: { Authorization: `Bearer ${session.token}` } }}
      onError={() => setFailed(url)}
      className="rounded-full"
      style={{ width: size, height: size }}
    />
  )
}
