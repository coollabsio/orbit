import { Spinner } from '@/components/ui/spinner'

export function LoadingScreen() {
  // The spinner fades in only after 400ms, so fast loads never flash it (its own animation is the spin).
  return (
    <main className="grid min-h-full place-items-center bg-background">
      <div className="duration-200 animate-in fade-in fill-mode-both delay-400">
        <Spinner className="size-8 text-muted-foreground" />
      </div>
    </main>
  )
}
