import { useEffect, useRef, useState, type ReactNode } from 'react'
import { toast } from 'sonner'
import { ArrowDown2, CloseCircle, Copy, CopySuccess, Danger, InfoCircle, TickCircle } from 'reicon-react'
import { cn } from 'cn'
import type { MailDnsRecord } from '@orbit/api/generated/types.gen'
import { InfoTip } from '@/components/common/InfoTip'
import { Tip } from '@/components/common/Tip'
import { Button } from '@/components/ui/button'
import { writeClipboard } from '@/lib/clipboard'
import { relativeHost, type SetupStep, type StepState } from './domainSetup'

/** How long a copy button shows its check. */
const COPIED_MS = 1500

/** Copies on demand; `copied` is true for a moment after a copy worked. A failed copy says so in a toast. */
function useCopy() {
  const [copied, setCopied] = useState(false)
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined)
  useEffect(() => () => clearTimeout(timer.current), [])
  function copy(text: string) {
    writeClipboard(text).then(
      () => {
        setCopied(true)
        clearTimeout(timer.current)
        timer.current = setTimeout(() => setCopied(false), COPIED_MS)
      },
      () => toast.error('Could not copy. Select the text and copy it yourself.'),
    )
  }
  return { copied, copy }
}

/** An icon button that copies `text`; its icon turns into a check for a moment. */
export function CopyButton({ text, label }: { text: string; label: string }) {
  const { copied, copy } = useCopy()
  const name = `Copy ${label.toLowerCase()}`
  return (
    <Tip label={copied ? 'Copied' : name}>
      <Button type="button" variant="ghost" size="icon-xs" aria-label={name} onClick={() => copy(text)}>
        {copied ? <CopySuccess className="text-emerald-600 dark:text-emerald-400" /> : <Copy />}
      </Button>
    </Tip>
  )
}

/** A value to copy: the whole field is the button. A long value shows two lines until **Show all**. */
function CopyField({ label, text, children, long = false }: { label: string; text: string; children?: ReactNode; long?: boolean }) {
  const { copied, copy } = useCopy()
  const [expanded, setExpanded] = useState(false)
  const folded = long && !expanded
  return (
    <div className="flex min-w-0 flex-col items-start gap-1">
      <button
        type="button"
        aria-label={`Copy ${label.toLowerCase()}: ${text}`}
        onClick={() => copy(text)}
        className={cn(
          'group/copy flex w-full min-w-0 items-start gap-2 rounded-md border bg-muted/30 px-2 py-1 text-left font-mono text-xs text-foreground transition-colors duration-100 hover:border-foreground/25 hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none active:bg-muted',
          copied && 'border-emerald-600/40 dark:border-emerald-400/40',
        )}
      >
        <span className={cn('min-w-0 flex-1 py-px break-all', folded && 'line-clamp-2')}>{children ?? text}</span>
        <span aria-hidden className={cn('flex shrink-0 items-center gap-1 pt-px font-sans text-[0.6875rem] text-muted-foreground group-hover/copy:text-foreground', copied && 'text-emerald-600 group-hover/copy:text-emerald-600 dark:text-emerald-400')}>
          {copied ? <CopySuccess className="size-3.5" /> : <Copy className="size-3.5" />}
          {copied ? 'Copied' : null}
        </span>
      </button>
      <span role="status" className="sr-only">{copied ? `${label} copied` : ''}</span>
      {long ? (
        <button type="button" className="text-[0.6875rem] text-muted-foreground underline-offset-2 hover:text-foreground hover:underline" onClick={() => setExpanded((value) => !value)}>
          {expanded ? 'Show less' : 'Show all'}
        </button>
      ) : null}
    </div>
  )
}

/** A long TXT value (a DKIM key) folds to two lines. */
const LONG_VALUE = 120

/** What the last check found for a record that is not OK. */
function recordResult(record: MailDnsRecord, checked: boolean): { text: string; tone: 'ok' | 'todo' | 'problem' } {
  switch (record.status.status) {
    case 'ok':
      return { text: 'Found', tone: 'ok' }
    case 'missing':
      return { text: 'Not found yet', tone: 'todo' }
    case 'wrong':
      return { text: `Found another value: ${record.status.found || '(empty)'}`, tone: 'problem' }
    case 'unknown':
      return { text: checked ? 'The DNS did not answer' : 'Not checked yet', tone: 'todo' }
  }
}

/**
 * One record to publish: its type, its host relative to `zone` and its value, each to copy. `own`: the record's own
 * result is shown (the step has several records; with one, the step's summary says it).
 */
function RecordCard({ record, zone, checked, own }: { record: MailDnsRecord; zone: string; checked: boolean; own: boolean }) {
  const host = relativeHost(record.name, zone)
  const suffix = host === '@' ? null : record.name.slice(host.length)
  const result = recordResult(record, checked)
  return (
    <div className="grid grid-cols-[4.5rem_minmax(0,1fr)] items-start gap-x-3 gap-y-2 rounded-lg border bg-background p-3 text-xs">
      <span className="pt-1 text-muted-foreground">Type</span>
      <span className="pt-1 font-mono font-medium">{record.type}{record.priority != null ? <span className="ml-3 font-sans font-normal text-muted-foreground">Priority <span className="font-mono text-foreground">{record.priority}</span></span> : null}</span>
      <span className="flex items-center gap-1 pt-1 text-muted-foreground">
        Host
        <InfoTip size={12} text={`Most DNS panels add ${zone} themselves: enter only this part.${host === '@' ? ' @ means the domain itself; some panels want it empty.' : ''} If your panel wants the full name, enter ${record.name}.`} />
      </span>
      <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
        <div className="min-w-0 max-w-full"><CopyField label="Host" text={host} /></div>
        {suffix ? <span className="font-mono text-[0.6875rem] break-all text-muted-foreground/70" title={record.name}>{suffix}</span> : null}
      </div>
      <span className="pt-1 text-muted-foreground">Value</span>
      <CopyField label="Value" text={record.value} long={record.value.length > LONG_VALUE} />
      {record.status.status === 'wrong' || (own && record.status.status !== 'ok') ? (
        <>
          <span aria-hidden />
          <span className={cn('break-all', result.tone === 'problem' ? 'text-destructive' : 'text-muted-foreground')}>{result.text}</span>
        </>
      ) : null}
    </div>
  )
}

const STATE_LABEL: Record<StepState, string> = { done: 'Done', todo: 'To do', problem: 'Problem', unknown: 'Not known yet', blocked: 'Blocked' }

/** The step's state as a circle: a check when done, a cross for a wrong value, else its number. */
function StepIcon({ state, index }: { state: StepState; index: number }) {
  if (state === 'done') return <TickCircle role="img" aria-label={STATE_LABEL[state]} weight="Filled" className="size-5 shrink-0 text-emerald-600 dark:text-emerald-400" />
  if (state === 'problem') return <CloseCircle role="img" aria-label={STATE_LABEL[state]} weight="Filled" className="size-5 shrink-0 text-destructive" />
  if (state === 'blocked') return <Danger role="img" aria-label={STATE_LABEL[state]} className="size-5 shrink-0 text-amber-600 dark:text-amber-400" />
  return (
    <span role="img" aria-label={STATE_LABEL[state]} className="flex size-5 shrink-0 items-center justify-center rounded-full border border-muted-foreground/50 text-[0.625rem] font-semibold text-muted-foreground tabular-nums">
      {index + 1}
    </span>
  )
}

/**
 * One setup step: a header that opens and closes it, then its records. `extra` goes above the records (the route
 * select, a note); `blocked` replaces them when Orbit cannot say what to publish.
 */
export function SetupStepRow({
  step,
  index,
  open,
  onToggle,
  zone,
  checked,
  extra,
  blocked,
}: {
  step: SetupStep
  index: number
  open: boolean
  onToggle: () => void
  zone: string
  checked: boolean
  extra?: ReactNode
  blocked?: ReactNode
}) {
  const bodyId = `mail-step-${step.key}-${zone}`
  const tone = step.state === 'done' ? 'text-emerald-600 dark:text-emerald-400' : step.state === 'problem' ? 'text-destructive' : step.state === 'blocked' ? 'text-amber-600 dark:text-amber-400' : 'text-muted-foreground'
  return (
    <li className="flex flex-col">
      <div className="relative flex min-h-11 items-center gap-2.5 px-3 hover:bg-muted/40">
        <StepIcon state={step.state} index={index} />
        <span className="text-[0.8125rem] font-medium text-foreground">{step.title}</span>
        {/* above the button, which covers the whole row */}
        <span className="relative z-10 flex"><InfoTip text={step.help} /></span>
        {step.optional ? <span className="rounded-sm bg-muted px-1.5 py-px text-[0.625rem] font-medium text-muted-foreground">Recommended</span> : null}
        <span className={cn('ml-auto text-xs', tone)}>{step.summary}</span>
        <button
          type="button"
          aria-expanded={open}
          aria-controls={bodyId}
          aria-label={`${step.title}: ${step.summary}`}
          onClick={onToggle}
          className="flex size-6 items-center justify-center rounded-md text-muted-foreground after:absolute after:inset-0 after:content-[''] focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none"
        >
          <ArrowDown2 aria-hidden className={cn('size-3.5 transition-transform duration-150 ease-out', open && 'rotate-180')} />
        </button>
      </div>
      {open ? (
        <div id={bodyId} className="flex flex-col gap-2 px-3 pt-1 pb-3 pl-[2.625rem]">
          {extra}
          {blocked ?? step.records.map((record) => <RecordCard key={`${record.kind}:${record.name}:${record.value}`} record={record} zone={zone} checked={checked} own={step.records.length > 1} />)}
        </div>
      ) : null}
    </li>
  )
}

/** A muted note in a step (what else the route needs). */
export function StepNote({ children }: { children: ReactNode }) {
  return (
    <p className="m-0 flex items-start gap-1.5 text-xs text-muted-foreground">
      <InfoCircle aria-hidden className="mt-px size-3.5 shrink-0" />
      <span>{children}</span>
    </p>
  )
}
