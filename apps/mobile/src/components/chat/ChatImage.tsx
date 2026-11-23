import { requireOptionalNativeModule } from 'expo'
import type { ImageProps as ExpoImageProps } from 'expo-image'
import type { ComponentType } from 'react'
import { Image, type ImageStyle, Platform, type StyleProp } from 'react-native'

/**
 * `expo-image` plays animated WebP and GIF on iOS and Android; React Native's `Image` animates only a GIF on iOS, and
 * KLIPY serves WebP first. It has native code, so a build made before it was added lacks it: there the image falls back
 * to React Native's `Image` (the first frame of a WebP).
 */
const ExpoImage: ComponentType<ExpoImageProps> | null =
  Platform.OS === 'web' || requireOptionalNativeModule('ExpoImage') ? (require('expo-image') as typeof import('expo-image')).Image : null

/**
 * A chat image that may move: a GIF or sticker from KLIPY, or a workspace sticker. `contain` keeps all of it in the
 * box (stickers); otherwise it fills the box (GIFs and picker cells).
 */
export function ChatImage({
  source,
  style,
  label,
  contain = false,
}: {
  source: { uri: string; headers?: Record<string, string> }
  style: StyleProp<ImageStyle>
  label: string
  contain?: boolean
}) {
  if (ExpoImage) {
    return <ExpoImage accessibilityLabel={label} source={source} style={style} contentFit={contain ? 'contain' : 'cover'} cachePolicy="memory-disk" recyclingKey={source.uri} />
  }
  return <Image accessibilityLabel={label} source={source} style={style} resizeMode={contain ? 'contain' : 'cover'} />
}
