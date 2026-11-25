import { useEffect, useState } from 'react'
import { useLocation, useParams } from 'react-router'
import { ArrowUpRightSquare } from 'reicon-react'
import { customPageUrl } from '@orbit/api/customPages'
import { Button, buttonVariants } from '@/components/ui/button'
import { LoadingText } from '@/components/ui/spinner'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
import { InfoTip } from '@/components/common/InfoTip'
import { useWorkspace } from '@/features/workspaces/workspaceContext'
import { useCustomPages } from './api'

// Scripts, forms and popups run, in the page's own origin (never Orbit's: the server only takes other origins).
// No allow-top-navigation: the page cannot move the Orbit tab.
const SANDBOX = 'allow-scripts allow-same-origin allow-forms allow-popups allow-popups-to-escape-sandbox allow-downloads'

/** `/pages/{slug}/*`: a custom page, full height, in a sandboxed iframe. */
export function CustomPageView() {
  const { workspace } = useWorkspace()
  const { slug = '', '*': rest = '' } = useParams()
  const { search } = useLocation()
  const pages = useCustomPages(workspace.id)
  const page = pages.data?.items.find((item) => item.slug === slug)
  const url = page ? customPageUrl(page.url, rest, search) : null
  const framed = url !== null && page !== undefined && !pages.data?.blocked.includes(page.id)
  // The tab's CSP has the page origins the user saw when the tab loaded: a page added since then is still blocked here.
  const [blockedUrl, setBlockedUrl] = useState<string>()
  useEffect(() => {
    if (!url) return
    const origin = new URL(url).origin
    const onViolation = (event: SecurityPolicyViolationEvent) => {
      if (event.effectiveDirective === 'frame-src' && event.blockedURI.startsWith(origin)) setBlockedUrl(url)
    }
    document.addEventListener('securitypolicyviolation', onViolation)
    return () => document.removeEventListener('securitypolicyviolation', onViolation)
  }, [url])

  if (pages.isPending) return <Pane><div className="p-4"><LoadingText>Loading page…</LoadingText></div></Pane>
  if (!page || !url) {
    return <Pane><div className="flex flex-1 items-center justify-center p-6 text-center text-muted-foreground" role={pages.isError ? 'alert' : undefined}>
      {pages.isError ? 'The page could not be loaded.' : 'This page does not exist, or you cannot see it.'}
    </div></Pane>
  }
  return (
    <Pane>
      <PaneHeader>
        {page.icon ? <span aria-hidden="true" className="text-base leading-none">{page.icon}</span> : null}
        <PaneTitle render={<h1 />} className="m-0">{page.name}</PaneTitle>
        {/* The site's host stays visible, so a framed page cannot pass for Orbit itself (for example a fake sign-in). */}
        <span className="truncate text-xs text-muted-foreground" title={url}>{new URL(url).host}</span>
        {framed ? <InfoTip text="Empty page? The site does not let Orbit frame it. Its frame-ancestors must allow this Orbit, or open it in a new tab." /> : null}
        <a href={url} target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer" className={buttonVariants({ variant: 'ghost', size: 'sm', className: 'ml-auto' })}>
          <ArrowUpRightSquare className="size-4" /> Open in new tab
        </a>
      </PaneHeader>
      {framed && blockedUrl === url ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 p-6 text-center text-muted-foreground" role="status">
          <p className="m-0 font-medium text-foreground">Reload Orbit to show this page.</p>
          <p className="m-0">The page was added after this tab opened.</p>
          <Button type="button" variant="outline" onClick={() => window.location.reload()}>Reload</Button>
        </div>
      ) : framed ? (
        // keyed by URL: a new address loads afresh instead of keeping the frame's own navigation
        <iframe key={url} title={page.name} src={url} sandbox={SANDBOX} referrerPolicy="no-referrer" allow="fullscreen; clipboard-write" className="min-h-0 w-full flex-1 border-0 bg-background" />
      ) : (
        <div className="flex flex-1 flex-col items-center justify-center gap-1 p-6 text-center text-muted-foreground" role="status">
          <p className="m-0 font-medium text-foreground">Orbit cannot show this page here.</p>
          <p className="m-0">Its site is not on this Orbit's list of allowed origins (Admin → General). Open it in a new tab.</p>
        </div>
      )}
    </Pane>
  )
}
