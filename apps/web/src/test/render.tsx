import { render as baseRender, type RenderOptions } from '@testing-library/react'
import type { ReactElement, ReactNode } from 'react'
import { ShortcutProvider } from '@/shortcuts/ShortcutProvider'

/** `render` with the shortcut provider around the tree, as in the app. A given wrapper goes inside it. */
export function render(ui: ReactElement, options: RenderOptions = {}) {
  const Inner = options.wrapper
  const wrapper = ({ children }: { children: ReactNode }) => <ShortcutProvider>{Inner ? <Inner>{children}</Inner> : children}</ShortcutProvider>
  return baseRender(ui, { ...options, wrapper })
}
