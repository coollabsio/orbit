/**
 * Chat is being rebuilt: its routes, navigation and shortcuts exist in development builds only, and production keeps
 * "Coming soon". This is the only place that decides it.
 */
export const chatEnabled = import.meta.env.DEV === true
