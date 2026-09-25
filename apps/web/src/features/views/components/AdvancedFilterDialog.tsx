import { useEffect, useId, useRef, useState } from 'react'
import { Add as Plus, Trash } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { FIELD_META, FIELD_ORDER, defaultValue, listValue, operatorLabel, operatorNeedsValue, valueSummary, withOperator, type FilterOptions } from '../filterFields'
import {
  MAX_FILTER_CONDITIONS,
  MAX_FILTER_DEPTH,
  appendChild,
  blankCondition,
  firstLocalIssue,
  parseIssuePath,
  samePath,
  updateNode,
  type FilterIssue,
  type NodePath,
} from '../filterTree'
import { countConditions, emptyFilter, isGroup, type Condition, type FilterField, type FilterGroup, type FilterNode, type FilterOperator } from '../viewState'
import { FilterValuePicker } from './FilterValuePicker'
import { DIALOG_MOTION, POPOVER_MOTION } from './motion'

const OPERATOR_ITEM = 'h-6 px-2 text-xs text-muted-foreground aria-pressed:bg-muted aria-pressed:text-foreground'

export interface AdvancedFilterDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  filter: FilterGroup
  options: FilterOptions
  onApply: (filter: FilterGroup) => void
  /** Server dry-run; resolves to the first 422 issue, or null when the tree is valid. */
  validate?: (filter: FilterGroup) => Promise<FilterIssue | null>
}

/** True while the latest input was a key press, so a keyboard-opened dialog skips its entrance. */
function useKeyboardInput(): boolean {
  const [keyboard, setKeyboard] = useState(false)
  useEffect(() => {
    const onKeyDown = () => setKeyboard(true)
    const onPointerDown = () => setKeyboard(false)
    document.addEventListener('keydown', onKeyDown, true)
    document.addEventListener('pointerdown', onPointerDown, true)
    return () => {
      document.removeEventListener('keydown', onKeyDown, true)
      document.removeEventListener('pointerdown', onPointerDown, true)
    }
  }, [])
  return keyboard
}

export function AdvancedFilterDialog({ open, onOpenChange, filter, options, onApply, validate }: AdvancedFilterDialogProps) {
  const keyboard = useKeyboardInput()
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {open ? (
        <DialogContent data-instant={keyboard || undefined} className={cn('gap-0 p-0 sm:max-w-2xl', DIALOG_MOTION)}>
          <TreeEditor filter={filter} options={options} onApply={onApply} validate={validate} onClose={() => onOpenChange(false)} />
        </DialogContent>
      ) : null}
    </Dialog>
  )
}

interface TreeEditorProps {
  filter: FilterGroup
  options: FilterOptions
  onApply: (filter: FilterGroup) => void
  validate?: (filter: FilterGroup) => Promise<FilterIssue | null>
  onClose: () => void
}

function nodeAt(root: FilterGroup, path: NodePath): FilterNode | undefined {
  let node: FilterNode | undefined = root
  for (const index of path) node = node && isGroup(node) ? node.children[index] : undefined
  return node
}

function TreeEditor({ filter, options, onApply, validate, onClose }: TreeEditorProps) {
  const [draft, setDraft] = useState<FilterGroup>(() => structuredClone(filter))
  const [issue, setIssue] = useState<FilterIssue | null>(null)
  const [checking, setChecking] = useState(false)
  // closing the dialog (Cancel, Escape, backdrop) mid-check unmounts the editor: the pending check must then do nothing
  const mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])
  const parsed = issue ? parseIssuePath(issue.path) : null
  // an issue that names no node in the tree shows above it, so Apply never fails silently
  const issueNode = parsed && parsed.length > 0 && nodeAt(draft, parsed) ? parsed : null
  const rootIssue = issue && issueNode === null ? issue.message : null

  const edit = (next: FilterGroup) => {
    setDraft(next)
    setIssue(null)
  }
  const apply = async () => {
    const local = firstLocalIssue(draft)
    if (local) {
      setIssue(local)
      return
    }
    if (validate) {
      setChecking(true)
      let serverIssue: FilterIssue | null
      try {
        serverIssue = await validate(draft)
      } catch {
        serverIssue = { path: 'filter', message: "Orbit couldn't check this filter. Try again." }
      }
      if (!mounted.current) return
      setChecking(false)
      if (serverIssue) {
        setIssue(serverIssue)
        return
      }
    }
    onApply(draft)
    onClose()
  }

  return (
    <>
      <DialogHeader className="gap-1 border-b border-border px-4 py-3 pr-10">
        <DialogTitle>Advanced filter</DialogTitle>
        <DialogDescription className="text-xs">Combine conditions with and/or groups, up to three levels deep.</DialogDescription>
      </DialogHeader>
      {/* disabled while the server checks, so the draft that passes is the draft that gets applied */}
      <fieldset disabled={checking} className="m-0 flex max-h-[60vh] min-w-0 flex-col gap-2 overflow-y-auto border-0 px-4 py-3">
        {rootIssue ? (
          <p role="alert" className="text-xs text-destructive">
            {rootIssue}
          </p>
        ) : null}
        <GroupEditor group={draft} path={[]} root={draft} options={options} issue={issue} issueNode={issueNode} onChange={edit} />
      </fieldset>
      <DialogFooter className="mx-0 mb-0 items-center px-4 py-3">
        <Button type="button" variant="ghost" className="sm:mr-auto" disabled={checking || draft.children.length === 0} onClick={() => edit(emptyFilter())}>
          Clear all
        </Button>
        <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
        <Button type="button" disabled={checking} onClick={() => void apply()}>
          {checking ? 'Checking…' : 'Apply filter'}
        </Button>
      </DialogFooter>
    </>
  )
}

interface NodeEditorProps {
  path: NodePath
  root: FilterGroup
  options: FilterOptions
  issue: FilterIssue | null
  issueNode: NodePath | null
  onChange: (root: FilterGroup) => void
}

const nodeNumber = (path: NodePath) => path.map((index) => index + 1).join('.')

function GroupEditor({ group, path, root, options, issue, issueNode, onChange }: NodeEditorProps & { group: FilterGroup }) {
  const depth = path.length + 1
  const full = countConditions(root) >= MAX_FILTER_CONDITIONS
  const label = path.length === 0 ? 'Root group' : `Group ${nodeNumber(path)}`
  const error = path.length > 0 && issue && samePath(issueNode, path) ? issue.message : null
  const setOp = (op: FilterGroup['op']) => onChange(updateNode(root, path, (node) => (isGroup(node) ? { ...node, op } : node)))
  return (
    <div role="group" aria-label={label} className={cn('flex flex-col gap-2', path.length > 0 && 'border-l-2 border-border py-1 pl-3', error && 'border-destructive/60')}>
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <span>Match</span>
        <ToggleGroup
          aria-label={`${label} operator`}
          variant="outline"
          size="sm"
          spacing={0}
          value={[group.op]}
          onValueChange={(value: string[]) => {
            const next = value[0]
            if (next === 'and' || next === 'or') setOp(next)
          }}
        >
          <ToggleGroupItem value="and" className={OPERATOR_ITEM}>
            And
          </ToggleGroupItem>
          <ToggleGroupItem value="or" className={OPERATOR_ITEM}>
            Or
          </ToggleGroupItem>
        </ToggleGroup>
        <span>{group.op === 'and' ? 'every condition below' : 'any condition below'}</span>
        {path.length > 0 ? (
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            className="ml-auto text-muted-foreground"
            aria-label={`Delete ${label.toLowerCase()}`}
            onClick={() => onChange(updateNode(root, path, () => null))}
          >
            <Trash />
          </Button>
        ) : null}
      </div>
      {error ? (
        <p role="alert" className="text-xs text-destructive">
          {error}
        </p>
      ) : null}
      {group.children.length === 0 ? <p className="text-xs text-muted-foreground">No conditions. This group matches every task.</p> : null}
      {group.children.map((child, index) => {
        const childPath = [...path, index]
        return isGroup(child) ? (
          <GroupEditor key={index} group={child} path={childPath} root={root} options={options} issue={issue} issueNode={issueNode} onChange={onChange} />
        ) : (
          <ConditionRow key={index} condition={child} path={childPath} root={root} options={options} issue={issue} issueNode={issueNode} onChange={onChange} />
        )
      })}
      <div className="-ml-2 flex gap-1">
        <Button type="button" variant="ghost" size="sm" className="text-muted-foreground" disabled={full} onClick={() => onChange(appendChild(root, path, blankCondition()))}>
          <Plus />
          Add condition
        </Button>
        {depth < MAX_FILTER_DEPTH ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="text-muted-foreground"
            disabled={full}
            onClick={() => onChange(appendChild(root, path, { op: group.op === 'and' ? 'or' : 'and', children: [blankCondition()] }))}
          >
            <Plus />
            Add group
          </Button>
        ) : null}
      </div>
    </div>
  )
}

function ConditionRow({ condition, path, root, options, issue, issueNode, onChange }: NodeEditorProps & { condition: Condition }) {
  const [picking, setPicking] = useState(false)
  const errorId = useId()
  const meta = FIELD_META[condition.field]
  const label = `Condition ${nodeNumber(path)}`
  const error = issue && samePath(issueNode, path) ? issue.message : null
  const count = meta.kind === 'list' ? listValue(condition).length : 1
  const replace = (next: Condition) => onChange(updateNode(root, path, () => next))
  const fieldItems = FIELD_ORDER.map((field) => ({ value: field, label: FIELD_META[field].label }))
  const operatorItems = meta.operators.map((operator) => ({ value: operator, label: operatorLabel(operator, count) }))
  return (
    <div role="group" aria-label={label} className="flex flex-col gap-1">
      <div className="flex flex-wrap items-center gap-1.5">
        <Select
          items={fieldItems}
          value={condition.field}
          onValueChange={(value) => {
            if (typeof value !== 'string') return
            const field = value as FilterField
            const operator = FIELD_META[field].defaultOperator
            replace({ field, operator, value: defaultValue(field, operator) })
          }}
        >
          <SelectTrigger size="sm" className="w-36" aria-label={`${label} field`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {fieldItems.map((item) => (
              <SelectItem key={item.value} value={item.value}>
                {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select
          items={operatorItems}
          value={condition.operator}
          onValueChange={(value) => {
            if (typeof value === 'string') replace(withOperator(condition, value as FilterOperator))
          }}
        >
          <SelectTrigger size="sm" className="w-36" aria-label={`${label} operator`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {operatorItems.map((item) => (
              <SelectItem key={item.value} value={item.value}>
                {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {operatorNeedsValue(condition.operator) ? (
          <Popover open={picking} onOpenChange={setPicking} modal={false}>
            <PopoverTrigger
              render={
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className={cn('max-w-60 min-w-0 justify-start font-normal', !valueSummary(condition, options) && 'text-muted-foreground')}
                  aria-label={`${label} value`}
                  aria-invalid={error ? true : undefined}
                  aria-describedby={error ? errorId : undefined}
                >
                  <span className="truncate">{valueSummary(condition, options) || 'Choose…'}</span>
                </Button>
              }
            />
            {picking ? (
              <PopoverContent align="start" className={cn('w-auto gap-0 p-0', POPOVER_MOTION)}>
                <FilterValuePicker condition={condition} options={options} onChange={replace} onDone={() => setPicking(false)} />
              </PopoverContent>
            ) : null}
          </Popover>
        ) : null}
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          className="ml-auto text-muted-foreground"
          aria-label={`Delete ${label.toLowerCase()}`}
          onClick={() => onChange(updateNode(root, path, () => null))}
        >
          <Trash />
        </Button>
      </div>
      {error ? (
        <p id={errorId} role="alert" className="text-xs text-destructive">
          {error}
        </p>
      ) : null}
    </div>
  )
}
