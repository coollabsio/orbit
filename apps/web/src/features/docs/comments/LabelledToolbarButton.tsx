import type { ComponentProps } from 'react'
import { components as shadcnComponents } from '@blocknote/shadcn'

const ToolbarButton = shadcnComponents.Generic.Toolbar.Button

/** BlockNote's toolbar button; icon-only ones (Resolve, Reopen, More actions) get their tooltip as accessible name. */
export function LabelledToolbarButton(props: ComponentProps<typeof ToolbarButton>) {
  const label = props.label ?? (typeof props.mainTooltip === 'string' ? props.mainTooltip : undefined)
  return <ToolbarButton {...({ ...props, label } as ComponentProps<typeof ToolbarButton>)} />
}
