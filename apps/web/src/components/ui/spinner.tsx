import { cn } from "cn"

function Spinner({ className, ...props }: React.ComponentProps<"svg">) {
  return (
    <svg data-slot="spinner" role="status" aria-label="Loading" viewBox="0 0 24 24" fill="none" className={cn("size-4 shrink-0 animate-spin", className)} {...props}>
      <circle cx="12" cy="12" r="9.5" stroke="currentColor" strokeWidth="2.5" opacity="0.2" />
      <path d="M21.5 12a9.5 9.5 0 0 0-9.5-9.5" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" />
    </svg>
  )
}

/** A loading message with the spinner before it. */
function LoadingText({ className, children = "Loading…" }: { className?: string; children?: React.ReactNode }) {
  return (
    <span role="status" className={cn("inline-flex items-center gap-2 text-muted-foreground", className)}>
      <Spinner role="presentation" aria-hidden aria-label={undefined} className="size-3.5" />
      {children}
    </span>
  )
}

export { LoadingText, Spinner }
