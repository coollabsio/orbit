import { useState, type ReactNode } from 'react'
import { Setting2 as Settings, TickCircle, User } from 'reicon-react'
import { cn } from 'cn'
import { SettingsCard } from '@/components/common/SettingsCard'
import { CUSTOM_THEME_PRESETS, serializeCustomTheme, setCustomTheme, useCustomTheme } from '@/lib/customTheme'
import { CustomThemeDialog, CustomThemeSwatch } from '@/features/profile/components/CustomThemeDialog'
import { useTheme, type Appearance } from '@/lib/themeContext'

const THEMES: { value: Appearance['theme']; label: string; description: string; preview: string }[] = [
  { value: 'light', label: 'Light', description: 'Bright surfaces and dark text.', preview: 'bg-white' },
  { value: 'system', label: 'System', description: 'Follow your operating system.', preview: 'bg-linear-to-r from-white via-neutral-400 to-[#050505]' },
  { value: 'dark', label: 'Dark', description: 'Dark surfaces and soft contrast.', preview: 'bg-[#181818]' },
]

const NAV_STYLES: { value: Appearance['navStyle']; label: string; description: string }[] = [
  { value: 'default', label: 'Default', description: 'The sidebar and the content run edge to edge.' },
  { value: 'floating', label: 'Floating', description: 'Framed panels float on a dotted backdrop. Only on wide screens.' },
]

const STYLES: { value: Appearance['style']; label: string; description: string }[] = [
  { value: '3d', label: '3D', description: 'Buttons and sidebar icons sit on a raised edge that lifts on hover.' },
  { value: 'flat', label: 'Flat', description: 'No raised edges. Sidebar items are plain rows with a highlight for the current page.' },
]

/** Coolify's Profile → Appearance: how Orbit looks in this browser. */
export function AppearancePage() {
  const { appearance, setAppearance } = useTheme()
  const customTheme = useCustomTheme()
  const [customThemeOpen, setCustomThemeOpen] = useState(false)
  const customThemeName = customTheme
    ? (CUSTOM_THEME_PRESETS.find((preset) => serializeCustomTheme(preset.theme) === serializeCustomTheme(customTheme))?.name ?? 'Your colors')
    : null

  return (
    <>
      <SettingsCard title="Color theme" description="Choose the color scheme used in this browser.">
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
          {THEMES.map((option) => (
            <AppearanceOption
              key={option.value}
              label={option.label}
              description={option.description}
              selected={!customTheme && appearance.theme === option.value}
              onClick={() => {
                setCustomTheme(null)
                setAppearance({ theme: option.value })
              }}
              previewClassName={option.preview}
            >
              <span className="h-8 w-20 rounded-md border border-black/10 bg-neutral-100/80 shadow-sm dark:border-white/10 dark:bg-black/20" />
            </AppearanceOption>
          ))}
          {/* the custom theme opens its editor: it is a theme of its own, and picking one above turns it off */}
          <AppearanceOption
            label={customTheme ? `Custom · ${customThemeName}` : 'Custom'}
            description="Your own colors for the whole app. Pick a preset or your own."
            selected={Boolean(customTheme)}
            aria-haspopup="dialog"
            onClick={() => setCustomThemeOpen(true)}
          >
            <CustomThemeSwatch theme={customTheme} className="h-12 w-24" />
          </AppearanceOption>
        </div>
      </SettingsCard>

      <SettingsCard title="Navigation style" description="Choose how the sidebar and the content sit on the page.">
        <div className="grid gap-3 sm:grid-cols-2">
          {NAV_STYLES.map((option) => {
            const floating = option.value === 'floating'
            return (
              <AppearanceOption
                key={option.value}
                label={option.label}
                description={option.description}
                selected={appearance.navStyle === option.value}
                onClick={() => setAppearance({ navStyle: option.value })}
                previewClassName={cn('items-stretch justify-start', floating && 'gap-1.5 bg-[radial-gradient(circle,rgb(0_0_0/0.13)_1px,transparent_1.3px)] bg-size-[10px_10px] p-1.5 dark:bg-[radial-gradient(circle,rgb(255_255_255/0.09)_1px,transparent_1.3px)]')}
              >
                <span className={cn('flex flex-col items-center gap-1.5 bg-background px-2 py-2', floating ? 'rounded-lg border' : 'border-r')}>
                  {[User, Settings].map((Icon, index) => (
                    <span key={index} className="flex size-6 items-center justify-center rounded-md border bg-muted/50 text-muted-foreground">
                      <Icon className="size-3.5" />
                    </span>
                  ))}
                </span>
                <span className={cn('flex-1 bg-background', floating && 'rounded-lg border')} />
              </AppearanceOption>
            )
          })}
        </div>
      </SettingsCard>

      <SettingsCard title="Style" description="Choose how buttons and sidebar items look.">
        <div className="grid gap-3 sm:grid-cols-2">
          {STYLES.map((option) => {
            const tiles = option.value === '3d'
            return (
              <AppearanceOption
                key={option.value}
                label={option.label}
                description={option.description}
                selected={appearance.style === option.value}
                onClick={() => setAppearance({ style: option.value })}
                previewClassName="flex-col items-start gap-1.5 px-4"
              >
                {[User, Settings].map((Icon, index) => (
                  <span key={index} className={cn('flex items-center gap-2 rounded-md text-muted-foreground', !tiles && 'px-1.5 py-1', !tiles && index === 0 && 'bg-black/5 dark:bg-white/8')}>
                    <span className={cn('flex items-center justify-center', tiles && 'size-6 rounded-md border bg-background shadow-[0_2px_0_rgb(0_0_0/0.22)] dark:shadow-[0_2px_0_rgb(255_255_255/0.08)]')}>
                      <Icon className="size-3.5" />
                    </span>
                    <span className="h-1.5 w-16 rounded-full bg-muted-foreground/25" />
                  </span>
                ))}
              </AppearanceOption>
            )
          })}
        </div>
      </SettingsCard>

      {customThemeOpen ? <CustomThemeDialog open onOpenChange={setCustomThemeOpen} /> : null}
    </>
  )
}

/** One choice: a picture of the look, its name, and a check when it is the current one. */
function AppearanceOption({
  label,
  description,
  selected,
  previewClassName,
  children,
  ...props
}: Omit<React.ComponentProps<'button'>, 'children'> & {
  label: string
  description: string
  /** `undefined` for a card that opens a dialog instead of being chosen. */
  selected?: boolean
  previewClassName?: string
  children: ReactNode
}) {
  return (
    <button
      type="button"
      data-slot="appearance-option"
      aria-pressed={selected}
      className="group/option min-w-0 overflow-hidden rounded-[10px] border border-border bg-card text-left outline-none transition-[border-color,box-shadow] hover:border-foreground/20 hover:shadow-sm focus-visible:ring-3 focus-visible:ring-ring/50 aria-pressed:border-primary aria-pressed:bg-primary/6 aria-pressed:ring-2 aria-pressed:ring-primary/25"
      {...props}
    >
      <span aria-hidden="true" className={cn('flex h-20 items-center justify-center border-b border-border bg-muted/40', previewClassName)}>
        {children}
      </span>
      <span className="block p-3">
        <span className="flex items-center justify-between gap-2">
          <span className="truncate text-sm font-semibold text-foreground">{label}</span>
          <TickCircle className="hidden size-4 shrink-0 text-primary group-aria-pressed/option:block" />
        </span>
        <span className="mt-1 block text-xs leading-5 text-muted-foreground">{description}</span>
      </span>
    </button>
  )
}
