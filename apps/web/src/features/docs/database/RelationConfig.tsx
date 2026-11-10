import { useState } from 'react'
import { ArrowLeft } from 'reicon-react'
import { Button } from '@/components/ui/button'
import {
  DropdownMenuCheckboxItem,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
} from '@/components/ui/dropdown-menu'
import { Field, FieldLabel } from '@/components/ui/field'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { confirmAction } from '@/components/common/confirmAction'
import { usePageTree } from '@/features/docs/api/pages'
import { pageTitle } from '@/features/docs/pageTree'
import {
  ROLLUP_FUNCTION_LABELS,
  rollupFits,
  rollupFunctionsFor,
  rollupTargetable,
  type RelationConfig,
  type RelationProperty,
  type RollupConfig,
  type RollupFunction,
  type RollupProperty,
} from '@orbit/database/relations'
import type { Property } from '@orbit/database/types'
import { useDatabase, useUpdateProperty } from './api'
import { useDatabaseEnv } from './context'

type Limit = RelationConfig['limit']
const LIMITS: { value: Limit; label: string }[] = [
  { value: 'none', label: 'No limit' },
  { value: 'one', label: '1 page' },
]

/** The databases the viewer can see (the docs tree), by title. */
function useDatabases(): { id: string; title: string }[] {
  const { workspaceId } = useDatabaseEnv()
  const tree = usePageTree(workspaceId)
  return (tree.data ?? []).filter((page) => page.kind === 'database').map((page) => ({ id: page.id, title: pageTitle(page) }))
}

function useDatabaseTitle(databaseId: string): string {
  return useDatabases().find((database) => database.id === databaseId)?.title ?? 'the related database'
}

/** A labelled select of the setup forms. */
function SettingSelect<T extends string>({ label, value, items, placeholder, onChange }: {
  label: string
  value: T | null
  items: { value: T; label: string }[]
  placeholder: string
  onChange: (value: T) => void
}) {
  return (
    <Field className="gap-1.5">
      <FieldLabel className="text-xs text-muted-foreground">{label}</FieldLabel>
      <Select items={items} value={value} onValueChange={(next) => next && onChange(next as T)}>
        <SelectTrigger size="sm" aria-label={label} className="w-full">
          <SelectValue placeholder={placeholder} />
        </SelectTrigger>
        <SelectContent>
          {items.map((item) => (
            <SelectItem key={item.value} value={item.value}>
              {item.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Field>
  )
}

function SetupHeader({ title, onBack }: { title: string; onBack: () => void }) {
  return (
    <div className="flex items-center gap-1 border-b border-border p-1">
      <Button type="button" variant="ghost" size="icon-sm" aria-label="Back to the types" onClick={onBack}>
        <ArrowLeft />
      </Button>
      <span className="text-sm font-medium">{title}</span>
    </div>
  )
}

/** A new relation (spec P3 11.8): the target database, the limit and "Show on <database>" (two-way). */
export function RelationSetup({ onCreate, onBack }: { onCreate: (config: Record<string, unknown>) => void; onBack: () => void }) {
  const databases = useDatabases()
  const [target, setTarget] = useState<string | null>(null)
  const [limit, setLimit] = useState<Limit>('none')
  const [twoWay, setTwoWay] = useState(false)
  const targetTitle = databases.find((database) => database.id === target)?.title
  return (
    <div className="flex flex-col" onKeyDown={(event) => event.stopPropagation()}>
      <SetupHeader title="Relation" onBack={onBack} />
      <div className="flex flex-col gap-3 p-3">
        <SettingSelect
          label="Related to"
          value={target}
          placeholder="Choose a database"
          items={databases.map((database) => ({ value: database.id, label: database.title }))}
          onChange={setTarget}
        />
        <SettingSelect label="Limit" value={limit} items={LIMITS} placeholder="No limit" onChange={setLimit} />
        <label className="flex items-center justify-between gap-3 text-sm">
          <span className="min-w-0 truncate">Show on {targetTitle ?? 'the related database'}</span>
          <Switch checked={twoWay} disabled={!target} onCheckedChange={setTwoWay} />
        </label>
        <Button type="button" size="sm" disabled={!target} onClick={() => target && onCreate({ database_id: target, limit, two_way: twoWay })}>
          Add relation
        </Button>
      </div>
    </div>
  )
}

/** The functions of a rollup target, keeping `current` when it still fits. */
function fittingFunction(target: Property | undefined, current: RollupFunction): RollupFunction {
  if (!target || rollupFits(current, target)) return current
  return rollupFunctionsFor(target)[0] ?? 'show_original'
}

/** A rollup's three choices; the relation's target database gives the property list. */
function useRollupChoices(relationId: string | null, targetId: string | null) {
  const { workspaceId, databaseId } = useDatabaseEnv()
  const own = useDatabase(workspaceId, databaseId)
  const relations = (own.data?.properties ?? []).filter((property): property is RelationProperty => property.type === 'relation')
  const relation = relations.find((property) => property.id === relationId)
  const targetDatabase = useDatabase(workspaceId, relation?.config.database_id ?? databaseId)
  const targets = relation ? (targetDatabase.data?.properties ?? []).filter(rollupTargetable) : []
  const target = targets.find((property) => property.id === targetId)
  return { relations, relation, targets, target }
}

/** A new rollup: the relation, the property of the related pages and the function. */
export function RollupSetup({ onCreate, onBack }: { onCreate: (config: RollupConfig) => void; onBack: () => void }) {
  const [relationId, setRelationId] = useState<string | null>(null)
  const [targetId, setTargetId] = useState<string | null>(null)
  const [fn, setFn] = useState<RollupFunction>('show_original')
  const { relations, targets, target } = useRollupChoices(relationId, targetId)
  return (
    <div className="flex flex-col" onKeyDown={(event) => event.stopPropagation()}>
      <SetupHeader title="Rollup" onBack={onBack} />
      <div className="flex flex-col gap-3 p-3">
        {relations.length === 0 ? (
          <p className="text-xs text-muted-foreground">A rollup shows values of related pages. Add a relation property first.</p>
        ) : (
          <>
            <SettingSelect
              label="Relation"
              value={relationId}
              placeholder="Choose a relation"
              items={relations.map((property) => ({ value: property.id, label: property.name }))}
              onChange={(id) => {
                setRelationId(id)
                setTargetId(null)
              }}
            />
            <SettingSelect
              label="Property"
              value={targetId}
              placeholder="Choose a property"
              items={targets.map((property) => ({ value: property.id, label: property.name }))}
              onChange={(id) => {
                setTargetId(id)
                setFn((current) => fittingFunction(targets.find((property) => property.id === id), current))
              }}
            />
            <SettingSelect
              label="Calculate"
              value={fn}
              placeholder="Show original"
              items={(target ? rollupFunctionsFor(target) : []).map((value) => ({ value, label: ROLLUP_FUNCTION_LABELS[value] }))}
              onChange={setFn}
            />
          </>
        )}
        <Button
          type="button"
          size="sm"
          disabled={!relationId || !target}
          onClick={() => relationId && targetId && onCreate({ relation_property_id: relationId, target_property_id: targetId, function: fn })}
        >
          Add rollup
        </Button>
      </div>
    </div>
  )
}

/** The relation part of "Edit property": the target, the limit and the two-way switch (a synced side only has the limit). */
export function RelationMenu({ property }: { property: RelationProperty }) {
  const { workspaceId, databaseId } = useDatabaseEnv()
  const updateProperty = useUpdateProperty(workspaceId, databaseId)
  const { database_id, limit, synced_property_id, primary } = property.config
  const targetTitle = useDatabaseTitle(database_id)
  const save = (change: { limit?: Limit; two_way?: boolean }) =>
    updateProperty.mutate({ propertyId: property.id, body: { config: { database_id, limit, ...change } } })
  const setTwoWay = async (on: boolean) => {
    if (
      !on &&
      !(await confirmAction({
        title: `Stop showing on ${targetTitle}?`,
        description: `Its relation property in ${targetTitle} is deleted. The links stay here.`,
        confirmLabel: 'Stop showing',
        danger: true,
      }))
    ) {
      return
    }
    save({ two_way: on })
  }
  return (
    <>
      <DropdownMenuSeparator />
      <DropdownMenuGroup>
        <DropdownMenuLabel>Related to</DropdownMenuLabel>
        <DropdownMenuItem disabled>{targetTitle}</DropdownMenuItem>
      </DropdownMenuGroup>
      <DropdownMenuGroup>
        <DropdownMenuLabel>Limit</DropdownMenuLabel>
        <DropdownMenuRadioGroup value={limit} onValueChange={(value: Limit) => save({ limit: value })}>
          {LIMITS.map((item) => (
            <DropdownMenuRadioItem key={item.value} value={item.value}>
              {item.label}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuGroup>
      {primary ? (
        <DropdownMenuCheckboxItem checked={synced_property_id !== null} onCheckedChange={(on: boolean) => void setTwoWay(on)}>
          Show on {targetTitle}
        </DropdownMenuCheckboxItem>
      ) : (
        <DropdownMenuItem disabled>Two-way: edit it from {targetTitle}</DropdownMenuItem>
      )}
    </>
  )
}

/** The rollup part of "Edit property": relation, property and function submenus. */
export function RollupMenu({ property }: { property: RollupProperty }) {
  const { workspaceId, databaseId } = useDatabaseEnv()
  const updateProperty = useUpdateProperty(workspaceId, databaseId)
  const config = property.config
  /** A relation picked in the menu: it saves with the property picked next (the property list is the relation's). */
  const [pendingRelation, setPendingRelation] = useState<string | null>(null)
  const relationId = pendingRelation ?? config.relation_property_id
  const { relations, relation, targets, target } = useRollupChoices(relationId, pendingRelation ? null : config.target_property_id)
  const save = (next: RollupConfig) => {
    setPendingRelation(null)
    updateProperty.mutate({ propertyId: property.id, body: { config: next } })
  }
  return (
    <>
      <DropdownMenuSeparator />
      <RollupSubmenu
        label="Relation"
        value={relationId}
        current={relation?.name}
        items={relations.map((candidate) => ({ value: candidate.id, label: candidate.name }))}
        onChange={(id) => setPendingRelation(id === config.relation_property_id ? null : id)}
      />
      <RollupSubmenu
        label="Property"
        value={target?.id ?? ''}
        current={target?.name ?? (pendingRelation ? 'Choose…' : undefined)}
        items={targets.map((candidate) => ({ value: candidate.id, label: candidate.name }))}
        onChange={(id) =>
          save({ relation_property_id: relationId, target_property_id: id, function: fittingFunction(targets.find((candidate) => candidate.id === id), config.function) })
        }
      />
      <RollupSubmenu
        label="Calculate"
        value={config.function}
        current={ROLLUP_FUNCTION_LABELS[config.function]}
        items={(target ? rollupFunctionsFor(target) : [config.function]).map((value) => ({ value, label: ROLLUP_FUNCTION_LABELS[value] }))}
        onChange={(fn) => save({ ...config, function: fn as RollupFunction })}
      />
    </>
  )
}

function RollupSubmenu({ label, value, current, items, onChange }: {
  label: string
  value: string
  current: string | undefined
  items: { value: string; label: string }[]
  onChange: (value: string) => void
}) {
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>
        <span className="text-muted-foreground">{label}</span>
        <span className="ml-auto max-w-28 truncate">{current ?? 'None'}</span>
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent className="max-h-72 w-52 overflow-y-auto">
        <DropdownMenuRadioGroup value={value} onValueChange={(next: string) => onChange(next)}>
          {items.map((item) => (
            <DropdownMenuRadioItem key={item.value} value={item.value}>
              {item.label}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  )
}
