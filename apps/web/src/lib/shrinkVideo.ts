/** Longest video edge kept on upload: 1080p, which is enough for a full-width player. */
export const MAX_VIDEO_EDGE = 1920
/** Smaller videos are uploaded as they are: re-encoding takes time and saves little. */
export const SHRINK_VIDEO_MIN_BYTES = 8 * 1024 * 1024

/**
 * The size a video is encoded at: the long edge at most `MAX_VIDEO_EDGE`, same aspect ratio. H.264 needs even
 * dimensions, so each one is rounded down to an even number.
 */
export function videoTargetSize(width: number, height: number): { width: number; height: number } {
  const scale = Math.min(1, MAX_VIDEO_EDGE / Math.max(width, height))
  const even = (value: number) => Math.max(2, Math.floor((value * scale) / 2) * 2)
  return { width: even(width), height: even(height) }
}

/**
 * Makes a large video smaller before upload: scales it to `MAX_VIDEO_EDGE` and encodes it as H.264 MP4 with WebCodecs.
 * Returns the original file when it is small, the browser cannot encode it, a track would be lost, or the result
 * would not get smaller. `onProgress` gets values from 0 to 1. An abort stops the encoding and rejects.
 */
export async function shrinkVideo(file: File, options: { signal?: AbortSignal; onProgress?: (progress: number) => void } = {}): Promise<File> {
  if (!file.type.startsWith('video/') || file.size < SHRINK_VIDEO_MIN_BYTES || typeof VideoEncoder === 'undefined') return file
  // Loaded on the first video only: the library is large and most uploads are not videos.
  const media = await import('mediabunny').catch(() => null)
  if (!media) return file
  const input = new media.Input({ source: new media.BlobSource(file), formats: media.ALL_FORMATS })
  try {
    const track = await input.getPrimaryVideoTrack()
    if (!track) return file
    const size = videoTargetSize(track.displayWidth, track.displayHeight)
    if (!(await media.canEncodeVideo('avc', { ...size, quality: media.QUALITY_MEDIUM }))) return file
    const target = new media.BufferTarget()
    const output = new media.Output({ format: new media.Mp4OutputFormat({ fastStart: 'in-memory' }), target })
    const conversion = await media.Conversion.init({
      input,
      output,
      video: { ...size, fit: 'contain', codec: 'avc', quality: media.QUALITY_MEDIUM },
      // AAC audio is copied as it is; other audio is encoded as AAC, or the video is left alone (see below).
      audio: { codec: 'aac' },
    })
    // A dropped track (for example, audio that this browser cannot encode) would lose content: keep the original.
    if (!conversion.isValid || conversion.discardedTracks.length > 0) return file
    conversion.onProgress = options.onProgress
    const cancel = () => void conversion.cancel()
    options.signal?.addEventListener('abort', cancel, { once: true })
    try {
      await conversion.execute()
    } finally {
      options.signal?.removeEventListener('abort', cancel)
    }
    if (!target.buffer || target.buffer.byteLength >= file.size) return file
    const name = file.name.replace(/\.[^.]*$/, '') || 'video'
    return new File([target.buffer], `${name}.mp4`, { type: 'video/mp4', lastModified: file.lastModified })
  } catch (error) {
    if (options.signal?.aborted) throw error
    return file
  } finally {
    input.dispose()
  }
}
