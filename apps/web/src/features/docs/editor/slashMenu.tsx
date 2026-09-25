import { filterSuggestionItems } from '@blocknote/core/extensions'
import { getDefaultReactSlashMenuItems, type DefaultReactSuggestionItem } from '@blocknote/react'
import { DocumentText, Link2 } from 'reicon-react'
import type { PageEditorInstance } from './schema'

/** Group the custom page items join (the reference editor's default "Basic blocks" group). */
export const PAGE_ITEMS_GROUP = 'Basic blocks'

export interface PageSlashActions {
  onSubpage: () => void
  onLinkPage: () => void
}

/** "Sub-page" and "Link a page" — titles, subtexts and aliases ported from the old docs slash menu. */
export function pageSlashItems(actions: PageSlashActions): DefaultReactSuggestionItem[] {
  return [
    {
      title: 'Sub-page',
      subtext: 'Create a page under this one',
      aliases: ['subpage', 'new', 'under', 'page'],
      group: PAGE_ITEMS_GROUP,
      icon: <DocumentText className="size-[18px]" />,
      onItemClick: actions.onSubpage,
    },
    {
      title: 'Link a page',
      subtext: 'Insert a link to an existing page',
      aliases: ['link', 'reference', 'mention'],
      group: PAGE_ITEMS_GROUP,
      icon: <Link2 className="size-[18px]" />,
      onItemClick: actions.onLinkPage,
    },
  ]
}

/**
 * Inserts `custom` at the start of the `group` run inside `defaults`. The suggestion menu prints a group label
 * whenever the group changes between consecutive items, so the custom items must sit next to the existing
 * group members or the label would appear twice. Falls back to prepending when the group is absent.
 */
export function mergeSlashItems<T extends { group?: string }>(defaults: readonly T[], custom: readonly T[], group = PAGE_ITEMS_GROUP): T[] {
  const index = defaults.findIndex((item) => item.group === group)
  if (index === -1) return [...custom, ...defaults]
  return [...defaults.slice(0, index), ...custom, ...defaults.slice(index)]
}

/**
 * BlockNote's filter keeps the original order, so an alias hit can outrank a title hit ("/sub" selected
 * Heading 2 via its "subheading" alias before Sub-page). Puts groups with a title-prefix match first, and those
 * items first inside each group. Groups stay contiguous so each label is printed once.
 */
export function preferTitleMatches<T extends { title: string; group?: string }>(items: readonly T[], query: string): T[] {
  const needle = query.trim().toLowerCase()
  if (!needle) return [...items]
  const titleHit = (item: T) => item.title.toLowerCase().startsWith(needle)
  const groupOrder = [...new Set(items.map((item) => item.group))]
  const hitGroups = new Set(items.filter(titleHit).map((item) => item.group))
  const rank = (item: T) => [hitGroups.has(item.group) ? 0 : 1, groupOrder.indexOf(item.group), titleHit(item) ? 0 : 1]
  return [...items].sort((a, b) => {
    const [ra, rb] = [rank(a), rank(b)]
    return ra[0] - rb[0] || ra[1] - rb[1] || ra[2] - rb[2]
  })
}

/**
 * Slash menu items for the page editor: BlockNote's defaults (already limited to block types present in the
 * schema, so video/audio are gone while Image and File stay) plus the page items, filtered by the typed query.
 */
export function getPageSlashMenuItems(editor: PageEditorInstance, actions: PageSlashActions, query: string): DefaultReactSuggestionItem[] {
  const merged = mergeSlashItems(getDefaultReactSlashMenuItems(editor), pageSlashItems(actions))
  return preferTitleMatches(filterSuggestionItems(merged, query), query)
}
