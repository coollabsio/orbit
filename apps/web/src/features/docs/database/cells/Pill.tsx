import type { ComponentProps } from 'react'
import { cva } from 'class-variance-authority'
import { cn } from 'cn'
import type { OptionColor } from '@orbit/database/types'

const pillVariants = cva('inline-flex h-5 min-w-0 shrink-0 items-center gap-1.5 rounded-[4px] px-1.5 text-[0.8125rem] leading-5 whitespace-nowrap', {
  variants: {
    color: {
      default: 'bg-foreground/[0.07] text-foreground',
      gray: 'bg-neutral-500/15 text-neutral-700 dark:text-neutral-300',
      brown: 'bg-amber-900/15 text-amber-900 dark:bg-amber-700/20 dark:text-amber-200',
      orange: 'bg-orange-500/15 text-orange-700 dark:text-orange-300',
      yellow: 'bg-yellow-500/20 text-yellow-800 dark:text-yellow-200',
      green: 'bg-green-500/15 text-green-700 dark:text-green-300',
      blue: 'bg-blue-500/15 text-blue-700 dark:text-blue-300',
      purple: 'bg-purple-500/15 text-purple-700 dark:text-purple-300',
      pink: 'bg-pink-500/15 text-pink-700 dark:text-pink-300',
      red: 'bg-red-500/15 text-red-700 dark:text-red-300',
    },
  },
  defaultVariants: { color: 'default' },
})

/** A select, multi-select or status option. A status pill has a dot of its color. */
export function Pill({ color, dot = false, className, children, ...props }: Omit<ComponentProps<'span'>, 'color'> & { color: OptionColor; dot?: boolean }) {
  return (
    <span data-slot="option-pill" data-color={color} className={cn(pillVariants({ color }), dot && 'rounded-full pl-1.5', className)} {...props}>
      {dot ? <span aria-hidden="true" className="size-2 shrink-0 rounded-full bg-current opacity-70" /> : null}
      <span className="truncate">{children}</span>
    </span>
  )
}
