import { AuthBrand, AuthCard } from './AuthForm'

export function AuthMessage({ title, detail }: { title: string; detail: string }) {
  return <AuthCard><AuthBrand /><h1 className="mt-4 mb-2">{title}</h1><p className="leading-[1.6] text-muted-foreground">{detail}</p></AuthCard>
}
