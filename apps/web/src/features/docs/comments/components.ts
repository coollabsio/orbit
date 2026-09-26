// BlockNote's shadcn components for the comments UI, with two changes: comment editors get the "@" mention picker,
// and icon-only toolbar buttons get their tooltip as accessible name.
import { components as shadcnComponents } from '@blocknote/shadcn'
import { CommentEditor } from './CommentEditor'
import { LabelledToolbarButton } from './LabelledToolbarButton'

export const commentComponents = {
  ...shadcnComponents,
  Generic: {
    ...shadcnComponents.Generic,
    Toolbar: { ...shadcnComponents.Generic.Toolbar, Button: LabelledToolbarButton },
  },
  Comments: { ...shadcnComponents.Comments, Editor: CommentEditor },
}
