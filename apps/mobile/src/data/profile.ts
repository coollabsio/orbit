/** The Profile form, as the web saves it (`features/profile/profileLib.ts`). No React. */
import type { UpdateMeBody, User } from './orbit'

export interface ProfileForm {
  name: string
  title: string
  pronouns: string
  phone: string
  timezone: string
  bio: string
}

type ProfileSource = Pick<User, 'display_name' | 'title' | 'pronouns' | 'phone' | 'timezone' | 'bio'>

export function profileFormOf(user: ProfileSource): ProfileForm {
  return { name: user.display_name, title: user.title ?? '', pronouns: user.pronouns ?? '', phone: user.phone ?? '', timezone: user.timezone ?? '', bio: user.bio ?? '' }
}

const settled = (form: ProfileForm): ProfileForm => ({
  name: form.name.trim(),
  title: form.title.trim(),
  pronouns: form.pronouns.trim(),
  phone: form.phone.trim(),
  timezone: form.timezone.trim(),
  bio: form.bio.trim(),
})

const OPTIONAL_PARTS = ['title', 'pronouns', 'phone', 'timezone', 'bio'] as const

/** A digit, and only digits, spaces and `+ - ( ) .`. */
export const phoneValid = (phone: string) => /\d/.test(phone) && /^[\d +().-]+$/.test(phone)

/**
 * The request for the changed parts only, or `null` when nothing changed or the name is empty. The name always goes
 * along (the endpoint requires it); an emptied part goes as `''`, which removes it.
 */
export function profileChanges(user: ProfileSource, form: ProfileForm): UpdateMeBody | null {
  const saved = profileFormOf(user)
  const next = settled(form)
  const changed = OPTIONAL_PARTS.filter((part) => next[part] !== saved[part])
  if (!next.name || (next.name === saved.name && changed.length === 0)) return null
  const body: UpdateMeBody = { display_name: next.name }
  for (const part of changed) body[part] = next[part]
  return body
}
