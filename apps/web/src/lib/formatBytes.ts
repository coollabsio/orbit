/** `1.2 MB`, `340 KB`: decimal units, as storage limits are. */
export function formatBytes(bytes: number): string {
  if (bytes >= 1_000_000_000) return `${Number((bytes / 1_000_000_000).toFixed(1))} GB`
  if (bytes >= 1_000_000) return `${Number((bytes / 1_000_000).toFixed(1))} MB`
  if (bytes >= 1000) return `${Math.round(bytes / 1000)} KB`
  return `${bytes} B`
}
