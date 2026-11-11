/**
 * Where a tapped push notification leads. The server sends the web app's path (`url` beside `aps`); the app opens the
 * screen that shows the same thing. No React.
 */

/** The `/profile/*` pages the app has, under Settings. */
const ACCOUNT_PAGES = new Set(['notifications', 'security', 'sessions', 'appearance', 'agentic'])

/**
 * The app route for a web path: `/chat/<id>?thread=<root>` → `/chat/thread/<root>`, `/views/<view>/<task>` →
 * `/tasks/<task>`, `/profile/notifications` → `/settings/notifications`. `null` for a path the app has no screen for.
 */
export function pushRoute(url: string): string | null {
  const match = /^(?:[a-z]+:\/\/[^/]+)?(\/[^?#]*)?(?:\?([^#]*))?/i.exec(url.trim())
  const path = match?.[1] ?? '/'
  // React Native's URLSearchParams cannot read: the one key the server sends is read by hand
  const threadParam = /(?:^|&)thread=([^&]+)/.exec(match?.[2] ?? '')?.[1]
  const parts = path.split('/').filter(Boolean).map(decodeURIComponent)
  const [section, first, second, third] = parts

  switch (section) {
    case undefined:
      return '/'
    case 'chat': {
      if (!first) return '/chat'
      if (first === 'unreads' || first === 'threads' || first === 'browse') return `/chat/${first}`
      const thread = second === 'thread' && third ? third : threadParam && decodeURIComponent(threadParam)
      return thread ? `/chat/thread/${thread}` : `/chat/${first}`
    }
    case 'tasks': {
      // `/tasks/pulls/<owner>/<repo>/<number>[?comment=<id>]`: a pull request (from a review notification)
      if (first === 'pulls') {
        const fourth = parts[4]
        if (!second || !third || !fourth || !/^\d+$/.test(fourth)) return '/tasks'
        const comment = /(?:^|&)comment=(\d+)/.exec(match?.[2] ?? '')?.[1]
        const base = `/tasks/pulls/${encodeURIComponent(second)}/${encodeURIComponent(third)}/${fourth}`
        return comment ? `${base}?comment=${comment}` : base
      }
      if (first === 'projects') return second && third === 'pulls' ? `/tasks/project/${encodeURIComponent(second)}/pulls` : '/tasks'
      return first ? `/tasks/${first}` : '/tasks'
    }
    case 'views':
      return second ? `/tasks/${second}` : '/tasks'
    case 'docs':
      return first && first !== 'import' ? (first === 'trash' ? '/docs/trash' : `/docs/${first}`) : '/docs'
    case 'activity':
    case 'inbox':
      return '/activity'
    case 'profile':
    case 'settings':
      // the account GitHub page; `/settings/github` is the workspace's GitHub App
      if (section === 'profile' && first === 'github') return '/settings/github-account'
      if (first && ACCOUNT_PAGES.has(first)) return `/settings/${first}`
      return section === 'profile' && (!first || first === 'general') ? '/settings/profile' : '/settings'
    default:
      return null
  }
}

/**
 * The web path a notification carries. On iOS the remote payload is in `trigger.payload` (expo-notifications puts only
 * an Expo-style `body` key in `content.data`); `content.data.url` covers a notification shaped the other way.
 */
export function pushUrl(request: { content: { data?: Record<string, unknown> | null }; trigger?: unknown }): string | null {
  const payload = (request.trigger as { payload?: Record<string, unknown> } | null | undefined)?.payload
  const url = payload?.url ?? request.content.data?.url
  return typeof url === 'string' && url.length > 0 ? url : null
}
