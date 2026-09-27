import { AuthBrand } from './AuthForm'

export function AuthMessage({ title, detail }: { title: string; detail: string }) {
  return <main className="grid min-h-screen place-items-center bg-background p-6"><section className="w-[min(100%,420px)] rounded-xl border border-border bg-card p-8 shadow-lg"><AuthBrand /><h1 className="mt-4 mb-2">{title}</h1><p className="leading-[1.6] text-muted-foreground">{detail}</p></section></main>
}
