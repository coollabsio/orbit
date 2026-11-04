import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

type AccessLevel = 'none' | 'read' | 'write'

/** A resource row; a level without a scope is unavailable, except `read` with no `read` scope, which is always granted. */
export type ScopeResource<S extends string> = { name: string; hint: string; read?: S; write?: S; disabled?: boolean }

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

export function ScopeAccess<S extends string>({ resources, scopes, onChange }: { resources: ScopeResource<S>[]; scopes: S[]; onChange: (scopes: S[]) => void }) {
  return <>
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
  </>
}
