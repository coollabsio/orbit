/** Longest image edge kept on upload; enough for a full-width view on a high-density screen. */
export const MAX_IMAGE_EDGE = 2560
/** Smaller images are uploaded as they are: re-encoding saves little and loses quality. */
export const SHRINK_MIN_BYTES = 512 * 1024
const QUALITY = 0.85
/**
 * Photo types. The server makes a PNG (most often a screenshot) a lossless WebP, which a canvas cannot encode, so
 * a PNG is uploaded as it is. GIF and SVG are never changed: GIF is often animated, SVG is not pixels.
 */
const SHRINKABLE_TYPES = new Set(['image/jpeg', 'image/webp'])

/** The size an image is drawn at: the long edge at most `MAX_IMAGE_EDGE`, same aspect ratio. */
export function targetSize(width: number, height: number): { width: number; height: number } {
  const scale = Math.min(1, MAX_IMAGE_EDGE / Math.max(width, height))
  return { width: Math.max(1, Math.round(width * scale)), height: Math.max(1, Math.round(height * scale)) }
}

/** Animated WebP (an `ANIM` chunk) would lose all frames but the first; `acTL` marks an animated PNG. */
export function isAnimated(header: Uint8Array): boolean {
  const text = String.fromCharCode(...header)
  return text.includes('acTL') || text.includes('ANIM')
}

/**
 * Makes a large photo smaller before upload: scales it to `MAX_IMAGE_EDGE` and encodes it as WebP.
 * Returns the original file when it is small, animated, cannot be decoded or encoded, or would not get smaller.
 */
export async function shrinkImage(file: File): Promise<File> {
  if (!SHRINKABLE_TYPES.has(file.type) || file.size < SHRINK_MIN_BYTES || typeof createImageBitmap !== 'function') return file
  try {
    // Both animation chunks come before the image data, near the start of the file.
    if (isAnimated(new Uint8Array(await file.slice(0, 4096).arrayBuffer()))) return file
    const bitmap = await createImageBitmap(file)
    const size = targetSize(bitmap.width, bitmap.height)
    const canvas = document.createElement('canvas')
    canvas.width = size.width
    canvas.height = size.height
    const context = canvas.getContext('2d')
    if (!context) {
      bitmap.close()
      return file
    }
    context.imageSmoothingQuality = 'high'
    context.drawImage(bitmap, 0, 0, size.width, size.height)
    bitmap.close()
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/webp', QUALITY))
    // A browser that cannot encode WebP returns PNG, which is often larger.
    if (!blob || blob.type !== 'image/webp' || blob.size >= file.size) return file
    const name = file.name.replace(/\.[^.]*$/, '') || 'image'
    return new File([blob], `${name}.webp`, { type: 'image/webp', lastModified: file.lastModified })
  } catch {
    return file
  }
}
