import { cn } from 'cn'

/** The note on a setting that an `ORBIT_*` environment variable sets: Admin cannot change it. */
export function SetByEnvironment({ variable, className }: { variable: string; className?: string }) {
  return (
    <p data-slot="set-by-environment" className={cn('text-xs text-muted-foreground', className)}>
      Set by <code className="font-mono">{variable}</code> on the server. Change the environment variable and restart Orbit.
    </p>
  )
}
