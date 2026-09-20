import { ApiProblem } from '@/api/problem'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'

export function AuthInput({ label, value, onChange, type = 'text', required = true }: { label: string; value: string; onChange: (value: string) => void; type?: string; required?: boolean }) {
  return <Label className="grid gap-1.5 text-[13px] text-muted-foreground"><span>{label}</span><Input type={type} required={required} value={value} onChange={(event) => onChange(event.target.value)} /></Label>
}

export function AuthForm({ title, children, error, pending, submitLabel, onSubmit, footer }: { title: string; children: React.ReactNode; error: Error | null; pending: boolean; submitLabel: string; onSubmit: () => Promise<unknown>; footer?: React.ReactNode }) {
  return (
    <main className="grid min-h-screen place-items-center bg-background p-6"><form className="grid w-[min(100%,420px)] gap-[18px] rounded-xl border border-border bg-card p-8 shadow-lg" onSubmit={(event) => {
      event.preventDefault()
      // Dismiss the mobile keyboard before replacing the login form with the app.
      if (document.activeElement instanceof HTMLElement) document.activeElement.blur()
      void onSubmit().catch(() => undefined)
    }}>
      <span className="text-[13px] font-bold tracking-[0.08em] text-primary uppercase">Orbit</span><h1 className="mt-4 mb-2">{title}</h1>
      <div className="grid gap-[14px]">{children}</div>
      {error ? <p className="text-[13px] text-destructive" role="alert">{error instanceof ApiProblem ? error.detail : 'The server could not complete the request.'}</p> : null}
      <Button type="submit" disabled={pending}>{pending ? 'Please wait…' : submitLabel}</Button>
      {footer ? <div className="text-center text-[13px]">{footer}</div> : null}
    </form></main>
  )
}
