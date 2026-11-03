import { createContext, type CSSProperties } from 'react'

/** How the menus opened inside an area look, e.g. the app sidebar and its custom theme. Menus are portalled out, so they read it here. */
export interface MenuSurface {
  /** On the panel, after the caller's classes. */
  className: string
  /** On the wrapper outside the panel: a colour scheme class and theme variables the panel inherits. */
  positionerClassName?: string
  positionerStyle?: CSSProperties
}

export const MenuSurfaceContext = createContext<MenuSurface | null>(null)
