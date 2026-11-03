import * as DocumentPicker from 'expo-document-picker'
import * as ImagePicker from 'expo-image-picker'

import type { PickedFile } from '../../data/tasks'
import { uploadPart } from '../../upload-part'

export type FileSource = 'photos' | 'files'

/** Adds the part to send for each picked file (`src/upload-part.ts`). */
const withParts = (files: Omit<PickedFile, 'blob'>[]): Promise<PickedFile[]> =>
  Promise.all(files.map(async (file) => ({ ...file, blob: await uploadPart(file.uri, file.name) })))

/** The browser's file dialog (the web preview of the app). */
export async function pickFiles(source: FileSource): Promise<PickedFile[]> {
  if (source === 'photos') {
    const result = await ImagePicker.launchImageLibraryAsync({ mediaTypes: ['images'], allowsMultipleSelection: true })
    if (result.canceled) return []
    return withParts(result.assets.map((asset, index) => ({ name: asset.fileName ?? `photo-${index + 1}.png`, mimeType: asset.mimeType ?? 'image/png', uri: asset.uri })))
  }
  const result = await DocumentPicker.getDocumentAsync({ multiple: true })
  if (result.canceled) return []
  return withParts(result.assets.map((asset) => ({ name: asset.name, mimeType: asset.mimeType ?? 'application/octet-stream', uri: asset.uri })))
}

/** Fetches the file with the session token and opens it in a new tab. */
export async function openFile(url: string, _name: string, token: string): Promise<void> {
  const response = await fetch(url, { headers: { Authorization: `Bearer ${token}` } })
  if (!response.ok) throw new Error(`Download failed (${response.status})`)
  window.open(URL.createObjectURL(await response.blob()), '_blank')
}
