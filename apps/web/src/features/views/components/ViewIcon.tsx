import {
  Book,
  Bookmark,
  Box,
  Briefcase,
  Bug,
  Chart,
  Code,
  Cup,
  Flag,
  Flash,
  Folder,
  Heart,
  Hierarchy,
  Lamp,
  Layer,
  Rocket,
  Star,
  Tag,
  Target,
  Timer,
  type IconComponent,
} from 'reicon-react'
import { cn } from 'cn'

/** Stored in `saved_views.icon` by key; unknown or null keys render the Layer glyph. */
export const VIEW_ICONS: Record<string, { icon: IconComponent; label: string }> = {
  layer: { icon: Layer, label: 'Layers' },
  star: { icon: Star, label: 'Star' },
  flag: { icon: Flag, label: 'Flag' },
  bug: { icon: Bug, label: 'Bug' },
  rocket: { icon: Rocket, label: 'Rocket' },
  target: { icon: Target, label: 'Target' },
  flash: { icon: Flash, label: 'Lightning' },
  heart: { icon: Heart, label: 'Heart' },
  bookmark: { icon: Bookmark, label: 'Bookmark' },
  box: { icon: Box, label: 'Box' },
  code: { icon: Code, label: 'Code' },
  folder: { icon: Folder, label: 'Folder' },
  tag: { icon: Tag, label: 'Tag' },
  timer: { icon: Timer, label: 'Timer' },
  chart: { icon: Chart, label: 'Chart' },
  lamp: { icon: Lamp, label: 'Idea' },
  cup: { icon: Cup, label: 'Cup' },
  briefcase: { icon: Briefcase, label: 'Briefcase' },
  book: { icon: Book, label: 'Book' },
  hierarchy: { icon: Hierarchy, label: 'Hierarchy' },
}
export const VIEW_ICON_NAMES = Object.keys(VIEW_ICONS)

/** Pink (the app's primary) first; the rest match the status palette's calmer hues. */
export const VIEW_COLORS: Array<{ value: string; name: string }> = [
  { value: '#e0457b', name: 'Pink' },
  { value: '#eb5757', name: 'Red' },
  { value: '#f2994a', name: 'Orange' },
  { value: '#f2c94c', name: 'Yellow' },
  { value: '#4cb782', name: 'Green' },
  { value: '#26b5ce', name: 'Teal' },
  { value: '#5e6ad2', name: 'Blue' },
  { value: '#8b8f98', name: 'Gray' },
]

export function ViewIcon({ icon, color, className }: { icon: string | null | undefined; color: string | null | undefined; className?: string }) {
  const Icon = (icon ? VIEW_ICONS[icon]?.icon : undefined) ?? Layer
  return <Icon aria-hidden="true" className={cn('size-4 shrink-0', !color && 'text-muted-foreground', className)} style={color ? { color } : undefined} />
}
