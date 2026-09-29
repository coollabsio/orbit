import type { ComponentProps } from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'

const colorSwatchVariants = cva(
  'relative shrink-0 rounded-full border-0 p-0 text-white hover:text-white aria-pressed:outline-2 aria-pressed:outline-offset-2 aria-pressed:outline-solid aria-pressed:outline-foreground/60',
  {
    variants: {
      size: {
        default: 'size-7',
        sm: 'size-5',
        xs: 'size-4.5',
      },
    },
    defaultVariants: { size: 'default' },
  },
)

/**
 * A preset colour as a round, pressable swatch; `aria-pressed` marks the chosen one. The colour is runtime data, so it
 * stays inline. `color={null}` is the dashed "no colour" swatch.
 */
function ColorSwatch({ color, size, className, style, ...props }: Omit<ComponentProps<typeof Button>, 'size' | 'color'> & VariantProps<typeof colorSwatchVariants> & { color: string | null }) {
  return (
    <Button
      type="button"
      size="icon"
      className={cn(colorSwatchVariants({ size }), color === null && 'border border-dashed border-muted-foreground bg-transparent hover:bg-transparent', className)}
      style={color === null ? style : { ...style, backgroundColor: color }}
      {...props}
    />
  )
}

/** The rainbow swatch that opens the native colour picker for a custom colour. */
function CustomColorSwatch({ value, onChange }: { value: string; onChange: (color: string) => void }) {
  return (
    <label
      data-slot="custom-color-swatch"
      title="Custom color"
      className="relative inline-flex size-7 shrink-0 cursor-pointer rounded-full bg-[conic-gradient(#eb5757,#f2c94c,#4cb782,#26b5ce,#5e6ad2,#a78bfa,#eb5757)]"
    >
      {/* the native colour picker, invisible over the swatch */}
      <input type="color" value={value} aria-label="Custom color" className="absolute inset-0 cursor-pointer opacity-0" onChange={(event) => onChange(event.target.value)} />
    </label>
  )
}

export { ColorSwatch, CustomColorSwatch }
