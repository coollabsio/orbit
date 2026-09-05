export function dueDateInputValue(value: string | null): string {
  if (!value) return ''
  const date = new Date(value)
  const local = new Date(date.getTime() - date.getTimezoneOffset() * 60_000)
  return local.toISOString().slice(0, 16)
}

export function dueDatePatchValue(value: string): string | null {
  return value ? new Date(value).toISOString() : null
}
