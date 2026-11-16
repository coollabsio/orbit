// Previews of the images in the text (§9.1 U8). The text keeps `cid:{content_id}` URLs, which is what the message
// sends; the editor shows each one from the draft file's bytes (`GET drafts/{id}/files/{file_id}`), or at once from
// the pasted file. Object URLs are revoked when the composer closes.

export interface InlineImages {
  /** The URL the editor shows for an image URL: an object URL for `cid:`, other URLs as they are. */
  resolve: (url: string) => Promise<string>
  /** A file just uploaded: its preview comes from the local copy. */
  put: (contentId: string, blob: Blob) => void
  /** The draft's files, to find the file of a `cid:`. */
  setFiles: (files: readonly { id: string; content_id?: string | null }[]) => void
  /** Revokes the object URLs (the next `resolve` loads again). */
  dispose: () => void
}

export function createInlineImages(load: (fileId: string) => Promise<Blob>, files: readonly { id: string; content_id?: string | null }[] = []): InlineImages {
  const fileOf = new Map<string, string>()
  const urls = new Map<string, Promise<string>>()
  const created: string[] = []
  // bumped by `dispose`: a load that started before it ends with its URL revoked at once
  let generation = 0
  const objectUrl = (blob: Blob, started = generation) => {
    const url = URL.createObjectURL(blob)
    if (started !== generation) {
      URL.revokeObjectURL(url)
      return ''
    }
    created.push(url)
    return url
  }
  const setFiles = (list: readonly { id: string; content_id?: string | null }[]) => {
    for (const file of list) if (file.content_id?.trim()) fileOf.set(file.content_id.trim(), file.id)
  }
  setFiles(files)
  return {
    resolve: (url) => {
      if (!url.toLowerCase().startsWith('cid:')) return Promise.resolve(url)
      const contentId = url.slice(4).trim()
      const known = urls.get(contentId)
      if (known) return known
      const fileId = fileOf.get(contentId)
      // not a file of this draft (yet): nothing to show, and nothing cached, so a later try can find it
      if (!fileId) return Promise.resolve('')
      const started = generation
      const loading = load(fileId).then((blob) => objectUrl(blob, started), () => {
        urls.delete(contentId)
        return ''
      })
      urls.set(contentId, loading)
      return loading
    },
    put: (contentId, blob) => void urls.set(contentId, Promise.resolve(objectUrl(blob))),
    setFiles,
    dispose: () => {
      generation += 1
      for (const url of created.splice(0)) URL.revokeObjectURL(url)
      urls.clear()
    },
  }
}
