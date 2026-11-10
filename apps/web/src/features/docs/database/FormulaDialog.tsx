import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react'
import { useQuery } from '@tanstack/react-query'
import { cn } from 'cn'
import { previewDatabaseFormula } from '@orbit/api/generated/sdk.gen'
import { FORMULA_FUNCTIONS } from '@orbit/database/formula/functions'
import { quote, toEditable, tokenize, type FormulaToken } from '@orbit/database/formula/tokens'
import type { FormulaProperty } from '@orbit/database/formula/values'
import type { Property, Row } from '@orbit/database/types'
import { parseValue } from '@orbit/database/values'
import { formulaError } from '@orbit/database/formula/values'
import { apiClient } from '@/api/client'
import { Button } from '@/components/ui/button'
import { Modal } from '@/components/common/Modal'
import { useDatabase, useUpdateProperty, viewerTimeZone } from './api'
import { FormulaCell } from './cells/FormulaCell'
import { useDatabaseEnv } from './context'

const MAX_EXPRESSION_CHARS = 2_000
const MAX_SUGGESTIONS = 8
const PREVIEW_DELAY_MS = 300

type Suggestion = { label: string; detail: string; insert: string; replace: number }

const TOKEN_CLASSES: Record<FormulaToken['kind'], string> = {
  prop: 'text-primary',
  function: 'text-sky-600 dark:text-sky-400',
  text: 'text-emerald-700 dark:text-emerald-400',
  number: 'text-amber-700 dark:text-amber-400',
  keyword: 'text-purple-600 dark:text-purple-400',
  operator: 'text-muted-foreground',
  name: '',
  space: '',
  unknown: 'text-destructive',
}

/**
 * The formula editor (spec P3 15.10): a code field with highlighting and completion of property names and functions,
 * the server's error under the field, and a preview of the first rows. Formulas run only on the server (D-P3-3).
 */
export function FormulaDialog({ property, onClose }: { property: FormulaProperty; onClose: () => void }) {
  const { workspaceId, databaseId } = useDatabaseEnv()
  const { data: database } = useDatabase(workspaceId, databaseId)
  const properties = useMemo(() => database?.properties ?? [], [database])
  const others = useMemo(() => properties.filter((other) => other.id !== property.id), [properties, property.id])
  const updateProperty = useUpdateProperty(workspaceId, databaseId)
  const [expression, setExpression] = useState(() => toEditable(property.config.expression, properties))
  const [caret, setCaret] = useState(expression.length)
  const [chosen, setChosen] = useState(0)
  const [closed, setClosed] = useState(false)
  const field = useRef<HTMLTextAreaElement>(null)
  const overlay = useRef<HTMLPreElement>(null)
  const debounced = useDebounced(expression, PREVIEW_DELAY_MS)
  const preview = useQuery({
    queryKey: ['database-formula-preview', workspaceId, databaseId, property.id, debounced],
    queryFn: async () => {
      const { data } = await previewDatabaseFormula({
        client: apiClient,
        path: { workspace_id: workspaceId, database_id: databaseId },
        query: { tz: viewerTimeZone() },
        body: { expression: debounced, property_id: property.id },
        throwOnError: true,
      })
      return data
    },
    placeholderData: (previous) => previous,
    staleTime: Infinity,
  })
  const result = preview.data
  const error = debounced === expression ? result?.error : undefined
  const tokens = useMemo(() => tokenize(expression, others), [expression, others])
  const suggestions = useMemo(() => (closed ? [] : suggest(expression.slice(0, caret), others)), [expression, caret, others, closed])

  const save = () =>
    updateProperty.mutate({ propertyId: property.id, body: { config: { expression: expression.trim() } } }, { onSuccess: onClose })
  const edit = (next: string, at: number) => {
    setExpression(next)
    setCaret(at)
    setChosen(0)
    setClosed(false)
  }
  const accept = (suggestion: Suggestion) => {
    const start = caret - suggestion.replace
    const next = expression.slice(0, start) + suggestion.insert + expression.slice(caret)
    const at = start + suggestion.insert.length
    edit(next, at)
    requestAnimationFrame(() => field.current?.setSelectionRange(at, at))
  }
  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
      event.preventDefault()
      save()
      return
    }
    if (suggestions.length === 0) return
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      const step = event.key === 'ArrowDown' ? 1 : -1
      setChosen((current) => (current + step + suggestions.length) % suggestions.length)
    } else if (event.key === 'Enter' || event.key === 'Tab') {
      event.preventDefault()
      accept(suggestions[Math.min(chosen, suggestions.length - 1)])
    } else if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      setClosed(true)
    }
  }

  const rows = (database?.rows ?? []).slice(0, 10)
  return (
    <Modal title={`Formula “${property.name}”`} description={'Use prop("Name") to read a property. The result is computed each time the database loads.'} onClose={onClose}>
      <div className="flex max-h-[70vh] flex-col gap-3 overflow-y-auto">
        <div className="relative rounded-md border bg-background font-mono text-sm focus-within:ring-2 focus-within:ring-ring/50">
          <pre
            ref={overlay}
            aria-hidden
            className="pointer-events-none absolute inset-0 m-0 overflow-hidden p-2 break-words whitespace-pre-wrap"
          >
            {tokens.map((token) => (
              <span
                key={token.start}
                className={cn(
                  token.kind === 'prop' && token.property === null ? 'text-destructive' : TOKEN_CLASSES[token.kind],
                  error && token.start < error.end && token.end > error.start && 'underline decoration-destructive decoration-wavy',
                )}
              >
                {token.text}
              </span>
            ))}
            {error && error.start >= expression.length ? <span className="underline decoration-destructive decoration-wavy">{' '}</span> : null}
            {'\n'}
          </pre>
          <textarea
            ref={field}
            aria-label="Formula"
            aria-invalid={error ? true : undefined}
            spellCheck={false}
            autoFocus
            rows={4}
            maxLength={MAX_EXPRESSION_CHARS}
            value={expression}
            placeholder='if(prop("Done"), "✓", "")'
            className="relative block min-h-24 w-full resize-y bg-transparent p-2 break-words whitespace-pre-wrap text-transparent caret-foreground outline-none placeholder:text-muted-foreground"
            onChange={(event) => edit(event.target.value, event.target.selectionStart)}
            onSelect={(event) => setCaret(event.currentTarget.selectionStart)}
            onScroll={(event) => {
              if (overlay.current) overlay.current.scrollTop = event.currentTarget.scrollTop
            }}
            onKeyDown={onKeyDown}
          />
        </div>
        {suggestions.length > 0 ? (
          <ul role="listbox" aria-label="Suggestions" className="flex flex-col rounded-md border p-1 text-sm">
            {suggestions.map((suggestion, index) => (
              <li
                key={suggestion.label}
                role="option"
                aria-selected={index === chosen}
                className={cn('flex cursor-pointer items-baseline gap-2 rounded-sm px-2 py-1', index === chosen && 'bg-accent')}
                onMouseDown={(event) => {
                  event.preventDefault()
                  accept(suggestion)
                }}
              >
                <span className="font-mono">{suggestion.label}</span>
                <span className="truncate text-xs text-muted-foreground">{suggestion.detail}</span>
              </li>
            ))}
          </ul>
        ) : null}
        {error ? (
          <p role="alert" className="text-sm text-destructive">
            {error.message}
          </p>
        ) : result?.result_type ? (
          <p className="text-xs text-muted-foreground">Result: {result.result_type}</p>
        ) : null}
        {result && !result.error && rows.length > 0 ? (
          <div className="flex flex-col gap-1">
            <span className="text-sm font-medium">Preview</span>
            <ul className="flex flex-col divide-y rounded-md border text-sm">
              {rows.map((row) => {
                const sample = result.samples.find((candidate) => candidate.row_id === row.id)
                return (
                  <li key={row.id} className="flex h-8 items-center gap-3 px-2">
                    <span className="w-1/3 truncate text-muted-foreground">{row.title || 'Untitled'}</span>
                    <span className="flex min-w-0 flex-1 items-center">
                      <FormulaCell
                        property={previewProperty(property, result.result_type ?? 'text')}
                        row={previewRow(row, property, result.result_type ?? 'text', sample?.value)}
                        width={320}
                        wrap={false}
                      />
                    </span>
                  </li>
                )
              })}
            </ul>
          </div>
        ) : null}
      </div>
      <div className="flex justify-end gap-2">
        <Button type="button" variant="outline" onClick={onClose}>
          Cancel
        </Button>
        <Button type="button" disabled={updateProperty.isPending} onClick={save}>
          Save
        </Button>
      </div>
    </Modal>
  )
}

function previewProperty(property: FormulaProperty, resultType: string): FormulaProperty {
  return { ...property, config: { ...property.config, result_type: resultType } }
}

/** A row that shows a preview sample as its formula cell. */
function previewRow(row: Row, property: FormulaProperty, resultType: string, raw: unknown): Row {
  const values = { ...row.values }
  delete values[property.id]
  const value = parseValue(previewProperty(property, resultType), raw)
  if (value) values[property.id] = value
  const message = formulaError(raw)
  return { ...row, values, formulaErrors: message === null ? undefined : { [property.id]: message } }
}

/** Completions for the text before the caret: property names inside `prop("`, else functions and `prop("…")` by name. */
function suggest(before: string, properties: readonly Property[]): Suggestion[] {
  const inProp = /prop\(\s*"((?:[^"\\]|\\.)*)$/.exec(before)
  if (inProp) {
    const typed = inProp[1].toLowerCase()
    return properties
      .filter((property) => property.name.toLowerCase().includes(typed))
      .slice(0, MAX_SUGGESTIONS)
      .map((property) => ({ label: property.name, detail: property.type, insert: `${quote(property.name).slice(1)})`, replace: inProp[1].length }))
  }
  const word = /[A-Za-z_][A-Za-z0-9_]*$/.exec(before)?.[0]
  if (!word) return []
  const typed = word.toLowerCase()
  const functions = FORMULA_FUNCTIONS.filter((fn) => fn.name.toLowerCase().startsWith(typed) && fn.name !== word).map((fn) => ({
    label: fn.name,
    detail: fn.signature,
    insert: `${fn.name}(`,
    replace: word.length,
  }))
  const props = properties
    .filter((property) => property.name.toLowerCase().startsWith(typed))
    .map((property) => ({ label: property.name, detail: `prop · ${property.type}`, insert: `prop(${quote(property.name)})`, replace: word.length }))
  return [...props, ...functions].slice(0, MAX_SUGGESTIONS)
}

function useDebounced<T>(value: T, delay: number): T {
  const [settled, setSettled] = useState(value)
  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), delay)
    return () => clearTimeout(timer)
  }, [value, delay])
  return settled
}
