import { useEffect, useRef, useState } from 'react'
import { useUploadAttachment } from '../../api/mutations'
import type { Attachment } from '../../api/types'

export interface Upload {
  id: string
  file: File
  /** 0 to 1. */
  progress: number
  status: 'uploading' | 'failed' | 'done'
  attachment?: Attachment
}

/** The files of one composer: each uploads on its own, with progress, cancel and retry. */
export function useUploads() {
  const upload = useUploadAttachment()
  const [uploads, setUploads] = useState<Upload[]>([])
  const controllers = useRef(new Map<string, AbortController>())

  // Leaving the composer cancels what is still uploading.
  useEffect(() => {
    const running = controllers.current
    return () => {
      for (const controller of running.values()) controller.abort()
    }
  }, [])

  function patch(id: string, change: Partial<Upload>) {
    setUploads((current) => current.map((item) => (item.id === id ? { ...item, ...change } : item)))
  }

  function start(id: string, file: File) {
    const controller = new AbortController()
    controllers.current.set(id, controller)
    upload(file, { signal: controller.signal, onProgress: (progress) => patch(id, { progress }) })
      .then(
        (attachment) => patch(id, { status: 'done', progress: 1, attachment }),
        () => {
          // A cancelled upload is removed by `remove`; anything else failed.
          if (!controller.signal.aborted) patch(id, { status: 'failed' })
        },
      )
      .finally(() => {
        if (controllers.current.get(id) === controller) controllers.current.delete(id)
      })
  }

  return {
    uploads,
    add(files: Iterable<File>) {
      const added = Array.from(files, (file): Upload => ({ id: crypto.randomUUID(), file, progress: 0, status: 'uploading' }))
      if (added.length === 0) return
      setUploads((current) => [...current, ...added])
      for (const item of added) start(item.id, item.file)
    },
    /** Cancels an upload in progress, or takes a finished or failed file out. */
    remove(id: string) {
      controllers.current.get(id)?.abort()
      setUploads((current) => current.filter((item) => item.id !== id))
    },
    retry(id: string) {
      const item = uploads.find((candidate) => candidate.id === id)
      if (!item) return
      patch(id, { status: 'uploading', progress: 0 })
      start(id, item.file)
    },
    /** After a send: the files now belong to the message. */
    clear() {
      setUploads([])
    },
  }
}
