import { useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Spinner } from '@/components/ui/spinner'
import { validCoverUrl } from '@/features/docs/coverLib'

/** Raster types the server serves inline (anything else would download instead of rendering). */
export const COVER_IMAGE_TYPES = 'image/png,image/jpeg,image/gif,image/webp,image/avif'

/**
 * Cover image source: upload an image to the page, or paste an http(s) image URL.
 * `onUpload` stores the file and resolves to its URL (reject with a user-facing message).
 */
export function CoverSourcePanel({
  onPicked,
  onUpload,
}: {
  onPicked: (url: string) => void
  onUpload?: (file: File) => Promise<string>
}) {
  const [url, setUrl] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [uploading, setUploading] = useState(false)
  const fileInput = useRef<HTMLInputElement>(null)

  const submit = () => {
    const valid = validCoverUrl(url)
    if (!valid) {
      setError('Enter an http(s) image URL.')
      return
    }
    setError(null)
    onPicked(valid)
  }

  const upload = async (file: File | undefined) => {
    if (!file || !onUpload) return
    setError(null)
    setUploading(true)
    try {
      const uploaded = await onUpload(file)
      onPicked(uploaded)
    } catch (uploadError) {
      setError(uploadError instanceof Error && uploadError.message ? uploadError.message : 'Could not upload the image.')
    } finally {
      setUploading(false)
      if (fileInput.current) fileInput.current.value = ''
    }
  }

  return (
    <div className="flex flex-col gap-2.5">
      {onUpload ? (
        <>
          <input
            ref={fileInput}
            type="file"
            accept={COVER_IMAGE_TYPES}
            className="hidden"
            aria-label="Cover image file"
            onChange={(e) => void upload(e.target.files?.[0])}
          />
          <Button type="button" variant="outline" disabled={uploading} onClick={() => fileInput.current?.click()}>
            {uploading ? <Spinner /> : null}
            {uploading ? 'Uploading…' : 'Upload image'}
          </Button>
          <p className="text-center text-xs text-muted-foreground">or paste a link</p>
        </>
      ) : null}
      <div className="flex gap-2">
        <Input
          className="min-w-0 flex-1"
          value={url}
          placeholder="https://… image URL"
          aria-label="Image URL"
          aria-invalid={error ? true : undefined}
          autoFocus
          disabled={uploading}
          onChange={(e) => {
            setUrl(e.target.value)
            setError(null)
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && url.trim()) {
              e.preventDefault()
              submit()
            }
          }}
        />
        <Button type="button" variant="outline" disabled={!url.trim() || uploading} onClick={submit}>
          Use
        </Button>
      </div>
      {error ? <p className="text-xs text-destructive">{error}</p> : null}
    </div>
  )
}
