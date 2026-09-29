import { Star } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { useSetFavorite, type SavedView } from '../api/views'

export interface FavoriteStarProps {
  workspaceId: string
  view: Pick<SavedView, 'id' | 'name' | 'is_favorite'>
  /** Put the view's name in the label, for lists where several stars sit side by side. */
  named?: boolean
  className?: string
}

/** Favorite toggle for a saved view (view header, Views page rows); solid pink while on. */
export function FavoriteStar({ workspaceId, view, named = false, className }: FavoriteStarProps) {
  const setFavorite = useSetFavorite(workspaceId)
  const on = view.is_favorite
  const label = on
    ? named ? `Remove ${view.name} from favorites` : 'Remove from favorites'
    : named ? `Add ${view.name} to favorites` : 'Add to favorites'
  return (
    <Button
      type="button"
      variant="ghost"
      size="icon-xs"
      aria-label={label}
      aria-pressed={on}
      className={cn('text-muted-foreground aria-pressed:text-primary', className)}
      onClick={() => setFavorite.mutate({ viewId: view.id, favorite: !on })}
    >
      <Star weight={on ? 'Filled' : 'Outline'} />
    </Button>
  )
}
