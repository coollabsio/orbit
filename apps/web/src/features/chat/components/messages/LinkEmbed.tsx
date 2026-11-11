import { useState, type ComponentProps } from 'react'
import { Heart, Message as Replies, Play, Repeat } from 'reicon-react'
import { useLinkPreview } from '../../api/queries'
import type { LinkPreview } from '../../api/types'

const COUNT = new Intl.NumberFormat(undefined, { notation: 'compact', maximumFractionDigits: 1 })
const POSTED = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric', year: 'numeric' })

/** An image of another site. Without a source, and when it does not load, it takes no space. The site does not learn where the reader is. */
function EmbedImage({ src, className, ...props }: Omit<ComponentProps<'img'>, 'alt'>) {
  const [failed, setFailed] = useState(false)
  if (!src || failed) return null
  return <img src={src} alt="" referrerPolicy="no-referrer" className={className} onError={() => setFailed(true)} {...props} />
}

/** The mark of a video on its poster. */
function PlayMark() {
  return (
    <span aria-hidden="true" className="absolute top-1/2 left-1/2 flex size-11 -translate-1/2 items-center justify-center rounded-full bg-black/65 text-white backdrop-blur-sm transition-transform group-hover/play:scale-105">
      <Play weight="Filled" className="ml-0.5 size-5" />
    </span>
  )
}

/** A page: site, title and description, with the page's image wide above or small beside them. */
function PageEmbed({ preview }: { preview: LinkPreview }) {
  return (
    <a
      href={preview.url}
      target="_blank"
      rel="noopener noreferrer"
      data-slot="link-embed"
      data-kind="link"
      className="flex w-full max-w-md min-w-0 flex-col overflow-hidden rounded-lg border bg-card text-[0.8125rem] text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring/50 hover-fine:hover:bg-muted/50"
    >
      {preview.largeImage ? <EmbedImage src={preview.imageUrl} className="aspect-[1.91/1] w-full border-b bg-muted object-cover" /> : null}
      <div className="flex min-w-0 items-start gap-3 p-2.5">
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <div className="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
            <EmbedImage src={preview.iconUrl} className="size-3.5 shrink-0 rounded-[3px]" />
            <span className="truncate">{preview.siteName}</span>
          </div>
          <div className="line-clamp-2 font-medium">{preview.title}</div>
          {preview.description ? <div className="line-clamp-2 text-xs leading-[1.45] text-muted-foreground">{preview.description}</div> : null}
        </div>
        {preview.largeImage ? null : <EmbedImage src={preview.imageUrl} className="size-16 shrink-0 rounded-md bg-muted object-cover" />}
      </div>
    </a>
  )
}

const escapeAttribute = (value: string) => value.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;')

/**
 * The video of a post, played in place. X's video host refuses a request that names another site as its referrer, and a
 * `<video>` has no referrer setting of its own, so the player is a small document of its own that sends none.
 */
function PostVideo({ preview }: { preview: LinkPreview }) {
  const [playing, setPlaying] = useState(false)
  const ratio = preview.videoWidth && preview.videoHeight ? preview.videoWidth / preview.videoHeight : 16 / 9
  return (
    // the poster and the player have the same box, so the list does not move when the video starts
    <div data-slot="link-embed-video" className="relative z-10 max-h-96 w-full overflow-hidden rounded-md border bg-black" style={{ aspectRatio: ratio }}>
      {playing ? (
        <iframe
          title={`Video by ${preview.authorName ?? preview.authorHandle ?? 'X'}`}
          // Sandboxed: an opaque origin with no forms, popups or top navigation. The player runs no script (the CSP
          // allows no inline one), but a sandbox without `allow-scripts` also blocks `autoplay`: Play would not start it.
          sandbox="allow-scripts"
          referrerPolicy="no-referrer"
          allow="autoplay; fullscreen; picture-in-picture"
          allowFullScreen
          className="size-full border-0"
          srcDoc={`<!doctype html><html style="height:100%"><head><meta name="referrer" content="no-referrer"></head><body style="height:100%;margin:0;overflow:hidden;background:#000"><video src="${escapeAttribute(preview.videoUrl ?? '')}" poster="${escapeAttribute(preview.imageUrl ?? '')}" controls autoplay playsinline style="display:block;width:100%;height:100%"></video></body></html>`}
        />
      ) : (
        <button type="button" aria-label="Play video" className="group/play relative block size-full outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset" onClick={() => setPlaying(true)}>
          <EmbedImage src={preview.imageUrl} className="size-full object-cover" />
          <PlayMark />
        </button>
      )}
    </div>
  )
}

/** A post on X: who wrote it, what it says, its video or first image, and its numbers. The video plays in the card. */
function PostEmbed({ preview }: { preview: LinkPreview }) {
  const counts = [
    { label: 'replies', icon: Replies, count: preview.replies },
    { label: 'reposts', icon: Repeat, count: preview.reposts },
    { label: 'likes', icon: Heart, count: preview.likes },
  ]
  return (
    <div
      data-slot="link-embed"
      data-kind="x"
      className="relative flex w-full max-w-md min-w-0 flex-col gap-2 rounded-lg border bg-card p-3 text-[0.8125rem] text-foreground has-[[data-slot=link-embed-link]:focus-visible]:ring-2 has-[[data-slot=link-embed-link]:focus-visible]:ring-ring/50 hover-fine:has-[[data-slot=link-embed-link]:hover]:bg-muted/50"
    >
      <div className="flex min-w-0 items-center gap-2">
        <EmbedImage src={preview.authorAvatarUrl} className="size-8 shrink-0 rounded-full bg-muted" />
        <div className="flex min-w-0 flex-1 flex-col leading-tight">
          {/* the link covers the card; the video sits above it */}
          <a href={preview.url} target="_blank" rel="noopener noreferrer" data-slot="link-embed-link" className="truncate font-medium outline-none after:absolute after:inset-0 after:rounded-lg">
            {preview.authorName ?? preview.authorHandle}
          </a>
          {preview.authorHandle ? <span className="truncate text-xs text-muted-foreground">@{preview.authorHandle}</span> : null}
        </div>
        <svg viewBox="0 0 24 24" aria-label="X" role="img" className="size-4 shrink-0 fill-current text-muted-foreground">
          <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z" />
        </svg>
      </div>
      {preview.description ? <p className="line-clamp-[10] leading-[1.45] wrap-anywhere whitespace-pre-wrap">{preview.description}</p> : null}
      {preview.videoUrl ? (
        <PostVideo preview={preview} />
      ) : preview.imageUrl ? (
        <span className="block overflow-hidden rounded-md border empty:hidden">
          <EmbedImage src={preview.imageUrl} className="max-h-80 w-full bg-muted object-cover" />
        </span>
      ) : null}
      <div className="flex min-w-0 items-center gap-3 text-xs text-muted-foreground tabular-nums">
        {counts.map(({ label, icon: Icon, count }) =>
          count === undefined ? null : (
            <span key={label} className="flex items-center gap-1" aria-label={`${count} ${label}`}>
              <Icon className="size-3.5" aria-hidden="true" />
              {COUNT.format(count)}
            </span>
          ),
        )}
        {preview.createdAt ? <time dateTime={new Date(preview.createdAt).toISOString()} className="ml-auto truncate">{POSTED.format(preview.createdAt)}</time> : null}
      </div>
    </div>
  )
}

/** A YouTube video: the poster until the reader asks for the player, so YouTube loads nothing before that. */
function VideoEmbed({ preview }: { preview: LinkPreview }) {
  const [playing, setPlaying] = useState(false)
  return (
    <div data-slot="link-embed" data-kind="youtube" className="flex w-full max-w-md min-w-0 flex-col overflow-hidden rounded-lg border bg-card text-[0.8125rem] text-foreground">
      {playing ? (
        <iframe
          src={`https://www.youtube-nocookie.com/embed/${preview.videoId}?autoplay=1`}
          title={preview.title}
          allow="autoplay; encrypted-media; picture-in-picture; fullscreen"
          allowFullScreen
          referrerPolicy="strict-origin-when-cross-origin"
          className="aspect-video w-full border-0 bg-black"
        />
      ) : (
        <button type="button" aria-label={`Play ${preview.title}`} className="group/play relative block aspect-video w-full overflow-hidden bg-black outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset" onClick={() => setPlaying(true)}>
          <EmbedImage src={preview.imageUrl} className="size-full object-cover" />
          <PlayMark />
        </button>
      )}
      <a href={preview.url} target="_blank" rel="noopener noreferrer" className="flex min-w-0 flex-col gap-0.5 border-t p-2.5 outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset hover-fine:hover:bg-muted/50">
        <span className="line-clamp-2 font-medium">{preview.title}</span>
        <span className="truncate text-xs text-muted-foreground">{['YouTube', preview.authorName].filter(Boolean).join(' · ')}</span>
      </a>
    </div>
  )
}

/**
 * The card of a URL on another site: a post on X, a YouTube video, a bare image, or the page's own title, description
 * and image. Nothing shows while the server reads the page, and nothing for a URL without a preview.
 */
export function LinkEmbed({ url }: { url: string }) {
  const external = URL.canParse(url) && new URL(url).origin !== window.location.origin
  const preview = useLinkPreview(url, external)
  if (!preview) return null
  if (preview.kind === 'x') return <PostEmbed preview={preview} />
  if (preview.kind === 'youtube') return <VideoEmbed preview={preview} />
  if (preview.kind === 'image') {
    return (
      <a href={preview.url} target="_blank" rel="noopener noreferrer" data-slot="link-embed" data-kind="image" className="block w-fit max-w-md overflow-hidden rounded-lg outline-none empty:hidden focus-visible:ring-2 focus-visible:ring-ring/50">
        <EmbedImage src={preview.imageUrl} className="max-h-80 max-w-full rounded-lg border" />
      </a>
    )
  }
  return <PageEmbed preview={preview} />
}
