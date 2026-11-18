// Copy button for code blocks (chat messages, docs editor). With a mouse it is hidden until the parent `group` is
// hovered or the button has keyboard focus; on touch it is always visible.
import { toast } from 'sonner'
import { useState } from 'react'
import { Copy } from 'reicon-react'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { cn } from 'cn'
import { writeClipboard } from '@/lib/clipboard'

export function CopyCodeButton({ getText, className }: { getText: () => string; className?: string }) {
  const [copyState, setCopyState] = useState<'idle' | 'copied' | 'failed'>('idle')

  async function copyCode() {
    const text = getText()
    try {
      await writeClipboard(text)
      setCopyState('copied')
      toast('Code copied')
    } catch {
      setCopyState('failed')
      toast.error('Could not copy the code.')
    }
    setTimeout(() => setCopyState('idle'), 1600)
  }

  return (
    <Tip label="Copy code">
      <Button
        type="button"
        variant="outline"
        className={cn(
          'h-7 gap-1 bg-background/90 px-2 text-[0.6875rem] font-semibold text-muted-foreground shadow-sm transition-[opacity,background-color,color,transform] duration-150 focus-visible:opacity-100 hover-fine:opacity-0 hover-fine:group-hover:opacity-100 active:scale-95 active:not-aria-[haspopup]:translate-y-0 data-[state=copied]:bg-primary/10 data-[state=copied]:text-primary data-[state=failed]:bg-destructive/10 data-[state=failed]:text-destructive dark:bg-background/90 dark:data-[state=copied]:bg-primary/10 dark:data-[state=failed]:bg-destructive/10',
          className,
        )}
        data-state={copyState}
        onClick={copyCode}
      >
        <Copy className="size-3.5" />
        {copyState === 'copied' ? 'Copied' : copyState === 'failed' ? 'Failed' : 'Copy'}
      </Button>
    </Tip>
  )
}
