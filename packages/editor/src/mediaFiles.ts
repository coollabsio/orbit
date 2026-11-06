// Which player a file block or a lone link should show. The block type stays `file`: video and audio
// blocks are not in the schema, and existing documents already store these files as file blocks.

const VIDEO_EXTENSIONS = new Set(['mp4', 'm4v', 'webm', 'mov', 'ogv'])
const AUDIO_EXTENSIONS = new Set(['mp3', 'wav', 'ogg', 'oga', 'm4a', 'flac', 'opus', 'aac'])
const TEXT_EXTENSIONS = new Set(['txt', 'md', 'markdown', 'csv', 'json', 'log', 'yaml', 'yml', 'tsv', 'xml'])

export type FilePreviewKind = 'video' | 'audio' | 'pdf' | 'text' | 'file'

export type MediaEmbed = { kind: 'iframe' | 'video' | 'audio'; src: string; title: string }

/** The extension of a file name or a URL path, without the dot. Empty when there is none. */
export function fileExtension(value: string): string {
  const path = value.split(/[?#]/, 1)[0] ?? ''
  const name = path.slice(path.lastIndexOf('/') + 1)
  const dot = name.lastIndexOf('.')
  if (dot <= 0) return ''
  const extension = name.slice(dot + 1).toLowerCase()
  return /^[a-z0-9]{1,8}$/.test(extension) ? extension : ''
}

/** The player for a file. `name` wins; a URL path is the fallback (a link has no separate name). */
export function filePreviewKind(name: string, url = ''): FilePreviewKind {
  const extension = fileExtension(name) || fileExtension(url)
  if (extension === 'pdf') return 'pdf'
  if (VIDEO_EXTENSIONS.has(extension)) return 'video'
  if (AUDIO_EXTENSIONS.has(extension)) return 'audio'
  if (TEXT_EXTENSIONS.has(extension)) return 'text'
  return 'file'
}

/** A page file or a task attachment (`/api/...`). External `https` URLs are not. */
export function isAppFileUrl(url: string): boolean {
  return url.startsWith('/') && !url.startsWith('//') && !url.startsWith('/\\')
}

/**
 * True when the file block should show a player instead of the file row.
 * Text and PDF are read with `fetch`, which only works for our own files. Video and audio can also
 * be an `https` URL: the page policy allows `https` media.
 */
export function showsFilePreview(kind: FilePreviewKind, url: string): boolean {
  if (kind === 'file' || url === '') return false
  if (kind === 'video' || kind === 'audio') return isAppFileUrl(url) || isHttps(url)
  return isAppFileUrl(url)
}

/** A YouTube, Vimeo, or direct audio/video URL. Anything else stays a link. */
export function mediaEmbed(href: string, title: string): MediaEmbed | null {
  let url: URL
  try {
    url = new URL(href)
  } catch {
    return null
  }
  if (url.protocol !== 'https:') return null
  const youtube = youtubeEmbed(url)
  if (youtube) return { kind: 'iframe', src: youtube, title }
  const vimeo = vimeoEmbed(url)
  if (vimeo) return { kind: 'iframe', src: vimeo, title }
  const kind = filePreviewKind('', url.pathname)
  if (kind === 'video' || kind === 'audio') return { kind, src: url.toString(), title }
  return null
}

/** True when the bytes start with the PDF marker `%PDF-`. */
export function isPdfBytes(bytes: Uint8Array): boolean {
  return bytes.length >= 5 && bytes[0] === 0x25 && bytes[1] === 0x50 && bytes[2] === 0x44 && bytes[3] === 0x46 && bytes[4] === 0x2d
}

function isHttps(url: string): boolean {
  try {
    return new URL(url).protocol === 'https:'
  } catch {
    return false
  }
}

function youtubeEmbed(url: URL): string | null {
  const host = url.hostname.toLowerCase().replace(/^www\./, '')
  const parts = url.pathname.split('/').filter(Boolean)
  let id = ''
  if (host === 'youtu.be') id = parts[0] ?? ''
  else if (host === 'youtube.com' || host === 'm.youtube.com' || host === 'music.youtube.com' || host === 'youtube-nocookie.com') {
    if (parts[0] === 'watch') id = url.searchParams.get('v') ?? ''
    else if (parts[0] === 'shorts' || parts[0] === 'live' || parts[0] === 'embed') id = parts[1] ?? ''
    else return null
  } else return null
  if (!/^[A-Za-z0-9_-]{11}$/.test(id)) return null
  return `https://www.youtube-nocookie.com/embed/${id}`
}

function vimeoEmbed(url: URL): string | null {
  const host = url.hostname.toLowerCase().replace(/^www\./, '')
  const parts = url.pathname.split('/').filter(Boolean)
  let id = ''
  let hash = url.searchParams.get('h') ?? ''
  if (host === 'player.vimeo.com') {
    if (parts[0] !== 'video') return null
    id = parts[1] ?? ''
  } else if (host === 'vimeo.com') {
    id = parts[0] ?? ''
    if (!hash && parts[1] && /^[A-Za-z0-9]+$/.test(parts[1])) hash = parts[1]
  } else return null
  if (!/^\d+$/.test(id)) return null
  const src = new URL(`https://player.vimeo.com/video/${id}`)
  if (hash && /^[A-Za-z0-9]+$/.test(hash)) src.searchParams.set('h', hash)
  return src.toString()
}
