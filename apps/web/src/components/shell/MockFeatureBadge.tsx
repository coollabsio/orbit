export function MockFeatureBadge() {
  if (!import.meta.env.DEV || import.meta.env.VITE_API_MODE === 'server') return null

  return (
    <span className="mock-feature-badge" title="This screen still uses local development data.">
      Mock data
    </span>
  )
}
