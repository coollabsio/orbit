import { type Href, router } from 'expo-router'
import { type ReactNode, useEffect, useState } from 'react'
import { Alert, View } from 'react-native'

import { type ImportKind, type ImportProgressView, type ImportRecord, type ImportTree, importErrorMessage, importHeadline, isFinalImport, isImportMissing } from '../../data/import-api'
import { useCancelImport, useImport, useImportTree } from '../../hooks/imports'
import { useTheme } from '../../theme'
import { Button, EmptyState, Icon, Spinner, Text } from '../../ui'
import { CancelDialog, ImportBody, ImportScreen, Loading, ProgressBar, SectionLabel, Stat } from './Parts'

/** What differs between the sources in the shared steps (scanning, importing, the result frame). */
export interface FlowSource<K extends ImportKind> {
  kind: K
  /** "Import from Notion". */
  title: string
  /** The header of the ready step: "Choose pages". */
  chooseTitle: string
  /** The connect step's route; "Import more" and "New import" go there. */
  home: Href & string
  /** The account or file the import reads, beside the title. */
  detail: (item: ImportRecord<K>) => string | null
  scanning: (item: ImportRecord<K>) => { headline: string; text: string }
  /** What cancelling does before the import starts and once it runs. */
  cancel: { before: string; started: string }
  /**
   * The importing step: its headline for the status and total, the counted unit, the closing note. A source whose
   * import reports stages gives `progress` (it replaces the headline and the count), and `notice` for a wait.
   */
  importing: {
    headline: (item: ImportRecord<K>) => string
    unit: string
    note: string
    progress?: (item: ImportRecord<K>) => ImportProgressView
    notice?: { until: (item: ImportRecord<K>) => string | null; text: (until: string | null, now: number) => string | null }
  }
  /** The chooser's fallback when the scan has no tree. */
  treeMissing: string
}

/**
 * One import through its states: the connect step without an id; with one, scanning, the chooser, the running
 * import and the result, as the web's import panes.
 */
export function ImportFlow<K extends ImportKind>({ source, importId, connect, chooser, result }: { source: FlowSource<K>; importId: string | undefined; connect: ReactNode; chooser: (item: ImportRecord<K>, tree: ImportTree<K>) => ReactNode; result: (item: ImportRecord<K>) => ReactNode }) {
  const query = useImport(source.kind, importId)
  if (!importId) return connect
  const item = query.data
  if (item) {
    if (item.status === 'scanning') return <ScanningStep source={source} item={item} />
    if (item.status === 'ready') return <ChooseStep key={item.id} source={source} item={item} chooser={chooser} />
    if (item.status === 'queued' || item.status === 'importing') return <ImportingStep source={source} item={item} />
    if (isFinalImport(item.status)) return result(item)
  }
  if (query.isError) {
    const missing = isImportMissing(query.error)
    return (
      <ImportScreen title={source.title}>
        <View className="flex-1 p-6">
          <EmptyState
            icon="import"
            title={missing ? 'Import not found' : 'Import unavailable'}
            description={missing ? 'This import does not exist or belongs to someone else.' : 'The server could not load this import.'}
            action={
              <View className="flex-row gap-2">
                {missing ? null : <Button label="Retry" variant="outline" onPress={() => void query.refetch()} />}
                <Button label="New import" variant="outline" onPress={() => router.replace(source.home)} />
              </View>
            }
          />
        </View>
      </ImportScreen>
    )
  }
  return (
    <ImportScreen title={source.title}>
      <Loading />
    </ImportScreen>
  )
}

/** Cancel with the web's confirmation; `dialog` must be rendered by the step. */
function useCancel<K extends ImportKind>(source: FlowSource<K>, item: ImportRecord<K>) {
  const cancel = useCancelImport(source.kind)
  const [request, setRequest] = useState<{ title: string; description: string; onConfirm: () => void } | null>(null)
  const ask = (started: boolean) =>
    setRequest({
      title: started ? 'Cancel the import?' : 'Cancel this import?',
      description: started ? source.cancel.started : source.cancel.before,
      onConfirm: () => cancel.mutate(item.id, { onError: (error) => Alert.alert('Import', importErrorMessage(source.kind, error)) }),
    })
  return { ask, pending: cancel.isPending, dialog: <CancelDialog request={request} onClose={() => setRequest(null)} /> }
}

function ScanningStep<K extends ImportKind>({ source, item }: { source: FlowSource<K>; item: ImportRecord<K> }) {
  const cancel = useCancel(source, item)
  const { headline, text } = source.scanning(item)
  return (
    <ImportScreen title={source.title} detail={source.detail(item)}>
      <ImportBody>
        <View accessibilityRole="progressbar" className="items-center gap-3 py-16">
          <Spinner size="large" />
          <Text className="text-center font-noto-medium text-[14px] leading-[20px]">{headline}</Text>
          <Text className="max-w-[384px] text-center text-[14px] leading-[20px] text-muted-foreground">{text}</Text>
          <Button label="Cancel" variant="outline" size="sm" disabled={cancel.pending} onPress={() => cancel.ask(false)} />
        </View>
      </ImportBody>
      {cancel.dialog}
    </ImportScreen>
  )
}

/** The ready step: the scan tree is fetched once, then the source's chooser. */
function ChooseStep<K extends ImportKind>({ source, item, chooser }: { source: FlowSource<K>; item: ImportRecord<K>; chooser: (item: ImportRecord<K>, tree: ImportTree<K>) => ReactNode }) {
  const cancel = useCancel(source, item)
  const scan = useImportTree(source.kind, item.id)
  return (
    <ImportScreen title={source.chooseTitle} detail={source.detail(item)} actions={<Button label="Cancel" variant="ghost" size="sm" disabled={cancel.pending} onPress={() => cancel.ask(false)} />}>
      {scan.data ? (
        chooser(item, scan.data)
      ) : scan.isError || scan.isSuccess ? (
        <View accessibilityRole="alert" className="flex-1 items-center justify-center gap-3 p-6">
          <Text className="text-center text-[14px] leading-[20px] text-muted-foreground">{scan.isError ? importErrorMessage(source.kind, scan.error) : source.treeMissing}</Text>
          <Button label="Retry" variant="outline" size="sm" onPress={() => void scan.refetch()} />
        </View>
      ) : (
        <Loading />
      )}
      {cancel.dialog}
    </ImportScreen>
  )
}

function ImportingStep<K extends ImportKind>({ source, item }: { source: FlowSource<K>; item: ImportRecord<K> }) {
  const cancel = useCancel(source, item)
  const { colors } = useTheme()
  const { done, total, failed } = item.progress
  const progress = source.importing.progress?.(item) ?? {
    label: source.importing.headline(item),
    step: null,
    done: done + failed,
    total,
    detail: `${done} of ${total} ${source.importing.unit} imported`,
  }
  // the poll keeps the same data while the import waits, so the minutes left need their own clock
  const until = source.importing.notice?.until(item) ?? null
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!until) return
    setNow(Date.now())
    const timer = setInterval(() => setNow(Date.now()), 15_000)
    return () => clearInterval(timer)
  }, [until])
  const notice = source.importing.notice?.text(until, now) ?? null
  return (
    <ImportScreen title={source.title} detail={source.detail(item)}>
      <ImportBody>
        <View accessibilityLiveRegion="polite" className="gap-4 py-10">
          <View className="flex-row items-center gap-2">
            <Spinner />
            <Text className="min-w-0 flex-1 font-noto-medium text-[14px] leading-[20px]">{progress.label}</Text>
            {progress.step ? <Text className="text-[12px] leading-[16px] text-muted-foreground tabular-nums">Step {progress.step} of 4</Text> : null}
          </View>
          <ProgressBar done={progress.done} total={progress.total} label="Import progress" />
          <View className="flex-row flex-wrap items-center gap-4">
            <Text className="text-[14px] leading-[20px] text-muted-foreground tabular-nums">{progress.detail}</Text>
            {failed > 0 ? <Text className="text-[14px] leading-[20px] text-destructive">{failed} failed</Text> : null}
            <View className="flex-1" />
            <Button label="Cancel import" variant="outline" size="sm" disabled={cancel.pending} onPress={() => cancel.ask(true)} />
          </View>
          {notice ? (
            <View className="flex-row items-start gap-2 rounded-md border border-border px-3 py-2">
              <Icon name="clock" size={16} color={colors.mutedForeground} />
              <Text className="min-w-0 flex-1 text-[14px] leading-[20px]">{notice}</Text>
            </View>
          ) : null}
          <Text className="text-[14px] leading-[20px] text-muted-foreground">{source.importing.note}</Text>
        </View>
      </ImportBody>
      {cancel.dialog}
    </ImportScreen>
  )
}

export interface Failure {
  key: string
  title: string
  error: string
}

/**
 * The result step: the headline with its icon, the error, the stats, what was imported (`children`), the notes, the
 * failures and "Import more".
 */
export function ResultStep<K extends ImportKind>({ source, item, summary, stats, columns, notes, notesTitle, failures, failuresTitle, again, children }: { source: FlowSource<K>; item: ImportRecord<K>; summary?: ReactNode; stats: readonly (readonly [string, number])[] | null; columns: 1 | 2; notes: ReactNode[]; notesTitle: string; failures: Failure[]; failuresTitle: string; again?: { label: string; primary: boolean }; children?: ReactNode }) {
  const { colors } = useTheme()
  const failed = item.status === 'failed'
  const rows = stats ? (columns === 1 ? stats.map((stat) => [stat]) : stats.flatMap((stat, index) => (index % 2 === 0 ? [stats.slice(index, index + 2)] : []))) : []
  return (
    <ImportScreen title={source.title} detail={source.detail(item)}>
      <ImportBody>
        <View className="gap-3">
          <View className="flex-row items-center gap-2">
            <Icon name={item.status === 'completed' ? 'checkCircle' : 'warning'} size={20} color={item.status === 'completed' ? colors.primary : failed ? colors.destructive : colors.mutedForeground} />
            <Text accessibilityRole="header" className="font-noto-semibold text-[16px] leading-[24px]">
              {importHeadline(item.status)}
            </Text>
          </View>
          {summary}
        </View>
        {rows.length > 0 ? (
          <View className="gap-2">
            {rows.map((row) => (
              <View key={row[0][0]} className="flex-row gap-2">
                {row.map(([label, value]) => (
                  <Stat key={label} label={label} value={value} />
                ))}
              </View>
            ))}
          </View>
        ) : null}
        {children}
        {notes.length > 0 ? (
          <View accessibilityLabel={notesTitle} className="gap-2">
            <SectionLabel>{notesTitle}</SectionLabel>
            {notes}
          </View>
        ) : null}
        {failures.length > 0 ? (
          <View accessibilityLabel={failuresTitle} className="gap-2">
            <SectionLabel>{failuresTitle}</SectionLabel>
            <View className="border-t border-border">
              {failures.map((failure) => (
                <View key={failure.key} className="gap-0.5 border-b border-border px-1 py-2">
                  <Text className="text-[13px]">{failure.title}</Text>
                  <Text className="text-[12px] leading-[16px] text-muted-foreground">{failure.error}</Text>
                </View>
              ))}
            </View>
          </View>
        ) : null}
        <View className="flex-row">
          <Button label={again?.label ?? 'Import more'} icon="import" variant={again?.primary ? 'primary' : 'outline'} onPress={() => router.replace(source.home)} />
        </View>
      </ImportBody>
    </ImportScreen>
  )
}

/** One conversion note line. */
export function NoteLine({ children }: { children: ReactNode }) {
  return <Text className="text-[14px] leading-[20px]">{children}</Text>
}
