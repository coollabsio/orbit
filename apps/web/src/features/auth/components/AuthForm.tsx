import { ApiProblem } from '@/api/problem'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'

export function AuthInput({ label, value, onChange, type = 'text', required = true }: { label: string; value: string; onChange?: (value: string) => void; type?: string; required?: boolean }) {
  return <Label className="grid gap-1.5"><span>{label}</span><Input type={type} required={required} readOnly={!onChange} value={value} onChange={(event) => onChange?.(event.target.value)} /></Label>
}

/** The centred card every auth screen sits in. */
export function AuthCard({ children }: { children: React.ReactNode }) {
  return (
    <main className="grid min-h-screen place-items-center bg-background p-6">
      <section data-slot="auth-card" className="w-[min(100%,420px)] rounded-xl border bg-card p-8 shadow-lg">{children}</section>
    </main>
  )
}

export function AuthBrand() {
  // 22px keeps the 11-unit pixel logo on whole device pixels.
  return <span className="flex items-center gap-2 text-[13px] font-bold tracking-[0.08em] text-primary uppercase"><img src="/logo.svg" alt="" className="size-[22px]" />Orbit</span>
}

export function AuthForm({ title, children, error, pending, submitLabel, onSubmit, footer }: { title: string; children: React.ReactNode; error: Error | null; pending: boolean; submitLabel: string; onSubmit: () => Promise<unknown>; footer?: React.ReactNode }) {
  return (
    <AuthCard><form className="grid gap-[18px]" onSubmit={(event) => {
      event.preventDefault()
      // Dismiss the mobile keyboard before replacing the login form with the app.
      if (document.activeElement instanceof HTMLElement) document.activeElement.blur()
      void onSubmit().catch(() => undefined)
    }}>
      <AuthBrand /><h1 className="mt-4 mb-2">{title}</h1>
      <div className="grid gap-[14px]">{children}</div>
      {error ? <p className="text-[13px] text-destructive" role="alert">{error instanceof ApiProblem ? error.detail : 'The server could not complete the request.'}</p> : null}
      <Button type="submit" disabled={pending}>{pending ? 'Please wait…' : submitLabel}</Button>
      {footer ? <div className="text-center text-[13px]">{footer}</div> : null}
    </form></AuthCard>
  )
}
