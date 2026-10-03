const AVATAR_SIZE = 256

/**
 * Center-crops a picture to a square and scales it to 256 px, so the upload stays far below the
 * server's 512 KiB limit. Browsers that cannot encode WebP (older Safari) fall back to PNG,
 * which the server accepts too.
 */
export async function avatarImage(file: Blob): Promise<Blob> {
  const bitmap = await createImageBitmap(file)
  const side = Math.min(bitmap.width, bitmap.height)
  const canvas = document.createElement('canvas')
  canvas.width = AVATAR_SIZE
  canvas.height = AVATAR_SIZE
  const context = canvas.getContext('2d')
  if (!context) throw new Error('Canvas is not available.')
  context.imageSmoothingQuality = 'high'
  context.drawImage(bitmap, (bitmap.width - side) / 2, (bitmap.height - side) / 2, side, side, 0, 0, AVATAR_SIZE, AVATAR_SIZE)
  bitmap.close()
  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error('The image could not be encoded.'))), 'image/webp', 0.9)
  })
}
