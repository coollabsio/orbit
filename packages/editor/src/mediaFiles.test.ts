import { describe, expect, test } from 'bun:test'
import { filePreviewKind, isPdfBytes, mediaEmbed, showsFilePreview } from './mediaFiles'

const PAGE_FILE = '/api/v1/workspaces/0199a0b0-0000-7000-8000-0000000000a1/pages/0199a0b0-0000-7000-8000-0000000000b2/files/0199a0b0-0000-7000-8000-0000000000c3'

describe('filePreviewKind', () => {
  test('uses the file name, then the URL path', () => {
    expect(filePreviewKind('note.txt')).toBe('text')
    expect(filePreviewKind('orbit.PDF')).toBe('pdf')
    expect(filePreviewKind('beep.wav')).toBe('audio')
    expect(filePreviewKind('flower.mp4')).toBe('video')
    expect(filePreviewKind('archive.tar.gz')).toBe('file')
    expect(filePreviewKind('', 'https://cdn.example.com/clip.webm?token=1')).toBe('video')
    expect(filePreviewKind('clip', PAGE_FILE)).toBe('file')
  })

  test('shows a preview only for a player we can load', () => {
    expect(showsFilePreview('text', PAGE_FILE)).toBe(true)
    expect(showsFilePreview('pdf', PAGE_FILE)).toBe(true)
    expect(showsFilePreview('text', 'https://example.com/note.txt')).toBe(false)
    expect(showsFilePreview('video', 'https://cdn.example.com/flower.mp4')).toBe(true)
    expect(showsFilePreview('video', 'http://cdn.example.com/flower.mp4')).toBe(false)
    expect(showsFilePreview('file', PAGE_FILE)).toBe(false)
    expect(showsFilePreview('audio', '')).toBe(false)
  })
})

describe('mediaEmbed', () => {
  test('turns a YouTube or Vimeo URL into its player, and a media file into a player', () => {
    expect(mediaEmbed('https://youtu.be/dQw4w9WgXcQ', 'External video')).toEqual({
      kind: 'iframe',
      src: 'https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ',
      title: 'External video',
    })
    expect(mediaEmbed('https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=3', 'Watch')?.src).toBe('https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ')
    expect(mediaEmbed('https://youtube.com/shorts/dQw4w9WgXcQ', 'Short')?.src).toBe('https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ')
    expect(mediaEmbed('https://vimeo.com/226053498', 'Vimeo')?.src).toBe('https://player.vimeo.com/video/226053498')
    expect(mediaEmbed('https://vimeo.com/226053498/abcdef', 'Unlisted')?.src).toBe('https://player.vimeo.com/video/226053498?h=abcdef')
    expect(mediaEmbed('https://cdn.example.com/flower.mp4', 'Clip')).toEqual({ kind: 'video', src: 'https://cdn.example.com/flower.mp4', title: 'Clip' })
    expect(mediaEmbed('https://cdn.example.com/beep.wav', 'Beep')?.kind).toBe('audio')
  })

  test('leaves a normal link, a short id, and plain http alone', () => {
    expect(mediaEmbed('https://example.com/docs', 'Docs')).toBeNull()
    expect(mediaEmbed('https://youtu.be/short', 'Bad')).toBeNull()
    expect(mediaEmbed('http://youtu.be/dQw4w9WgXcQ', 'Http')).toBeNull()
    expect(mediaEmbed('javascript:alert(1)', 'Script')).toBeNull()
  })
})

test('a PDF starts with %PDF-', () => {
  expect(isPdfBytes(new Uint8Array([0x25, 0x50, 0x44, 0x46, 0x2d, 0x31]))).toBe(true)
  expect(isPdfBytes(new TextEncoder().encode('hello'))).toBe(false)
})
