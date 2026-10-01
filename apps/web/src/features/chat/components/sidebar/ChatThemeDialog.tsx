import { useId, useState } from 'react'
import { toast } from 'sonner'
import { Copy, Hashtag } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldDescription, FieldError, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group'
import {
  CHAT_THEME_PRESETS,
  chatThemeVariables,
  normalizeHex,
  parseChatTheme,
  serializeChatTheme,
  setChatTheme,
  useChatTheme,
  type ChatTheme,
} from '@/features/chat/lib/chatTheme'

/** The colours the fields start from when the user edits the Orbit default. */
const CUSTOM_BASE: ChatTheme = { sidebar: '#1f1f23', selected: '#34343b', accent: '#f2458f' }

const COLOR_FIELDS: readonly { key: keyof ChatTheme; label: string }[] = [
  { key: 'sidebar', label: 'Sidebar' },
  { key: 'selected', label: 'Selected conversation' },
  { key: 'accent', label: 'Unread and badges' },
]

/** The chat theme editor: presets, three colours, a live preview, and a code to share. Saved for this browser. */
export function ChatThemeDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>Chat theme</DialogTitle>
          <DialogDescription>Colours of your chat sidebar. Only you see them, and messages stay as they are.</DialogDescription>
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
      <div className="grid gap-5 sm:grid-cols-[minmax(0,1fr)_200px]">
        <div className="grid content-start gap-4">
          <div role="group" aria-label="Presets" className="flex flex-wrap gap-1.5">
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

          <div className="grid gap-2.5">
            {COLOR_FIELDS.map((field) => (
              <ColorField
                key={`${field.key}-${revision}`}
                label={field.label}
                value={colors[field.key]}
                onChange={(color) => {
                  setTheme({ ...colors, [field.key]: color })
                  setCodeError(false)
                }}
              />
            ))}
          </div>
        </div>

        <ThemePreview theme={theme} />
      </div>

      <Field data-invalid={codeError || undefined}>
        <FieldLabel htmlFor={codeId}>Theme code</FieldLabel>
        <InputGroup>
          <InputGroupInput
            key={revision}
            id={codeId}
            defaultValue={serializeChatTheme(colors)}
            spellCheck={false}
            autoComplete="off"
            aria-invalid={codeError || undefined}
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
              aria-label="Copy theme code"
              onClick={() => {
                void navigator.clipboard.writeText(serializeChatTheme(colors)).then(
                  () => toast.success('Theme code copied'),
                  () => toast.error('Could not copy the theme code.'),
                )
              }}
            >
              <Copy />
            </InputGroupButton>
          </InputGroupAddon>
        </InputGroup>
        {codeError ? (
          <FieldError>A theme code is three hex colours, for example #3f0e40,#1164a3,#ecb22e.</FieldError>
        ) : (
          <FieldDescription>Paste a code to use a theme that someone shared, or copy this one to share yours.</FieldDescription>
        )}
      </Field>

      <DialogFooter>
        <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
        <Button type="submit" disabled={codeError}>
          Save theme
        </Button>
      </DialogFooter>
    </form>
  )
}

/** A preset as a small three-colour chip and its name. "Orbit" (`theme` null) is the default look. */
function PresetButton({ name, theme, pressed, onSelect }: { name: string; theme: ChatTheme | null; pressed: boolean; onSelect: () => void }) {
  return (
    <Button type="button" variant="outline" size="sm" aria-pressed={pressed} className="gap-1.5 px-2 aria-pressed:border-primary aria-pressed:ring-2 aria-pressed:ring-primary/30" onClick={onSelect}>
      <span aria-hidden="true" className="flex size-3.5 overflow-hidden rounded-[4px] border border-border bg-sidebar">
        {theme ? (
          <>
            <span className="flex-1" style={{ backgroundColor: theme.sidebar }} />
            <span className="flex-1" style={{ backgroundColor: theme.selected }} />
            <span className="flex-1" style={{ backgroundColor: theme.accent }} />
          </>
        ) : (
          <span className="m-auto size-1.5 bg-primary" />
        )}
      </span>
      {name}
    </Button>
  )
}

/** One colour: the system colour picker on the swatch, and a hex field for exact values. */
function ColorField({ label, value, onChange }: { label: string; value: string; onChange: (color: string) => void }) {
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
      <label htmlFor={id} className="min-w-0 flex-1 truncate text-sm">
        {label}
      </label>
      <Input
        id={id}
        value={text}
        spellCheck={false}
        autoComplete="off"
        maxLength={7}
        aria-invalid={invalid || undefined}
        className="w-24 font-mono text-xs"
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

/** A small copy of the sidebar with the theme on it: every state the three colours touch. */
function ThemePreview({ theme }: { theme: ChatTheme | null }) {
  return (
    <div
      data-slot="chat-theme-preview"
      aria-hidden="true"
      className="flex flex-col gap-px self-start rounded-lg border border-border bg-background p-2 text-[13px] text-foreground/80 dark:[--background:var(--card)]"
      style={chatThemeVariables(theme)}
    >
      <span className="px-2 pt-0.5 pb-1.5 text-xs font-semibold text-foreground">Chat</span>
      <PreviewRow name="announcements" unread />
      <PreviewRow name="general" selected />
      <PreviewRow name="design" count={3} />
      <PreviewRow name="support" />
      <PreviewRow name="random" />
    </div>
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
