// Copy button for code blocks (chat messages, docs editor). With a mouse it is hidden until the parent `group` is
// hovered or the button has keyboard focus; on touch it is always visible.
import { useState } from 'react'
import { Copy } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { cn } from 'cn'

function copyTextFallback(text: string) {
  const textarea = document.createElement('textarea')
  textarea.value = text
  textarea.setAttribute('readonly', '')
  textarea.style.position = 'fixed'
  textarea.style.top = '-9999px'
  textarea.style.left = '-9999px'
  document.body.appendChild(textarea)
  textarea.select()
  textarea.setSelectionRange(0, textarea.value.length)
  const copied = document.execCommand('copy')
  document.body.removeChild(textarea)
  if (!copied) throw new Error('Copy failed')
}

export function CopyCodeButton({ getText, className }: { getText: () => string; className?: string }) {
  const [copyState, setCopyState] = useState<'idle' | 'copied' | 'failed'>('idle')

  async function copyCode() {
    const text = getText()
    try {
      if (navigator.clipboard?.writeText) await navigator.clipboard.writeText(text)
      else copyTextFallback(text)
      setCopyState('copied')
    } catch {
      try {
        copyTextFallback(text)
        setCopyState('copied')
      } catch {
        setCopyState('failed')
      }
    }
    setTimeout(() => setCopyState('idle'), 1600)
  }

  return (
    <Button
      type="button"
      variant="outline"
      className={cn(
        'h-7 gap-1 bg-background/90 px-2 text-[11px] font-semibold text-muted-foreground shadow-sm transition-[opacity,background-color,color,transform] duration-150 focus-visible:opacity-100 hover-fine:opacity-0 hover-fine:group-hover:opacity-100 active:scale-95 active:not-aria-[haspopup]:translate-y-0 data-[state=copied]:bg-primary/10 data-[state=copied]:text-primary data-[state=failed]:bg-destructive/10 data-[state=failed]:text-destructive dark:bg-background/90 dark:data-[state=copied]:bg-primary/10 dark:data-[state=failed]:bg-destructive/10',
        className,
      )}
      data-state={copyState}
      title="Copy code"
      onClick={copyCode}
    >
      <Copy className="size-3.5" />
      {copyState === 'copied' ? 'Copied' : copyState === 'failed' ? 'Failed' : 'Copy'}
    </Button>
  )
}
