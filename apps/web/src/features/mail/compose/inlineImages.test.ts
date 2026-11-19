import { afterEach, expect, spyOn, test } from 'bun:test'
import { createInlineImages } from './inlineImages'

const spies: { mockRestore: () => void }[] = []
afterEach(() => {
  for (const spy of spies.splice(0)) spy.mockRestore()
})

function fakeUrls() {
  let next = 0
  const revoked: string[] = []
  spies.push(spyOn(URL, 'createObjectURL').mockImplementation(() => `blob:preview-${++next}`))
  spies.push(spyOn(URL, 'revokeObjectURL').mockImplementation((url: string) => void revoked.push(url)))
  return revoked
}

test('a cid: image shows the bytes of its draft file; other URLs stay as they are', async () => {
  fakeUrls()
  const loads: string[] = []
  const images = createInlineImages(async (fileId) => {
    loads.push(fileId)
    return new Blob(['png'])
  }, [{ id: 'f1', content_id: 'img-1@orbit' }])
  expect(await images.resolve('https://example.com/a.png')).toBe('https://example.com/a.png')
  expect(await images.resolve('cid:img-1@orbit')).toBe('blob:preview-1')
  // loaded once
  expect(await images.resolve('cid:img-1@orbit')).toBe('blob:preview-1')
  expect(loads).toEqual(['f1'])
  // not a file of this draft
  expect(await images.resolve('cid:other@x')).toBe('')
  // a pasted image shows from its local copy
  images.put('img-2@orbit', new Blob(['png']))
  expect(await images.resolve('cid:img-2@orbit')).toBe('blob:preview-2')
})

test('closing revokes the previews, and a load that ends after it is revoked at once', async () => {
  const revoked = fakeUrls()
  let finish: (blob: Blob) => void = () => {}
  const images = createInlineImages(() => new Promise<Blob>((resolve) => (finish = resolve)), [{ id: 'f1', content_id: 'late@orbit' }])
  images.put('now@orbit', new Blob(['png']))
  const late = images.resolve('cid:late@orbit')
  images.dispose()
  expect(revoked).toEqual(['blob:preview-1'])
  finish(new Blob(['png']))
  expect(await late).toBe('')
  expect(revoked).toEqual(['blob:preview-1', 'blob:preview-2'])
})
