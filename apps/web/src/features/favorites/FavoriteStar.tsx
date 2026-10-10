import { Star } from 'reicon-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Tip } from '@/components/common/Tip'
import { useFavorites, useSetFavorite, type FavoriteKind } from './api/favorites'

export interface FavoriteStarProps {
  workspaceId: string
  kind: FavoriteKind
  targetId: string
  /** Whether the item is a favorite, when the caller's record says so (a view's `is_favorite`); else the favorites list decides. */
  on?: boolean
  /** The item's name goes in the label, for lists where several stars sit side by side. */
  name?: string
  className?: string
}

/** Favorite toggle for a task, a saved view, a project or a milestone (their headers, Views page rows); solid pink while on. */
export function FavoriteStar({ workspaceId, kind, targetId, on: known, name, className }: FavoriteStarProps) {
  const setFavorite = useSetFavorite(workspaceId)
  const favorites = useFavorites(workspaceId, known === undefined)
  const listed = favorites.data?.some((item) => item.kind === kind && item.target_id === targetId) ?? false
  // while the change is on its way, the star shows what was asked for
  const on = known ?? (setFavorite.isPending ? setFavorite.variables.favorite : listed)
  const label = on
    ? name ? `Remove ${name} from favorites` : 'Remove from favorites'
    : name ? `Add ${name} to favorites` : 'Add to favorites'
  return (
    <Tip label={on ? 'Remove from favorites' : 'Add to favorites'}>
      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        aria-label={label}
        aria-pressed={on}
        className={cn('text-muted-foreground aria-pressed:text-primary', className)}
        onClick={() => setFavorite.mutate({ kind, targetId, favorite: !on })}
      >
        <Star weight={on ? 'Filled' : 'Outline'} />
      </Button>
    </Tip>
  )
}
