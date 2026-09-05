export function MockFeatureBadge() {
  if (!import.meta.env.DEV) return null

  return (
    <span className="mock-feature-badge" title="This screen still uses local development data.">
      Mock data
    </span>
  )
}
