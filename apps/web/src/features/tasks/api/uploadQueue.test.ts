import { expect, test } from 'bun:test'
import { PartialUploadError, uploadFiles } from './uploadQueue'

test('a partial multi-file failure reports persisted work and resumes only remaining files', async () => {
  const files = [new File(['a'], 'a.txt'), new File(['b'], 'b.txt'), new File(['c'], 'c.txt')]
  const uploaded: string[] = []
  const progress: number[] = []
  let failed = false
  const upload = async (file: File) => {
    if (file.name === 'b.txt' && !failed) {
      failed = true
      throw new Error('offline')
    }
    uploaded.push(file.name)
  }

  let remaining: File[] = []
  try {
    await uploadFiles(files, upload, (value) => progress.push(value))
  } catch (error) {
    expect(error).toBeInstanceOf(PartialUploadError)
    remaining = (error as PartialUploadError).remaining
    expect((error as PartialUploadError).completed).toBe(1)
  }
  await uploadFiles(remaining, upload, (value) => progress.push(value))

  expect(remaining.map((file) => file.name)).toEqual(['b.txt', 'c.txt'])
  expect(uploaded).toEqual(['a.txt', 'b.txt', 'c.txt'])
  expect(progress).toContain(33)
  expect(progress.at(-1)).toBe(100)
})
