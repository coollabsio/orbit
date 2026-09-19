export function LoadingScreen() {
  return (
    <main className="grid min-h-full place-items-center bg-background">
      <div
        role="status"
        aria-label="Loading"
        className="size-8 animate-spin rounded-full border-[3px] border-border border-t-primary"
      />
    </main>
  )
}
