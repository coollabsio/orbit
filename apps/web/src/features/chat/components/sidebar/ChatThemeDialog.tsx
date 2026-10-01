import { useId, useState, type ReactNode } from 'react'
import { toast } from 'sonner'
import { Copy, Hashtag } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldDescription, FieldError, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group'
import {
  CHAT_THEME_PRESETS,
  chatSidebarVariables,
  chatThemeVariables,
  normalizeHex,
  readableOn,
  parseChatTheme,
  serializeChatTheme,
  setChatTheme,
  useChatTheme,
  type ChatTheme,
} from '@/features/chat/lib/chatTheme'

/** The colours the fields start from when the user edits the Orbit default. */
const CUSTOM_BASE: ChatTheme = { sidebar: '#1f1f23', selected: '#34343b', accent: '#f2458f', chat: '#26262b' }

const COLOR_FIELDS: readonly { key: keyof ChatTheme; label: string; hint: string }[] = [
  { key: 'sidebar', label: 'Sidebar', hint: 'The conversation list' },
  { key: 'selected', label: 'Selected', hint: 'The open conversation' },
  { key: 'accent', label: 'Accent', hint: 'Unread, badges, mentions' },
  { key: 'chat', label: 'Messages', hint: 'Messages, header, panes' },
]

/** The chat theme editor: presets, four colours, a live preview, and a code to share. Saved for this browser. */
export function ChatThemeDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>Chat theme</DialogTitle>
          <DialogDescription>Colours of your chat. Only you see them, and text stays readable on every colour.</DialogDescription>
        </DialogHeader>
        <ChatThemeForm onDone={() => onOpenChange(false)} />
      </DialogContent>
    </Dialog>
  )
}

function ChatThemeForm({ onDone }: { onDone: () => void }) {
  const saved = useChatTheme()
  const [theme, setTheme] = useState<ChatTheme | null>(saved)
  // Changes when the colours come from a preset or a pasted code, so the hex fields take the new values.
  const [revision, setRevision] = useState(0)
  const [codeError, setCodeError] = useState(false)
  const codeId = useId()
  const presetsId = useId()
  const colorsId = useId()
  const colors = theme ?? CUSTOM_BASE

  const replace = (next: ChatTheme | null) => {
    setTheme(next)
    setRevision((current) => current + 1)
    setCodeError(false)
  }

  return (
    <form
      className="grid gap-5"
      onSubmit={(event) => {
        event.preventDefault()
        setChatTheme(theme)
        onDone()
      }}
    >
      <div className="grid gap-6 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
        <section aria-labelledby={presetsId} className="grid content-start gap-2">
          <h3 id={presetsId} className="text-xs font-medium text-muted-foreground">
            Presets
          </h3>
          <div role="group" aria-labelledby={presetsId} className="grid grid-cols-3 gap-x-2 gap-y-2.5">
            <PresetButton name="Orbit" theme={null} pressed={theme === null} onSelect={() => replace(null)} />
            {CHAT_THEME_PRESETS.map((preset) => (
              <PresetButton
                key={preset.name}
                name={preset.name}
                theme={preset.theme}
                pressed={theme !== null && serializeChatTheme(theme) === serializeChatTheme(preset.theme)}
                onSelect={() => replace(preset.theme)}
              />
            ))}
          </div>
        </section>

        <div className="grid content-start gap-4">
          <ThemePreview theme={theme} />

          <section aria-labelledby={colorsId} className="grid gap-2">
            <h3 id={colorsId} className="text-xs font-medium text-muted-foreground">
              Colours
            </h3>
            <div className="grid gap-1.5">
              {COLOR_FIELDS.map((field) => (
                <ColorField
                  key={`${field.key}-${revision}`}
                  label={field.label}
                  hint={field.hint}
                  value={colors[field.key]}
                  onChange={(color) => {
                    setTheme({ ...colors, [field.key]: color })
                    setCodeError(false)
                  }}
                />
              ))}
            </div>
          </section>

          <Field data-invalid={codeError || undefined} className="gap-1.5">
            <FieldLabel htmlFor={codeId} className="text-xs font-medium text-muted-foreground">
              Share code
            </FieldLabel>
            <InputGroup>
              <InputGroupInput
                key={revision}
                id={codeId}
                defaultValue={serializeChatTheme(colors)}
                spellCheck={false}
                autoComplete="off"
                aria-invalid={codeError || undefined}
                aria-describedby={`${codeId}-hint`}
                className="font-mono text-xs"
                onChange={(event) => {
                  const parsed = parseChatTheme(event.target.value)
                  setCodeError(!parsed)
                  if (parsed) setTheme(parsed)
                }}
              />
              <InputGroupAddon align="inline-end">
                <InputGroupButton
                  size="icon-xs"
                  aria-label="Copy share code"
                  onClick={() => {
                    void navigator.clipboard.writeText(serializeChatTheme(colors)).then(
                      () => toast.success('Share code copied'),
                      () => toast.error('Could not copy the share code.'),
                    )
                  }}
                >
                  <Copy />
                </InputGroupButton>
              </InputGroupAddon>
            </InputGroup>
            {codeError ? (
              <FieldError id={`${codeId}-hint`}>A code is four hex colours, for example #3f0e40,#1164a3,#ecb22e,#ffffff.</FieldError>
            ) : (
              <FieldDescription id={`${codeId}-hint`} className="text-xs">
                Paste a code that someone shared, or copy yours.
              </FieldDescription>
            )}
          </Field>
        </div>
      </div>

      <DialogFooter>
        <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
        <Button type="submit" disabled={codeError}>
          Save theme
        </Button>
      </DialogFooter>
    </form>
  )
}

/** The app's own colours, for the "Orbit" preset's thumbnail. */
const APP_COLORS = { sidebar: 'var(--background)', selected: 'var(--muted)', accent: 'var(--primary)', chat: 'var(--card)' }

/** A preset as a thumbnail of chat in its colours, with its name below. "Orbit" (`theme` null) is the default look. */
function PresetButton({ name, theme, pressed, onSelect }: { name: string; theme: ChatTheme | null; pressed: boolean; onSelect: () => void }) {
  const colors = theme ?? APP_COLORS
  const sidebarText = theme ? readableOn(theme.sidebar) : 'var(--foreground)'
  const chatText = theme ? readableOn(theme.chat) : 'var(--foreground)'
  return (
    <button
      type="button"
      data-slot="theme-preset"
      aria-pressed={pressed}
      className="group/preset flex min-w-0 flex-col gap-1 rounded-lg text-left outline-none"
      onClick={onSelect}
    >
      <span
        aria-hidden="true"
        className="flex h-12 w-full overflow-hidden rounded-md border border-border transition-transform duration-150 ease-out group-focus-visible/preset:ring-3 group-focus-visible/preset:ring-ring/50 group-active/preset:scale-[0.97] group-aria-pressed/preset:border-primary group-aria-pressed/preset:ring-2 group-aria-pressed/preset:ring-primary/40 motion-reduce:transition-none"
      >
        <span className="flex w-[38%] flex-col gap-[3px] p-1.5" style={{ backgroundColor: colors.sidebar, color: sidebarText }}>
          <span className="h-[3px] w-3/4 rounded-full bg-current opacity-35" />
          <span className="-mx-0.5 flex h-[7px] items-center rounded-[2px] px-0.5" style={{ backgroundColor: colors.selected }} />
          <span className="flex items-center justify-between">
            <span className="h-[3px] w-1/2 rounded-full bg-current opacity-35" />
            <span className="size-[4px]" style={{ backgroundColor: colors.accent }} />
          </span>
        </span>
        <span className="flex flex-1 flex-col gap-[3px] p-1.5" style={{ backgroundColor: colors.chat, color: chatText }}>
          <span className="h-[3px] w-4/5 rounded-full bg-current opacity-45" />
          <span className="h-[3px] w-3/5 rounded-full bg-current opacity-25" />
          <span className="mt-auto h-[7px] rounded-[2px] border border-current/25" />
        </span>
      </span>
      <span className="truncate px-0.5 text-xs text-muted-foreground group-aria-pressed/preset:font-medium group-aria-pressed/preset:text-foreground">{name}</span>
    </button>
  )
}

/** One colour: the system colour picker on the swatch, what the colour is for, and a hex field for exact values. */
function ColorField({ label, hint, value, onChange }: { label: string; hint: string; value: string; onChange: (color: string) => void }) {
  const [text, setText] = useState(value)
  const id = useId()
  const invalid = normalizeHex(text) === null

  return (
    <div data-slot="color-field" className="flex items-center gap-2.5">
      <label
        className="relative size-8 shrink-0 cursor-pointer overflow-hidden rounded-md border border-input has-focus-visible:ring-3 has-focus-visible:ring-ring/50"
        style={{ backgroundColor: value }}
      >
        <span className="sr-only">Pick the colour: {label}</span>
        <input
          type="color"
          value={value}
          className="absolute inset-0 size-full cursor-pointer opacity-0"
          onChange={(event) => {
            setText(event.target.value)
            onChange(event.target.value)
          }}
        />
      </label>
      <label htmlFor={id} className="flex min-w-0 flex-1 flex-col leading-tight">
        <span className="truncate text-sm">{label}</span>
        <span className="truncate text-xs text-muted-foreground">{hint}</span>
      </label>
      <Input
        id={id}
        value={text}
        spellCheck={false}
        autoComplete="off"
        maxLength={7}
        aria-invalid={invalid || undefined}
        className="w-[88px] font-mono text-xs"
        onChange={(event) => {
          setText(event.target.value)
          const color = normalizeHex(event.target.value)
          if (color) onChange(color)
        }}
        onBlur={() => setText(value)}
      />
    </div>
  )
}

/** A small copy of chat with the theme on it: every state the four colours touch. */
function ThemePreview({ theme }: { theme: ChatTheme | null }) {
  return (
    <div
      data-slot="chat-theme-preview"
      aria-hidden="true"
      className="flex h-[188px] overflow-hidden rounded-lg border border-border bg-background text-[13px] text-foreground dark:[--background:var(--card)]"
      style={chatThemeVariables(theme)}
    >
      <div className="flex w-[132px] shrink-0 flex-col gap-px border-r border-border bg-background p-1.5 text-foreground/80" style={chatSidebarVariables(theme)}>
        <span className="px-2 pt-1 pb-2 text-xs font-semibold text-foreground">Chat</span>
        <PreviewRow name="news" unread />
        <PreviewRow name="general" selected />
        <PreviewRow name="design" count={3} />
        <PreviewRow name="support" />
      </div>
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="flex h-8 shrink-0 items-center gap-1 border-b border-border px-2.5 text-xs font-semibold">
          <Hashtag className="size-3.5 text-muted-foreground" />
          general
        </span>
        <div className="flex min-h-0 flex-1 flex-col gap-1.5 p-2.5">
          <PreviewMessage name="Maya">The build is green again.</PreviewMessage>
          <span className="flex items-center gap-1 text-[10px] font-semibold text-primary">
            <span className="size-1.5 bg-primary" />
            New
            <span className="h-px flex-1 bg-primary" />
          </span>
          <PreviewMessage name="Tom">
            Thanks <span className="rounded-sm bg-primary/10 px-0.5 font-medium text-primary dark:bg-primary/20">@Maya</span>
          </PreviewMessage>
          <span className="mt-auto flex h-7 shrink-0 items-center justify-between rounded-md border border-input px-2 text-xs text-muted-foreground dark:bg-input/30">
            Message
            <span className="size-3 rounded-[3px] bg-primary" />
          </span>
        </div>
      </div>
    </div>
  )
}

function PreviewMessage({ name, children }: { name: string; children: ReactNode }) {
  return (
    <span className="flex gap-1.5">
      <span className="mt-0.5 size-4 shrink-0 rounded-full bg-muted" />
      <span className="min-w-0 text-xs leading-snug">
        <span className="block font-medium">{name}</span>
        <span className="text-foreground/85">{children}</span>
      </span>
    </span>
  )
}

function PreviewRow({ name, selected, unread, count }: { name: string; selected?: boolean; unread?: boolean; count?: number }) {
  return (
    <span
      data-active={selected || undefined}
      className="flex h-7 items-center gap-2 rounded-md px-2 data-active:bg-[var(--chat-selected,var(--muted))] data-active:text-[var(--chat-selected-foreground,var(--foreground))]"
    >
      <Hashtag className="size-3.5 shrink-0 opacity-70" />
      <span className={unread || count ? 'min-w-0 flex-1 truncate font-semibold text-foreground' : 'min-w-0 flex-1 truncate'}>{name}</span>
      {count ? (
        <span className="inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-primary px-1 text-[10px] leading-none font-semibold text-primary-foreground">{count}</span>
      ) : unread ? (
        <span className="size-1.5 bg-primary" />
      ) : null}
    </span>
  )
}
