import { useMemo, useRef, useState, type ReactNode } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { DocumentUpload, Warning } from 'reicon-react'
import { apiClient } from '@/api/client'
import { queryKeys } from '@/api/queryKeys'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Spinner } from '@/components/ui/spinner'
import { createDatabaseFromCsv, importDatabaseCsv } from '@orbit/api/generated/sdk.gen'
import type { DatabaseImportResult, Page } from '@orbit/api/generated/types.gen'
import {
  MAX_CSV_BYTES,
  csvTable,
  guessTargets,
  importColumns,
  isTitleTarget,
  mappingProblem,
  parseCsv,
  retarget,
  type ColumnTarget,
  type CsvTable,
  type ImportType,
} from '@orbit/database/csv'
import type { Property } from '@orbit/database/types'
import { csvImportErrorMessage } from './csvImportText'
import { PROPERTY_TYPES } from './propertyMeta'

const PREVIEW_ROWS = 5
const IMPORT_TYPES: ImportType[] = ['text', 'number', 'select', 'multi_select', 'status', 'date', 'person', 'checkbox', 'url']

/** Where the rows go: a new database (created with the request's place and title), or an existing one. */
export type CsvDestination =
  | { kind: 'new'; request: () => { title: string; parent_id?: string; teamspace_id?: string; private?: boolean } }
  | { kind: 'merge'; databaseId: string; properties: Property[]; canAddProperties: boolean }

type Loaded = { fileName: string; records: string[][] }

/** A dry run's result that would leave values empty: the user confirms before the import. */
type Pending = { result: DatabaseImportResult }

function targetValue(target: ColumnTarget): string {
  if (target.kind === 'property') return `p:${target.propertyId}`
  if (target.kind === 'new') return `n:${target.type}`
  return 'skip'
}

function parseTarget(value: string): ColumnTarget {
  if (value.startsWith('p:')) return { kind: 'property', propertyId: value.slice(2) }
  if (value.startsWith('n:')) return { kind: 'new', type: value.slice(2) as ImportType | 'title' }
  return { kind: 'skip' }
}

/** "3 values in “Price”" lines of a dry run that clears values. */
function clearedLines(result: DatabaseImportResult, table: CsvTable): string[] {
  return result.columns.flatMap((column, index) =>
    column.cleared > 0 ? [`${column.cleared.toLocaleString()} ${column.cleared === 1 ? 'value' : 'values'} in “${table.header[index]}”`] : [],
  )
}

/**
 * The CSV import (spec P2 9.4): pick a file, map each column to a property, "New <type>" or Skip with a preview of the
 * first rows, then import. Values that would not fit their property are listed for a confirmation first (a dry run).
 */
export function CsvImportFlow({
  workspaceId,
  destination,
  fields,
  onImported,
  onCancel,
}: {
  workspaceId: string
  destination: CsvDestination
  /** Extra fields above the mapping (a new database's title and place). */
  fields?: ReactNode
  onImported: (result: DatabaseImportResult, database: Page | null) => void
  onCancel?: () => void
}) {
  const queryClient = useQueryClient()
  const input = useRef<HTMLInputElement>(null)
  const [loaded, setLoaded] = useState<Loaded | null>(null)
  const [hasHeader, setHasHeader] = useState(true)
  const [targets, setTargets] = useState<ColumnTarget[]>([])
  const [error, setError] = useState<string | null>(null)
  const [pending, setPending] = useState<Pending | null>(null)
  const [busy, setBusy] = useState(false)
  const [dropOver, setDropOver] = useState(false)
  const properties = destination.kind === 'merge' ? destination.properties : null
  const table = useMemo(() => (loaded ? csvTable(loaded.records, hasHeader) : null), [loaded, hasHeader])

  const read = async (file: File | undefined) => {
    if (input.current) input.current.value = ''
    if (!file) return
    setError(null)
    if (file.size > MAX_CSV_BYTES) {
      setError(`“${file.name}” is larger than 5 MB. Split it into smaller files.`)
      return
    }
    const records = parseCsv(await file.text())
    if (records.length === 0) {
      setError(`“${file.name}” has no rows.`)
      return
    }
    setLoaded({ fileName: file.name, records })
    setTargets(guessTargets(csvTable(records, true), properties))
    setHasHeader(true)
    setPending(null)
  }

  if (!loaded || !table) {
    return (
      <div className="flex flex-col gap-3">
        {fields}
        <div
          data-slot="drop-zone"
          data-drop-over={dropOver || undefined}
          className="flex flex-col items-center gap-3 rounded-lg border border-dashed border-border px-4 py-8 text-center transition-[background-color,border-color] duration-150 ease-out data-drop-over:border-primary data-drop-over:bg-primary/5"
          onDragOver={(event) => {
            if (!event.dataTransfer.types.includes('Files')) return
            event.preventDefault()
            setDropOver(true)
          }}
          onDragLeave={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropOver(false)
          }}
          onDrop={(event) => {
            if (!event.dataTransfer.types.includes('Files')) return
            event.preventDefault()
            setDropOver(false)
            void read(event.dataTransfer.files[0])
          }}
        >
          <DocumentUpload className="size-6 text-muted-foreground/70" />
          <p className="text-sm text-muted-foreground">Drop a .csv file here (5 MB, 5,000 rows and 100 columns at most), or</p>
          <input
            ref={input}
            type="file"
            accept=".csv,.tsv,.txt,text/csv,text/tab-separated-values"
            className="hidden"
            aria-label="CSV file"
            onChange={(event) => void read(event.target.files?.[0])}
          />
          <Button type="button" variant="outline" onClick={() => input.current?.click()}>
            Choose file
          </Button>
        </div>
        {error ? <ErrorLine>{error}</ErrorLine> : null}
      </div>
    )
  }

  const problem = mappingProblem(table, targets, properties)
  const rowCount = table.rows.length

  const send = async (dryRun: boolean) => {
    setBusy(true)
    setError(null)
    const columns = importColumns(table, targets)
    try {
      if (destination.kind === 'merge') {
        const { data } = await importDatabaseCsv({
          client: apiClient,
          path: { workspace_id: workspaceId, database_id: destination.databaseId },
          body: { columns, rows: table.rows, dry_run: dryRun },
          throwOnError: true,
        })
        if (dryRun) return data
        void queryClient.invalidateQueries({ queryKey: queryKeys.database(workspaceId, destination.databaseId) })
        onImported(data, null)
      } else {
        const titleColumn = targets.findIndex((target) => isTitleTarget(target, null))
        const { data } = await createDatabaseFromCsv({
          client: apiClient,
          path: { workspace_id: workspaceId },
          body: { ...destination.request(), title_column: titleColumn, columns, rows: table.rows, dry_run: dryRun },
          throwOnError: true,
        })
        if (dryRun) return data.result
        void queryClient.invalidateQueries({ queryKey: queryKeys.pages.all(workspaceId) })
        onImported(data.result, data.database)
      }
    } catch (caught) {
      setError(csvImportErrorMessage(caught, table))
    } finally {
      setBusy(false)
    }
    return null
  }

  const start = async () => {
    const result = await send(true)
    if (!result) return
    if (result.cleared > 0) setPending({ result })
    else await send(false)
  }

  const choices = targetChoices(destination)
  return (
    <div className="flex min-h-0 flex-col gap-4">
      {fields}
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-sm">
        <span className="min-w-0 truncate font-medium text-foreground">{loaded.fileName}</span>
        <span className="text-muted-foreground tabular-nums">
          {rowCount.toLocaleString()} {rowCount === 1 ? 'row' : 'rows'} · {table.header.length} {table.header.length === 1 ? 'column' : 'columns'}
        </span>
        <Label className="ml-auto flex items-center gap-2 text-sm font-normal">
          <Checkbox
            checked={hasHeader}
            onCheckedChange={(checked) => {
              setHasHeader(checked)
              setTargets(guessTargets(csvTable(loaded.records, checked), properties))
              setPending(null)
            }}
          />
          First row is the header
        </Label>
        <Button type="button" variant="ghost" size="sm" onClick={() => setLoaded(null)}>
          Choose another file
        </Button>
      </div>
      <ul aria-label="Columns" className="flex max-h-64 flex-col divide-y divide-border overflow-y-auto rounded-lg border border-border">
        {table.header.map((name, index) => {
          const sample = table.rows.map((row) => row[index]?.trim() ?? '').find((cell) => cell !== '') ?? ''
          const target = targets[index] ?? { kind: 'skip' }
          return (
            <li key={index} className="flex items-center gap-3 px-3 py-1.5">
              <div className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-sm text-foreground">{name}</span>
                <span className="truncate text-xs text-muted-foreground">{sample || 'Empty'}</span>
              </div>
              <span aria-hidden="true" className="text-muted-foreground">
                →
              </span>
              <Select
                items={choices.items}
                value={targetValue(target)}
                onValueChange={(value) => {
                  if (!value) return
                  setTargets(retarget(table, targets, index, parseTarget(value), properties))
                  setPending(null)
                }}
              >
                <SelectTrigger className="w-52" aria-label={`Import “${name}” as`}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>{choices.content}</SelectContent>
              </Select>
            </li>
          )
        })}
      </ul>
      <Preview table={table} targets={targets} />
      {pending ? (
        <div role="alert" className="flex flex-col gap-2 rounded-lg border border-amber-500/40 bg-amber-500/5 p-3 text-sm">
          <span className="flex items-center gap-2 font-medium text-foreground">
            <Warning className="size-4 text-amber-600" aria-hidden="true" />
            {pending.result.cleared.toLocaleString()} {pending.result.cleared === 1 ? 'value does not fit its property' : 'values do not fit their property'} and will be left empty
          </span>
          <ul className="list-disc pl-6 text-muted-foreground">
            {clearedLines(pending.result, table).map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </div>
      ) : null}
      {error ? <ErrorLine>{error}</ErrorLine> : null}
      <div className="flex items-center justify-end gap-2">
        {problem ? <span className="mr-auto text-sm text-muted-foreground">{problem}</span> : null}
        {onCancel ? (
          <Button type="button" variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
        ) : null}
        <Button type="button" disabled={problem !== null || busy} onClick={() => void (pending ? send(false) : start())}>
          {busy ? <Spinner className="size-4" /> : null}
          {pending ? 'Import anyway' : `Import ${rowCount.toLocaleString()} ${rowCount === 1 ? 'row' : 'rows'}`}
        </Button>
      </div>
    </div>
  )
}

/** "New number", "New URL". */
function newLabel(type: ImportType): string {
  const label = PROPERTY_TYPES[type].label
  return `New ${label === label.toUpperCase() ? label : label.toLowerCase()}`
}

/** The target menu: the database's properties (a merge), new properties of each type, Skip. */
function targetChoices(destination: CsvDestination) {
  const items: { value: string; label: string }[] = []
  const existing = destination.kind === 'merge' ? destination.properties.filter((property) => property.type === 'title' || IMPORT_TYPES.includes(property.type as ImportType)) : []
  for (const property of existing) items.push({ value: `p:${property.id}`, label: property.name })
  const canAdd = destination.kind === 'new' || destination.canAddProperties
  if (destination.kind === 'new') items.push({ value: 'n:title', label: 'Title' })
  if (canAdd) for (const type of IMPORT_TYPES) items.push({ value: `n:${type}`, label: newLabel(type) })
  items.push({ value: 'skip', label: 'Skip' })
  const item = (value: string, label: string, icon?: ReactNode) => (
    <SelectItem key={value} value={value}>
      {icon}
      <span className="truncate">{label}</span>
    </SelectItem>
  )
  const iconOf = (type: keyof typeof PROPERTY_TYPES) => {
    const Icon = PROPERTY_TYPES[type].icon
    return <Icon className="text-muted-foreground" />
  }
  const content = (
    <>
      {existing.length > 0 ? (
        <SelectGroup>
          <SelectLabel>Properties</SelectLabel>
          {existing.map((property) => item(`p:${property.id}`, property.name, iconOf(property.type)))}
        </SelectGroup>
      ) : null}
      {canAdd ? (
        <SelectGroup>
          <SelectLabel>{destination.kind === 'new' ? 'Property type' : 'New property'}</SelectLabel>
          {destination.kind === 'new' ? item('n:title', 'Title', iconOf('title')) : null}
          {IMPORT_TYPES.map((type) => item(`n:${type}`, newLabel(type), iconOf(type)))}
        </SelectGroup>
      ) : null}
      <SelectGroup>{item('skip', 'Skip')}</SelectGroup>
    </>
  )
  return { items, content }
}

function Preview({ table, targets }: { table: CsvTable; targets: ColumnTarget[] }) {
  const rows = table.rows.slice(0, PREVIEW_ROWS)
  if (rows.length === 0) return null
  return (
    <div className="min-w-0 overflow-x-auto rounded-lg border border-border">
      <table aria-label="Preview" className="w-max min-w-full border-collapse text-xs">
        <thead>
          <tr>
            {table.header.map((name, index) => (
              <th
                key={index}
                data-skipped={targets[index]?.kind === 'skip' || undefined}
                className="max-w-48 truncate border-b border-border bg-muted/40 px-2 py-1.5 text-left font-medium text-foreground data-skipped:text-muted-foreground/60 data-skipped:line-through"
              >
                {name}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, rowIndex) => (
            <tr key={rowIndex} className="border-b border-border last:border-b-0">
              {table.header.map((_, index) => (
                <td
                  key={index}
                  data-skipped={targets[index]?.kind === 'skip' || undefined}
                  className="max-w-48 truncate px-2 py-1.5 text-foreground data-skipped:text-muted-foreground/60"
                >
                  {row[index] ?? ''}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function ErrorLine({ children }: { children: ReactNode }) {
  return (
    <p role="alert" className="flex items-start gap-2 text-sm text-destructive">
      <Warning className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
      {children}
    </p>
  )
}

/** Database "⋯ → Merge with CSV": adds a CSV's rows to the database. */
export function MergeCsvDialog({
  workspaceId,
  databaseId,
  properties,
  canAddProperties,
  onClose,
  onMerged,
}: {
  workspaceId: string
  databaseId: string
  properties: Property[]
  canAddProperties: boolean
  onClose: () => void
  onMerged: (result: DatabaseImportResult) => void
}) {
  return (
    <Dialog open onOpenChange={(open) => (open ? undefined : onClose())}>
      <DialogContent className="flex flex-col sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Merge with CSV</DialogTitle>
          <DialogDescription>
            Adds each row of the file as a new row. Existing rows stay as they are.
            {canAddProperties ? null : ' The database is locked, so columns can only go to existing properties.'}
          </DialogDescription>
        </DialogHeader>
        <CsvImportFlow
          workspaceId={workspaceId}
          destination={{ kind: 'merge', databaseId, properties, canAddProperties }}
          onCancel={onClose}
          onImported={(result) => onMerged(result)}
        />
      </DialogContent>
    </Dialog>
  )
}
