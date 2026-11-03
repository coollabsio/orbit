import * as DocumentPicker from 'expo-document-picker'
import { Directory, File, Paths } from 'expo-file-system'
import * as ImagePicker from 'expo-image-picker'

import type { PickedFile } from '../../data/tasks'
import { uploadPart } from '../../upload-part'

export type FileSource = 'photos' | 'files'

/** Adds the part to send for each picked file (`src/upload-part.ts`). */
const withParts = (files: Omit<PickedFile, 'blob'>[]): Promise<PickedFile[]> =>
  Promise.all(files.map(async (file) => ({ ...file, blob: await uploadPart(file.uri, file.name) })))

/** Photos from the library (JPEG, so HEIC photos upload as images the web shows) or any files. `[]` when cancelled. */
export async function pickFiles(source: FileSource): Promise<PickedFile[]> {
  if (source === 'photos') {
    const result = await ImagePicker.launchImageLibraryAsync({ mediaTypes: ['images'], allowsMultipleSelection: true, quality: 0.85 })
    if (result.canceled) return []
    return withParts(result.assets.map((asset, index) => ({
      name: asset.fileName ?? `photo-${index + 1}.jpg`,
      mimeType: asset.mimeType ?? 'image/jpeg',
      uri: asset.uri })))
  }
  const result = await DocumentPicker.getDocumentAsync({ multiple: true, copyToCacheDirectory: true })
  if (result.canceled) return []
  return withParts(result.assets.map((asset) => ({ name: asset.name, mimeType: asset.mimeType ?? 'application/octet-stream', uri: asset.uri })))
}

/**
 * Downloads an attachment with the session token (never in the URL) into the cache and opens it in the system
 * preview: Quick Look on iOS, the app for its type on Android.
 */
export async function openFile(url: string, name: string, token: string): Promise<void> {
  const folder = new Directory(Paths.cache, 'attachments', String(Date.now()))
  folder.create({ intermediates: true, idempotent: true })
  const file = await File.downloadFileAsync(url, new File(folder, name.replace(/[/\\]/g, '_') || 'file'), { headers: { Authorization: `Bearer ${token}` }, idempotent: true })
  await file.preview()
}
