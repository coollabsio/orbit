/** The web's `MailDomainRecords.tsx`: a domain's setup steps, each with its DNS records to copy. */
import * as Clipboard from 'expo-clipboard'
import { type ReactNode, useEffect, useRef, useState } from 'react'
import { Pressable, View } from 'react-native'

import type { MailDnsRecord } from '@orbit/api/generated/types.gen'
import { relativeHost, type SetupStep, type StepState } from '../../../data/mail-domain-setup'
import { presenceColors, useTheme } from '../../../theme'
import { Icon, LabelTip, Text } from '../../../ui'

/** How long a copy button shows its check. */
const COPIED_MS = 1500

/** The web's success green, by scheme (`dark:` does not override a conflicting class here). */
export const useOkText = () => (useTheme().scheme === 'dark' ? 'text-emerald-400' : 'text-emerald-600')

/** Copies on demand; `copied` is true for a moment after a copy worked, `failed` after one did not. */
export function useCopy() {
  const [state, setState] = useState<'idle' | 'copied' | 'failed'>('idle')
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined)
  useEffect(() => () => clearTimeout(timer.current), [])
  function copy(text: string) {
    const show = (next: 'copied' | 'failed') => {
      setState(next)
      clearTimeout(timer.current)
      timer.current = setTimeout(() => setState('idle'), COPIED_MS)
    }
    Clipboard.setStringAsync(text).then((done) => show(done ? 'copied' : 'failed'), () => show('failed'))
  }
  return { copied: state === 'copied', failed: state === 'failed', copy }
}

/** An icon button that copies `text`; its icon turns into a check for a moment. */
export function CopyButton({ text, label }: { text: string; label: string }) {
  const { colors } = useTheme()
  const { copied, failed, copy } = useCopy()
  return (
    <Pressable accessibilityRole="button" accessibilityLabel={copied ? 'Copied' : `Copy ${label.toLowerCase()}`} hitSlop={8} onPress={() => copy(text)} className="size-7 items-center justify-center rounded-md active:bg-muted">
      <Icon name={copied ? 'copySuccess' : 'copy'} size={15} color={copied ? presenceColors.online : failed ? colors.destructive : colors.mutedForeground} />
    </Pressable>
  )
}

/** A value to copy: the whole field is the button. A long value shows two lines until **Show all**. */
function CopyField({ label, text, long = false }: { label: string; text: string; long?: boolean }) {
  const { colors } = useTheme()
  const okText = useOkText()
  const { copied, failed, copy } = useCopy()
  const [expanded, setExpanded] = useState(false)
  return (
    <View className="min-w-0 items-start gap-1">
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={`Copy ${label.toLowerCase()}: ${text}`}
        onPress={() => copy(text)}
        className={`w-full min-w-0 flex-row items-start gap-2 rounded-md border bg-muted/30 px-2 py-1 active:bg-muted ${copied ? 'border-emerald-600/40' : 'border-border'}`}
      >
        <Text numberOfLines={long && !expanded ? 2 : undefined} className="min-w-0 flex-1 font-mono text-[12px] leading-[17px] text-foreground">
          {text}
        </Text>
        <View className="flex-row items-center gap-1 pt-px">
          <Icon name={copied ? 'copySuccess' : 'copy'} size={14} color={copied ? presenceColors.online : failed ? colors.destructive : colors.mutedForeground} />
          {copied ? <Text className={`text-[11px] ${okText}`}>Copied</Text> : failed ? <Text className="text-[11px] text-destructive">Not copied</Text> : null}
        </View>
      </Pressable>
      {long ? (
        <Text accessibilityRole="button" onPress={() => setExpanded((value) => !value)} className="text-[11px] text-muted-foreground underline">
          {expanded ? 'Show less' : 'Show all'}
        </Text>
      ) : null}
    </View>
  )
}

/** A long TXT value (a DKIM key) folds to two lines. */
const LONG_VALUE = 120

/** What the last check found for a record. */
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
 * result is shown, found or not (the step has several records; with one, the step's summary says it).
 */
function RecordCard({ record, zone, checked, own }: { record: MailDnsRecord; zone: string; checked: boolean; own: boolean }) {
  const okText = useOkText()
  const host = relativeHost(record.name, zone)
  const suffix = host === '@' ? null : record.name.slice(host.length)
  const result = recordResult(record, checked)
  return (
    <View className="gap-2 rounded-lg border border-border bg-background p-3">
      <View className="flex-row items-center gap-3">
        <Text className="w-[72px] text-[12px] text-muted-foreground">Type</Text>
        <Text className="font-mono text-[12px] text-foreground">{record.type}</Text>
        {record.priority != null ? (
          <Text className="text-[12px] text-muted-foreground">
            Priority <Text className="font-mono text-[12px] text-foreground">{record.priority}</Text>
          </Text>
        ) : null}
      </View>
      <View className="flex-row items-start gap-3">
        <View className="w-[72px] pt-1">
          <LabelTip
            label="Host"
            className="text-[12px] text-muted-foreground"
            text={`Most DNS panels add ${zone} themselves: enter only this part.${host === '@' ? ' @ means the domain itself; some panels want it empty.' : ''} If your panel wants the full name, enter ${record.name}.`}
          />
        </View>
        <View className="min-w-0 flex-1 gap-1">
          <CopyField label="Host" text={host} />
          {suffix ? <Text className="font-mono text-[11px] text-muted-foreground/70">{suffix}</Text> : null}
        </View>
      </View>
      <View className="flex-row items-start gap-3">
        <Text className="w-[72px] pt-1 text-[12px] text-muted-foreground">Value</Text>
        <View className="min-w-0 flex-1">
          <CopyField label="Value" text={record.value} long={record.value.length > LONG_VALUE} />
        </View>
      </View>
      {record.status.status === 'wrong' || own ? (
        <Text className={`pl-[84px] text-[12px] ${result.tone === 'problem' ? 'text-destructive' : result.tone === 'ok' ? okText : 'text-muted-foreground'}`}>{result.text}</Text>
      ) : null}
    </View>
  )
}

const STATE_LABEL: Record<StepState, string> = { done: 'Done', todo: 'To do', problem: 'Problem', unknown: 'Not known yet', blocked: 'Blocked' }

/** The step's state as a circle: a check when done, a cross for a wrong value, else its number. */
function StepIcon({ state, index }: { state: StepState; index: number }) {
  const { colors } = useTheme()
  if (state === 'done') return <Icon name="checkCircle" size={20} color={presenceColors.online} />
  if (state === 'problem') return <Icon name="closeCircle" size={20} color={colors.destructive} />
  if (state === 'blocked') return <Icon name="danger" size={20} color={presenceColors.idle} />
  return (
    <View accessibilityLabel={STATE_LABEL[state]} className="size-5 items-center justify-center rounded-full border border-muted-foreground/50">
      <Text className="font-noto-semibold text-[10px] text-muted-foreground">{index + 1}</Text>
    </View>
  )
}

/**
 * One setup step: a header that opens and closes it, then its records. `extra` goes above the records (the route
 * select, a note); `blocked` replaces them when Orbit cannot say what to publish.
 */
export function SetupStepRow({ step, index, open, onToggle, zone, checked, extra, blocked, first }: { step: SetupStep; index: number; open: boolean; onToggle: () => void; zone: string; checked: boolean; extra?: ReactNode; blocked?: ReactNode; first: boolean }) {
  const { colors, scheme } = useTheme()
  const okText = useOkText()
  const tone = step.state === 'done' ? okText : step.state === 'problem' ? 'text-destructive' : step.state === 'blocked' ? (scheme === 'dark' ? 'text-amber-400' : 'text-amber-600') : 'text-muted-foreground'
  return (
    <View className={first ? '' : 'border-t border-border'}>
      <Pressable
        accessibilityRole="button"
        accessibilityState={{ expanded: open }}
        accessibilityLabel={`${step.title}: ${step.summary}`}
        onPress={onToggle}
        className="min-h-11 flex-row flex-wrap items-center gap-x-2.5 gap-y-1 px-3 py-2 active:bg-muted/40"
      >
        <StepIcon state={step.state} index={index} />
        <View className="min-w-0 shrink">
          <LabelTip label={step.title} text={step.help} className="font-noto-medium text-[13px] text-foreground" />
        </View>
        {step.optional ? (
          <View className="rounded-sm bg-muted px-1.5 py-px">
            <Text className="font-noto-medium text-[10px] text-muted-foreground">Recommended</Text>
          </View>
        ) : null}
        <View className="ml-auto flex-row items-center gap-1.5">
          <Text className={`text-[12px] ${tone}`}>{step.summary}</Text>
          <Icon name={open ? 'chevronUp' : 'chevronDown'} size={14} color={colors.mutedForeground} />
        </View>
      </Pressable>
      {open ? (
        <View className="gap-2 px-3 pb-3 pt-1">
          {extra}
          {blocked ?? step.records.map((record) => <RecordCard key={`${record.kind}:${record.name}:${record.value}`} record={record} zone={zone} checked={checked} own={step.records.length > 1} />)}
        </View>
      ) : null}
    </View>
  )
}

/** A muted note in a step (what else the route needs). */
export function StepNote({ children }: { children: ReactNode }) {
  const { colors } = useTheme()
  return (
    <View className="flex-row items-start gap-1.5">
      <View className="pt-0.5">
        <Icon name="infoCircle" size={14} color={colors.mutedForeground} />
      </View>
      <Text className="min-w-0 flex-1 text-[12px] leading-[17px] text-muted-foreground">{children}</Text>
    </View>
  )
}
