import { useId, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Dialog, DialogClose, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Field, FieldError, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { useCreateCategory, useRenameCategory } from '@/features/chat/api/mutations'
import type { Category } from '@/features/chat/api/types'
import { chatErrorMessage } from './channelLib'

interface CategoryDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The category to rename; without it the dialog creates one. */
  category?: Category
}

/** Creates a shared category or renames one. The form is mounted only while the dialog is open. */
export function CategoryDialog({ open, onOpenChange, category }: CategoryDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{category ? 'Rename category' : 'New category'}</DialogTitle>
        </DialogHeader>
        <CategoryForm category={category} onDone={() => onOpenChange(false)} />
      </DialogContent>
    </Dialog>
  )
}

function CategoryForm({ category, onDone }: { category?: Category; onDone: () => void }) {
  const createCategory = useCreateCategory()
  const renameCategory = useRenameCategory()
  const [name, setName] = useState(category?.name ?? '')
  const nameId = useId()
  const mutation = category ? renameCategory : createCategory
  const trimmed = name.trim()

  const submit = async () => {
    if (!trimmed || mutation.isPending) return
    if (category && trimmed === category.name) return onDone()
    try {
      if (category) await renameCategory.mutateAsync({ categoryId: category.id, name: trimmed })
      else await createCategory.mutateAsync(trimmed)
      onDone()
    } catch {
      // The form stays; the error shows under the name.
    }
  }

  return (
    <form
      className="grid gap-4"
      onSubmit={(event) => {
        event.preventDefault()
        void submit()
      }}
    >
      <Field data-invalid={mutation.isError || undefined}>
        <FieldLabel htmlFor={nameId}>Name</FieldLabel>
        <Input
          id={nameId}
          autoFocus
          required
          autoComplete="off"
          maxLength={60}
          placeholder="Projects"
          value={name}
          aria-invalid={mutation.isError || undefined}
          onChange={(event) => {
            setName(event.target.value)
            if (mutation.isError) mutation.reset()
          }}
        />
        {mutation.isError ? (
          <FieldError>
            {chatErrorMessage(mutation.error, category ? 'The category was not renamed. Try again.' : 'The category was not created. Try again.', {
              conflict: 'A category with this name already exists.',
              forbidden: 'Only workspace owners and admins can change categories.',
              not_found: 'This category no longer exists.',
            })}
          </FieldError>
        ) : null}
      </Field>
      <DialogFooter>
        <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
        <Button type="submit" disabled={!trimmed || mutation.isPending}>
          {category ? 'Rename category' : 'Create category'}
        </Button>
      </DialogFooter>
    </form>
  )
}
