import { useContext, type ReactNode } from 'react'
import { InternalLinkContext } from '@/lib/internalLinkContext'
import { appNavigate, isPlainClick } from '@/lib/navigateBridge'

/**
 * A link to this app in rendered text: it navigates in place. A bare link (no `children`) shows its URL, unless the host
 * draws it (`InternalLinkContext`).
 */
export function InternalLink({ url, path, children, className = 'text-primary hover:underline' }: { url: string; path: string; children?: ReactNode; className?: string }) {
  const render = useContext(InternalLinkContext)
  const plain = (
    <a
      href={path}
      className={className}
      onClick={(e) => {
        if (!isPlainClick(e)) return
        e.preventDefault()
        appNavigate(path)
      }}
    >
      {children ?? url}
    </a>
  )
  return render && children === undefined ? render(url, plain) : plain
}
