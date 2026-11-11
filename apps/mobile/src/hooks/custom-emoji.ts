/** The workspace settings' custom emoji and stickers: lists, writes and the image picker, over `data/custom-emoji-api.ts`. */
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import * as DocumentPicker from 'expo-document-picker'
import * as ImagePicker from 'expo-image-picker'

import { chatQueryKeys } from '../chat-cache'
import { emojiNames } from '../data/chat'
import * as chat from '../data/chat-api'
import * as api from '../data/custom-emoji-api'
import { useOrbit } from '../orbit-context'
import { uploadPart } from '../upload-part'

const stickersKey = (workspaceId: string) => ['chat-stickers', workspaceId] as const

/** The custom emoji list; the same cache as the chat's emoji, so a change shows in the picker too. */
export function useCustomEmojiList(enabled: boolean) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: chatQueryKeys.customEmoji(workspaceId), queryFn: () => chat.customEmoji(client, workspaceId), staleTime: 5 * 60_000, enabled })
}

/** The standard emoji names (`joy`), from the server's emojibase copy; `null` while it loads or when it cannot. */
export function useStandardEmojiNames(enabled: boolean): ReadonlyMap<string, string> | null {
  const { session } = useOrbit()
  const query = useQuery({ queryKey: chatQueryKeys.emojibase(session.server), queryFn: () => chat.emojibase(session.server), staleTime: Infinity, gcTime: Infinity, retry: 1, enabled, select: emojiNames })
  return query.data ?? null
}

export function useCustomStickers(enabled: boolean) {
  const { client, workspaceId } = useOrbit()
  return useQuery({ queryKey: stickersKey(workspaceId), queryFn: () => api.stickers(client, workspaceId), staleTime: 5 * 60_000, enabled })
}

/** Adding and deleting emoji; both refetch the list. */
export function useEmojiWrites() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const onSuccess = () => queryClient.invalidateQueries({ queryKey: chatQueryKeys.customEmoji(workspaceId) })
  return {
    create: useMutation({
      mutationFn: async ({ name, image }: { name: string; image: api.PickedImage }) => api.createEmoji(client, workspaceId, name, await uploadPart(image.uri, image.name)),
      onSuccess,
    }),
    remove: useMutation({ mutationFn: (emoji: api.CustomEmojiRecord) => api.deleteEmoji(client, workspaceId, emoji.id), onSuccess }),
  }
}

/** Adding and deleting stickers; both refetch the list. */
export function useStickerWrites() {
  const { client, workspaceId } = useOrbit()
  const queryClient = useQueryClient()
  const onSuccess = () => queryClient.invalidateQueries({ queryKey: stickersKey(workspaceId) })
  return {
    create: useMutation({
      mutationFn: async ({ name, image }: { name: string; image: api.PickedImage }) => api.createSticker(client, workspaceId, name, await uploadPart(image.uri, image.name)),
      onSuccess,
    }),
    remove: useMutation({ mutationFn: (sticker: api.CustomStickerRecord) => api.deleteSticker(client, workspaceId, sticker.id), onSuccess }),
  }
}

export type ImageSource = 'photos' | 'files'

/**
 * One image from the photo library or from Files, unchanged (a GIF stays animated); `null` when cancelled. The library
 * gives a photo without a known type as JPEG.
 */
export async function pickCustomImage(source: ImageSource): Promise<api.PickedImage | null> {
  if (source === 'photos') {
    const result = await ImagePicker.launchImageLibraryAsync({ mediaTypes: ['images'], allowsEditing: false, quality: 1 })
    const asset = result.canceled ? undefined : result.assets[0]
    if (!asset) return null
    const type = asset.mimeType ?? ''
    const extension = type === 'image/jpeg' ? 'jpg' : (type.split('/')[1] ?? 'jpg')
    return { uri: asset.uri, name: asset.fileName ?? `image.${extension}`, type, size: asset.fileSize ?? 0 }
  }
  const result = await DocumentPicker.getDocumentAsync({ type: api.IMAGE_TYPES, copyToCacheDirectory: true })
  const asset = result.canceled ? undefined : result.assets[0]
  if (!asset) return null
  return { uri: asset.uri, name: asset.name, type: asset.mimeType ?? '', size: asset.size ?? 0 }
}
