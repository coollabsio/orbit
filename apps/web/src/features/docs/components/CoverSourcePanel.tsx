import { useRef, useState } from 'react'
import { Export } from 'reicon-react'

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
    <div className="doc-source-panel">
      <button type="button" className="button" onClick={() => fileInput.current?.click()}>
        <Export size={14} />
        Upload from my computer
      </button>
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
      <div className="doc-source-divider">
        <span />
        <span className="text-faint text-xs">or</span>
        <span />
      </div>
      <div className="doc-source-url">
        <input
          className="input"
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
        <button type="button" className="button" disabled={!url.trim()} onClick={() => onPicked(url.trim())}>
          Use
        </button>
      </div>
      {error ? <p className="text-faint text-xs">{error}</p> : null}
    </div>
  )
}
