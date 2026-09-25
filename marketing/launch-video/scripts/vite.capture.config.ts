// Vite config for marketing captures of the upcoming Mail / Chat features.
// Loads apps/web/vite.config.ts unchanged and adds a pre-plugin that rewrites a few modules IN MEMORY at serve
// time (nothing under apps/ is written):
//   - App.tsx: real routes for mail, chat (incl. settings + full-screen thread) and dm instead of redirects
//   - SidebarNav.tsx: Mail / Chat / Direct messages enabled
//   - productNavigation.ts: no disabled product paths
//   - MockFeatureBadge.tsx: renders nothing
//   - mock/seed/{users,chat,mail,customEmojis}.ts: replaced by ./seed/*.ts (marketing content, same shapes)
// Run (from apps/web so root and node_modules resolve):
//   cd apps/web && VITE_API_PROXY=http://127.0.0.1:18080 ./node_modules/.bin/vite \
//     --config ../../marketing/launch-video/scripts/vite.capture.config.ts
import { existsSync, readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import type { ConfigEnv, Plugin, UserConfig } from 'vite'

const here = dirname(fileURLToPath(import.meta.url))
const repo = resolve(here, '../../..')
const webRoot = join(repo, 'apps/web')
const seedDir = join(here, 'seed')
const SWAP_SEEDS = process.env.CAPTURE_ORIGINAL_SEED !== '1'

function replaceOrThrow(code: string, id: string, search: string | RegExp, replacement: string): string {
  const next = code.replace(search, replacement)
  if (next === code) throw new Error(`[orbit-capture] pattern not found in ${id}: ${String(search)}`)
  return next
}

const norm = (id: string) => id.split('?')[0].replaceAll('\\', '/')

function capturePlugin(): Plugin {
  return {
    name: 'orbit-capture-enable-mail-chat',
    enforce: 'pre',
    // Everything happens in `load` (raw source from disk): other pre plugins (babel / react compiler) already
    // compile JSX in their `transform`, so the TSX text would no longer match there.
    load(rawId) {
      const id = norm(rawId)
      const seed = id.match(/\/apps\/web\/src\/mock\/seed\/(users|chat|mail|customEmojis)\.ts$/)
      if (seed) {
        const file = join(seedDir, `${seed[1]}.ts`)
        return SWAP_SEEDS && existsSync(file) ? readFileSync(file, 'utf8') : null
      }
      if (!/\/apps\/web\/src\/app\/(App\.tsx|shell\/(SidebarNav\.tsx|productNavigation\.ts|MockFeatureBadge\.tsx))$/.test(id)) return null
      const code = readFileSync(id, 'utf8')
      if (id.endsWith('/apps/web/src/app/App.tsx')) {
        let out = replaceOrThrow(
          code,
          id,
          "import { InboxPage } from '@/features/inbox/pages/InboxPage'",
          [
            "import { InboxPage } from '@/features/inbox/pages/InboxPage'",
            "import { MailPage } from '@/features/mail/pages/MailPage'",
            "import { ChatPage } from '@/features/chat/pages/ChatPage'",
            "import { DMPage } from '@/features/chat/pages/DMPage'",
            "import { ServerSettingsPage } from '@/features/chat/pages/ServerSettingsPage'",
          ].join('\n'),
        )
        out = replaceOrThrow(out, id, /<Route path="mail\/\*" element=\{<Navigate to="\/tasks" replace \/>\} \/>/, '<Route path="mail" element={<MailPage />} /><Route path="mail/:threadId" element={<MailPage />} />')
        out = replaceOrThrow(
          out,
          id,
          /<Route path="chat\/\*" element=\{<Navigate to="\/tasks" replace \/>\} \/>/,
          '<Route path="chat" element={<ChatPage />} /><Route path="chat/settings" element={<ServerSettingsPage />} /><Route path="chat/:channelId" element={<ChatPage />} /><Route path="chat/:channelId/thread/:rootId" element={<ChatPage />} />',
        )
        out = replaceOrThrow(out, id, /<Route path="dm\/\*" element=\{<Navigate to="\/tasks" replace \/>\} \/>/, '<Route path="dm" element={<DMPage />} /><Route path="dm/:dmId" element={<DMPage />} />')
        return { code: out, map: null }
      }
      if (id.endsWith('/apps/web/src/app/shell/SidebarNav.tsx')) {
        let out = replaceOrThrow(code, id, /(label: 'Mail', icon: Mail, enabled: )false/, '$1true')
        out = replaceOrThrow(out, id, /(label: 'Chat', icon: MessageSquare, enabled: )false/, '$1true')
        out = replaceOrThrow(
          out,
          id,
          /<Button variant="ghost" className=\{cn\(disabledClass, itemClass\(false\)\)\} disabled title="Direct messages — Coming soon">[\s\S]*?<\/Button>/,
          `<NavLink to="/dm" className={({ isActive }) => itemClass(isActive)} aria-label="Direct messages" title={collapsed ? 'Direct messages' : undefined} onClick={onNavigate}>
          <MessagesSquare className="size-[18px] shrink-0 opacity-90" />
          <span className={labelClass}>Direct messages</span>
        </NavLink>`,
        )
        return { code: out, map: null }
      }
      if (id.endsWith('/apps/web/src/app/shell/productNavigation.ts')) {
        return { code: replaceOrThrow(code, id, /export const disabledProductPaths = \[[^\]]*\] as const/, 'export const disabledProductPaths: readonly string[] = []'), map: null }
      }
      if (id.endsWith('/apps/web/src/app/shell/MockFeatureBadge.tsx')) {
        return { code: 'export function MockFeatureBadge() {\n  return null\n}\n', map: null }
      }
      return null
    },
  }
}

export default async (env: ConfigEnv): Promise<UserConfig> => {
  // vite lives in apps/web/node_modules; resolve it from there (marketing/ has no vite install)
  const require = createRequire(join(webRoot, 'package.json'))
  const vite = (await import(pathToFileURL(require.resolve('vite')).href)) as typeof import('vite')
  const loaded = await vite.loadConfigFromFile(env, join(webRoot, 'vite.config.ts'), webRoot)
  if (!loaded) throw new Error('could not load apps/web/vite.config.ts')
  return vite.mergeConfig(loaded.config, {
    root: webRoot,
    // own dep-optimizer cache so the user's dev server (same apps/web) is never disturbed
    cacheDir: join(here, '../.vite-capture-cache'),
    plugins: [capturePlugin()],
    server: { port: 18889, strictPort: true, host: '127.0.0.1' },
  })
}
