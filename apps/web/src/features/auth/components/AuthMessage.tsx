import { AuthCard } from './AuthForm'

export function AuthMessage({ title, detail }: { title: string; detail: string }) {
  return <AuthCard><h1 className="mb-1 text-xl font-semibold">{title}</h1><p className="text-[13px] leading-[1.6] text-muted-foreground">{detail}</p></AuthCard>
}
