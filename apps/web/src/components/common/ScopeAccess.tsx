import type { ReactNode } from 'react'
import { cn } from 'cn'
import { InfoTip } from '@/components/common/InfoTip'
import { buttonVariants } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Label } from '@/components/ui/label'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

type AccessLevel = 'none' | 'read' | 'write'

/** A resource row; a level without a scope is unavailable, except `read` with no `read` scope, which is always granted. `details` shows under the row, such as the project list of tasks. */
export type ScopeResource<S extends string> = { name: string; hint: string; read?: S; write?: S; disabled?: boolean; details?: ReactNode }

function accessLevel<S extends string>(resource: ScopeResource<S>, scopes: readonly S[]): AccessLevel {
  if (resource.write && scopes.includes(resource.write)) return 'write'
  return !resource.read || scopes.includes(resource.read) ? 'read' : 'none'
}

function withAccessLevel<S extends string>(resource: ScopeResource<S>, scopes: readonly S[], level: AccessLevel): S[] {
  return [
    ...scopes.filter((scope) => scope !== resource.read && scope !== resource.write),
    ...(level !== 'none' && resource.read ? [resource.read] : []),
    ...(level === 'write' && resource.write ? [resource.write] : []),
  ]
}

/** The access section of the OAuth consent page and the API token form: one row per resource. */
export function AccessPicker<S extends string>({ id, resources, scopes, onChange }: { id: string; resources: ScopeResource<S>[]; scopes: S[]; onChange: (scopes: S[]) => void }) {
  return <div role="group" aria-labelledby={id} className="grid gap-3">
    <span id={id} className="sr-only">Access</span>
    {resources.map((resource) => {
      const level = accessLevel(resource, scopes)
      return <div key={resource.name} className="grid gap-3">
      <div className="flex items-center justify-between gap-3">
        <span className="inline-flex items-center gap-1.5 text-sm font-medium">{resource.name} <InfoTip text={resource.hint} /></span>
        <ToggleGroup aria-label={resource.name} variant="outline" size="sm" spacing={0} value={[level]} onValueChange={(value) => {
          const next = value.at(-1)
          if (next === 'none' || next === 'read' || next === 'write') onChange(withAccessLevel(resource, scopes, next))
        }}>
          {(['none', 'read', 'write'] as const).map((option) => <ToggleGroupItem key={option} value={option}
            disabled={resource.disabled ? option !== 'none' : option === 'none' ? !resource.read : option === 'write' ? !resource.write : false}
            className={cn(buttonVariants({ variant: option === level ? 'default' : 'outline', size: 'sm' }), 'disabled:opacity-25 aria-pressed:bg-primary aria-pressed:text-primary-foreground aria-pressed:hover:bg-primary/80 hover:translate-y-0 hover:shadow-[0_2px_0_var(--button-depth)]')}>
            {option === 'none' ? 'No access' : option === 'read' ? 'Read' : 'Write'}
          </ToggleGroupItem>)}
        </ToggleGroup>
      </div>
      {resource.details ? <div className="border-l pl-4">{resource.details}</div> : null}
      </div>
    })}
  </div>
}

/** The project checklist under the Tasks access row, with "Select all" above two or more projects. `all` covers every project, including future ones, and hides the list. Its title is for screen readers only. */
/** Without `onAllChange` there is no "All projects" choice: only the listed projects can be picked. */
export function ProjectPicker({ id, projects, selected, onChange, all = false, onAllChange, empty }: { id: string; projects: { id: string; name: string }[]; selected: string[]; onChange: (ids: string[]) => void; all?: boolean; onAllChange?: (all: boolean) => void; empty?: string }) {
  return <div role="group" aria-labelledby={id} className="grid gap-3">
    <span id={id} className="sr-only">Projects</span>
    {onAllChange ? <Label className="flex items-center gap-2">
      <Checkbox checked={all} onCheckedChange={(checked) => onAllChange(Boolean(checked))} />
      All projects, including future ones
    </Label> : null}
    {all ? null : <>
    {projects.length > 1 ? <Label className="flex items-center gap-2 text-sm font-normal text-muted-foreground">
      <Checkbox checked={selected.length === projects.length} indeterminate={selected.length > 0 && selected.length < projects.length} onCheckedChange={(checked) => onChange(checked ? projects.map((project) => project.id) : [])} />
      Select all
    </Label> : null}
    {/* py-2 holds the checkbox's 8px tap area, so a short list does not scroll and catch the page's wheel. */}
    {projects.length ? <div className="-mx-1 -my-2 grid max-h-64 gap-3 overflow-y-auto px-1 py-2">
      {projects.map((project) => <Label key={project.id} className="flex items-center gap-2">
        <Checkbox checked={selected.includes(project.id)} onCheckedChange={(checked) => onChange(checked ? [...selected, project.id] : selected.filter((other) => other !== project.id))} />
        {project.name}
      </Label>)}
    </div> : null}
    {empty ? <p className="text-sm text-muted-foreground">{empty}</p> : null}
    </>}
  </div>
}
