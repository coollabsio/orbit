import { shrinkImage } from '@/lib/shrinkImage'
import { shrinkVideo } from '@/lib/shrinkVideo'

/** Makes a large photo, screenshot or video smaller before upload; other files stay as they are. */
export function shrinkUpload(file: File, options: { signal?: AbortSignal; onProgress?: (progress: number) => void } = {}): Promise<File> {
  return file.type.startsWith('video/') ? shrinkVideo(file, options) : shrinkImage(file)
}
