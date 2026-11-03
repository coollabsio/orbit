import { afterEach, expect, test } from 'bun:test'
import { render } from '@testing-library/react'
import { useState, type ReactNode } from 'react'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { setCustomTheme } from '@/lib/customTheme'
import { ThemeContext } from '@/lib/themeContext'
import { InAppSidebar } from './InAppSidebar'
import { SidebarSlotContext } from './sidebarSlot'

afterEach(() => setCustomTheme(null))

function RowMenu() {
  return (
    <DropdownMenu open>
      <DropdownMenuTrigger>Row options</DropdownMenuTrigger>
      <DropdownMenuContent>
        <DropdownMenuItem>Duplicate</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function Sidebar({ hosted, children }: { hosted: boolean; children: ReactNode }) {
  const [slot, setSlot] = useState<HTMLElement | null>(null)
  // the app shell gives the sidebar a slot only in the new UI
  const newUi = { theme: 'dark', appearance: { theme: 'dark', style: '3d', navStyle: 'default', layout: 'new' }, setAppearance: () => {}, toggleTheme: () => {} } as const
  return (
    <ThemeContext value={newUi}>
      <SidebarSlotContext value={hosted ? slot : null}>
        <aside ref={setSlot} />
        <InAppSidebar>{children}</InAppSidebar>
      </SidebarSlotContext>
    </ThemeContext>
  )
}

const panel = (view: ReturnType<typeof render>) => view.baseElement.querySelector('[data-slot=dropdown-menu-content]') as HTMLElement

test('a menu from a list in the app sidebar looks like the sidebar menus and carries the custom theme', () => {
  setCustomTheme({ sidebar: '#3f0e40', selected: '#1164a3', accent: '#ecb22e', chat: '#ffffff' })
  const view = render(<Sidebar hosted><RowMenu /></Sidebar>)
  expect(panel(view).className).toContain('bg-sidebar-popover')
  const wrapper = panel(view).parentElement as HTMLElement
  expect(wrapper.classList.contains('dark')).toBe(true)
  expect(wrapper.style.getPropertyValue('--sidebar')).toBe('#3f0e40')
})

test('the same menu beside the content keeps the plain popover look', () => {
  setCustomTheme({ sidebar: '#3f0e40', selected: '#1164a3', accent: '#ecb22e', chat: '#ffffff' })
  const view = render(<Sidebar hosted={false}><RowMenu /></Sidebar>)
  expect(panel(view).className).not.toContain('bg-sidebar-popover')
  expect((panel(view).parentElement as HTMLElement).style.getPropertyValue('--sidebar')).toBe('')
})
