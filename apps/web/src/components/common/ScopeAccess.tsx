import { Checkbox } from '@/components/ui/checkbox'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

type AccessLevel = 'none' | 'read' | 'write'

/** A resource row; a level without a scope is unavailable, except `read` with no `read` scope, which is always granted. */
export type ScopeResource<S extends string> = { name: string; hint: string; read?: S; write?: S; disabled?: boolean }

export type ScopePreset<S extends string> = { id: string; label: string; scopes: S[] }

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

/** The access section of the OAuth consent page and the API token form: a preset, then one row per resource. */
export function AccessPicker<S extends string>({ id, resources, presets, scopes, onChange }: { id: string; resources: ScopeResource<S>[]; presets: ScopePreset<S>[]; scopes: S[]; onChange: (scopes: S[]) => void }) {
  const blocked = new Set(resources.filter((resource) => resource.disabled).flatMap((resource) => [resource.read, resource.write]))
  const available = presets.filter((p) => !p.scopes.some((scope) => blocked.has(scope)))
  const preset = available.find((p) => p.scopes.length === scopes.length && p.scopes.every((scope) => scopes.includes(scope)))
  return <div role="group" aria-labelledby={id} className="grid gap-3">
    <div className="flex items-center justify-between gap-3">
      <span id={id} className="text-sm font-medium">Access</span>
      <Select value={preset?.id ?? 'custom'} onValueChange={(value) => { const next = available.find((p) => p.id === value); if (next) onChange(next.scopes) }}>
        <SelectTrigger size="sm" aria-label="Access preset" className="w-auto"><SelectValue>{preset?.label ?? 'Custom'}</SelectValue></SelectTrigger>
        <SelectContent>
          {available.map((p) => <SelectItem key={p.id} value={p.id}>{p.label}</SelectItem>)}
          {preset ? null : <SelectItem value="custom" disabled>Custom</SelectItem>}
        </SelectContent>
      </Select>
    </div>
    <p className="text-sm text-muted-foreground">Pick a preset or set each one.</p>
    {resources.map((resource) => {
      const level = accessLevel(resource, scopes)
      return <div key={resource.name} className="flex items-center justify-between gap-3">
        <div className="grid gap-0.5">
          <span className="text-sm font-medium">{resource.name}</span>
          <span className="text-xs text-muted-foreground">{resource.hint}</span>
        </div>
        <ToggleGroup aria-label={resource.name} variant="outline" size="sm" spacing={0} value={[level]} onValueChange={(value) => {
          const next = value.at(-1)
          if (next === 'none' || next === 'read' || next === 'write') onChange(withAccessLevel(resource, scopes, next))
        }}>
          {(['none', 'read', 'write'] as const).map((option) => <ToggleGroupItem key={option} value={option}
            disabled={resource.disabled ? option !== 'none' : option === 'none' ? !resource.read : option === 'write' ? !resource.write : false}
            className="disabled:opacity-25 aria-pressed:bg-primary aria-pressed:text-primary-foreground">
            {option === 'none' ? 'No access' : option === 'read' ? 'Read' : 'Write'}
          </ToggleGroupItem>)}
        </ToggleGroup>
      </div>
    })}
  </div>
}

/** The project checklist of the OAuth consent page and the API token form, with "Select all" above two or more projects. */
export function ProjectPicker({ id, projects, selected, onChange, empty }: { id: string; projects: { id: string; name: string }[]; selected: string[]; onChange: (ids: string[]) => void; empty?: string }) {
  return <div role="group" aria-labelledby={id} className="grid gap-3">
    <div className="flex items-center justify-between gap-3">
      <span id={id} className="text-sm font-medium">Projects</span>
      {projects.length > 1 ? <Label className="flex items-center gap-2 text-sm font-normal text-muted-foreground">
        <Checkbox checked={selected.length === projects.length} indeterminate={selected.length > 0 && selected.length < projects.length} onCheckedChange={(checked) => onChange(checked ? projects.map((project) => project.id) : [])} />
        Select all
      </Label> : null}
    </div>
    {projects.length ? <div className="-m-1 grid max-h-64 gap-3 overflow-y-auto p-1">
      {projects.map((project) => <Label key={project.id} className="flex items-center gap-2">
        <Checkbox checked={selected.includes(project.id)} onCheckedChange={(checked) => onChange(checked ? [...selected, project.id] : selected.filter((other) => other !== project.id))} />
        {project.name}
      </Label>)}
    </div> : null}
    {empty ? <p className="text-sm text-muted-foreground">{empty}</p> : null}
  </div>
}
