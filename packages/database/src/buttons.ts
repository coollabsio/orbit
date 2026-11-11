import type { DatabaseButtonAction } from '@orbit/api/generated/types.gen'

export type { DatabaseButtonAction, DatabaseButtonPress } from '@orbit/api/generated/types.gen'

/** A button property's config with every default filled in (spec P3 17.2). */
export type ButtonConfig = {
  /** Empty shows the property name. */
  label: string
  actions: DatabaseButtonAction[]
  confirm: string | null
}

export const MAX_BUTTON_ACTIONS = 5

/** Placeholders a value can hold, resolved by the server at the press. */
export const BUTTON_ME = 'me'
export const BUTTON_TODAY = 'today'
export const BUTTON_NOW = 'now'
export const BUTTON_THIS_ROW = 'this_row'

/** Reads a stored config; anything malformed falls back to the defaults. */
export function parseButtonConfig(config: Record<string, unknown>): ButtonConfig {
  const actions = Array.isArray(config.actions) ? config.actions.filter(isAction) : []
  return {
    label: typeof config.label === 'string' ? config.label : '',
    actions,
    confirm: typeof config.confirm === 'string' && config.confirm !== '' ? config.confirm : null,
  }
}

function isAction(value: unknown): value is DatabaseButtonAction {
  if (typeof value !== 'object' || value === null) return false
  const action = value as { type?: unknown; url?: unknown; database_id?: unknown }
  switch (action.type) {
    case 'set_values':
      return true
    case 'add_row':
      return typeof action.database_id === 'string'
    case 'open_url':
      return typeof action.url === 'string'
    default:
      return false
  }
}

/** What a button shows: its label, or the property name when it has none. */
export function buttonLabel(property: { name: string; config: ButtonConfig }): string {
  return property.config.label.trim() || property.name
}
