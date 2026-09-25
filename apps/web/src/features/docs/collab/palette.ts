/**
 * Cursor / presence colors, picked by user id. Mirrors `USER_COLORS` + `user_color` in
 * `apps/server/src/collab/hub.rs` exactly: the server stamps this color into every peer's awareness state, and the
 * header avatars must show the same color as the caret. Tailwind 600 shades: readable on the light and dark canvas,
 * and BlockNote's caret label picks white text for all of them.
 */
export const COLLAB_COLORS = ['#e11d48', '#db2777', '#9333ea', '#2563eb', '#0891b2', '#059669', '#ca8a04', '#ea580c'] as const

/** `hash = hash * 31 + byte` over the UTF-8 bytes of the id, wrapping at 32 bits (Rust `wrapping_mul`/`wrapping_add`). */
export function collabColor(userId: string): string {
  let hash = 0
  for (const byte of new TextEncoder().encode(userId)) hash = (Math.imul(hash, 31) + byte) >>> 0
  return COLLAB_COLORS[hash % COLLAB_COLORS.length]
}
