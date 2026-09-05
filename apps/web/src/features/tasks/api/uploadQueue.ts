export class PartialUploadError extends Error {
  readonly completed: number
  readonly remaining: File[]

  constructor(
    cause: unknown,
    completed: number,
    remaining: File[],
  ) {
    super('Some files could not be uploaded.', { cause })
    this.completed = completed
    this.remaining = remaining
  }
}

export async function uploadFiles(
  files: File[],
  upload: (file: File) => Promise<unknown>,
  onProgress: (progress: number) => void,
): Promise<void> {
  for (const [index, file] of files.entries()) {
    try {
      await upload(file)
    } catch (error) {
      throw new PartialUploadError(error, index, files.slice(index))
    }
    onProgress(Math.round(((index + 1) / files.length) * 100))
  }
}
