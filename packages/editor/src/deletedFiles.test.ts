import { expect, test } from 'bun:test'
import { deletedFileRules, fileBlockUrls } from './deletedFiles'

test('finds the Orbit files of file blocks, nested ones too, once each', () => {
  const file = (url: string) => ({ type: 'file', props: { url } })
  expect(fileBlockUrls([
    file('/api/v1/a'),
    { type: 'image', props: { url: '/api/v1/image' } },
    { type: 'paragraph', children: [file('/api/v1/b'), file('/api/v1/a')] },
    file('https://example.com/x.zip'),
    file(''),
  ])).toEqual(['/api/v1/a', '/api/v1/b'])
})

test('the rules match only file blocks of that URL', () => {
  const rules = deletedFileRules('/api/v1/w/pages/p/files/f')
  expect(rules.every((rule) => rule.startsWith('.bn-block-content[data-content-type="file"][data-url="/api/v1/w/pages/p/files/f"]'))).toBe(true)
  expect(rules.join('\n')).toContain('Deleted file')
})
