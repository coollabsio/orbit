import type { createImageBlockConfig } from '@blocknote/core'
import { ImageBlock, type ReactCustomBlockRenderProps } from '@blocknote/react'
import { proxiedImageUrl } from './outsideUrl'

/**
 * BlockNote's image block, with an outside image loading through Orbit. The URL is changed before BlockNote sees it:
 * a `resolveFileUrl` would show the outside URL while it resolves. The stored URL stays as it is.
 */
export function ProxiedImageBlock(props: ReactCustomBlockRenderProps<typeof createImageBlockConfig>) {
  const url = proxiedImageUrl(props.block.props.url, window.location.origin)
  if (url === props.block.props.url) return <ImageBlock {...props} />
  return <ImageBlock {...props} block={{ ...props.block, props: { ...props.block.props, url } }} />
}
