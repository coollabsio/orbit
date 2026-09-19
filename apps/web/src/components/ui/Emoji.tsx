import { useAppState } from '../../mock/store'

/** Renders an emoji value; custom shortcodes (":name:") become their uploaded image. */
export function Emoji({ value, size = 16 }: { value: string; size?: number }) {
  const { customEmojis } = useAppState()
  const match = value.match(/^:([a-z0-9_+-]+):$/)
  const custom = match ? customEmojis.find((e) => e.name === match[1]) : undefined
  if (custom) {
    return (
      <img
        className="inline-block rounded-[3px] object-contain align-[-4px]"
        src={custom.url}
        alt={`:${custom.name}:`}
        title={`:${custom.name}:`}
        style={{ width: size, height: size }}
      />
    )
  }
  return <>{value}</>
}
