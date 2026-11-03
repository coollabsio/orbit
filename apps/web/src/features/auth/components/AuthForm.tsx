import { useId } from 'react'
import { ApiProblem } from '@orbit/api/problem'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Label } from '@/components/ui/label'
import { cn } from 'cn'

export function AuthInput({ label, value, onChange, type = 'text', required = true }: { label: string; value: string; onChange?: (value: string) => void; type?: string; required?: boolean }) {
  const id = useId()
  const props = { id, required, readOnly: !onChange, value, onChange: (event: React.ChangeEvent<HTMLInputElement>) => onChange?.(event.target.value) }
  return <div className="grid gap-1.5"><Label htmlFor={id} className={cn('gap-1 text-muted-foreground', required && onChange && "after:text-primary after:content-['*']")}>{label}</Label>{type === 'password' ? <PasswordInput {...props} /> : <Input type={type} {...props} />}</div>
}

/** The centred card every auth screen sits in, under a soft accent glow. */
export function AuthCard({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <main className="grid min-h-screen place-items-center bg-background bg-[radial-gradient(ellipse_60%_45%_at_50%_0%,color-mix(in_oklch,var(--primary)_14%,transparent),transparent)] p-6">
      <section data-slot="auth-card" className={cn('w-[min(100%,460px)] rounded-xl border bg-card p-6 shadow-lg', className)}>{children}</section>
    </main>
  )
}

export function AuthBrand() {
  // 22px keeps the 11-unit pixel logo on whole device pixels.
  return <span className="flex items-center gap-2 text-[13px] font-bold tracking-[0.08em] text-primary uppercase"><img src="/logo.svg" alt="" className="size-[22px]" />Orbit</span>
}

/** Header, an inset panel with the fields and actions, and an optional footer row (Coolify's sign-in layout). */
export function AuthForm({ title, description, children, error, pending, submitLabel, onSubmit, actions, footer }: { title: string; description?: string; children: React.ReactNode; error: Error | null; pending: boolean; submitLabel: string; onSubmit: () => Promise<unknown>; actions?: React.ReactNode; footer?: React.ReactNode }) {
  return (
    <AuthCard className="p-0"><form onSubmit={(event) => {
      event.preventDefault()
      // Dismiss the mobile keyboard before replacing the login form with the app.
      if (document.activeElement instanceof HTMLElement) document.activeElement.blur()
      void onSubmit().catch(() => undefined)
    }}>
      <header className="grid gap-1 px-6 pt-6 pb-5">
        <h1 className="text-xl font-semibold">{title}</h1>
        {description ? <p className="text-[13px] text-muted-foreground">{description}</p> : null}
      </header>
      <div className={cn('-mx-px grid gap-4 rounded-xl border bg-background/40 p-6', !footer && '-mb-px')}>
        {children}
        {error ? <p className="text-[13px] text-destructive" role="alert">{error instanceof ApiProblem ? error.detail : 'The server could not complete the request.'}</p> : null}
        <div className="grid gap-3"><Button type="submit" size="lg" disabled={pending}>{pending ? 'Please wait…' : submitLabel}</Button>{actions}</div>
      </div>
      {footer ? <div className="px-6 py-4 text-center text-[13px] text-muted-foreground">{footer}</div> : null}
    </form></AuthCard>
  )
}
