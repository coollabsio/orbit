import { Directory, File, Paths } from 'expo-file-system'

/**
 * A picked file (photo library, camera, document picker) as a multipart part. Since SDK 52 the global `fetch` is
 * `expo/fetch`, which only sends parts that are strings, Blobs or objects with `bytes()`; React Native's classic
 * `{ uri, name, type }` object fails with "Unsupported FormDataPart implementation". A `File` from expo-file-system
 * has `bytes()`, and its name becomes the part's filename, so the picked name is kept by copying when it differs.
 */
export async function uploadPart(uri: string, name: string): Promise<Blob> {
  const picked = new File(uri)
  const filename = name.replace(/[/\\]/g, '_') || 'file'
  if (picked.name === filename) return picked as unknown as Blob
  const folder = new Directory(Paths.cache, 'uploads', String(Date.now()))
  folder.create({ intermediates: true, idempotent: true })
  const named = new File(folder, filename)
  await picked.copy(named)
  return named as unknown as Blob
}
