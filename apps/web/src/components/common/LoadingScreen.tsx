import { Spinner } from '@/components/ui/spinner'

export function LoadingScreen() {
  return (
    <main className="grid min-h-full place-items-center bg-background">
      <Spinner className="size-8 text-muted-foreground" />
    </main>
  )
}
