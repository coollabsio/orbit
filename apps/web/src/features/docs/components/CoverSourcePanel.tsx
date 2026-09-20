import { useRef, useState } from 'react'
import { Upload } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Separator } from '@/components/ui/separator'

/** Cover image source (the reference app's panel, mock-sized): upload from disk, or paste an image URL. */
export function CoverSourcePanel({ onPicked }: { onPicked: (url: string) => void }) {
  const fileInput = useRef<HTMLInputElement>(null)
  const [url, setUrl] = useState('')
  const [error, setError] = useState<string | null>(null)

  const pickFile = (file: File | undefined) => {
    if (!file) return
    if (!file.type.startsWith('image/')) {
      setError('This file is not an image.')
      return
    }
    onPicked(URL.createObjectURL(file))
  }

  return (
    <div className="flex flex-col gap-2.5">
      <Button type="button" variant="outline" onClick={() => fileInput.current?.click()}>
        <Upload className="size-3.5" />
        Upload from my computer
      </Button>
      <input
        ref={fileInput}
        type="file"
        accept="image/*"
        hidden
        aria-label="Upload cover image"
        onChange={(e) => {
          const file = e.target.files?.[0]
          e.target.value = ''
          pickFile(file)
        }}
      />
      <div className="flex items-center gap-2">
        <Separator className="flex-1" />
        <span className="text-xs text-muted-foreground/70">or</span>
        <Separator className="flex-1" />
      </div>
      <div className="flex gap-2">
        <Input
          className="min-w-0 flex-1"
          value={url}
          placeholder="Image URL"
          aria-label="Image URL"
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && url.trim()) {
              e.preventDefault()
              onPicked(url.trim())
            }
          }}
        />
        <Button type="button" variant="outline" disabled={!url.trim()} onClick={() => onPicked(url.trim())}>
          Use
        </Button>
      </div>
      {error ? <p className="text-xs text-muted-foreground/70">{error}</p> : null}
    </div>
  )
}
