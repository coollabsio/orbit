import type { AuthUserResponse, UpdateMeBody } from '@/api/generated/types.gen'

export const PROFILE_LIMITS = { title: 80, pronouns: 40, phone: 32, bio: 500, note: 1000 } as const

/** "3:42 PM local time" in the member's zone; `null` when the zone is missing or not a zone this browser knows. */
export function localTimeLabel(timezone: string | null | undefined, now: Date | number = Date.now(), locale?: string): string | null {
  if (!timezone) return null
  try {
    return `${new Intl.DateTimeFormat(locale, { hour: 'numeric', minute: '2-digit', timeZone: timezone }).format(now)} local time`
  } catch {
    return null
  }
}

/** The IANA zones this browser knows, for the picker. Old browsers have no list: the picker then offers only the current zone. */
export function timeZones(): string[] {
  try {
    return Intl.supportedValuesOf('timeZone')
  } catch {
    return []
  }
}

export function currentTimeZone(): string | null {
  try {
    return new Intl.DateTimeFormat().resolvedOptions().timeZone || null
  } catch {
    return null
  }
}

/** A phone number as people write it: at least one digit, and digits, spaces and `+ - ( ) .` only. The server has the same rule. */
export function phoneValid(phone: string): boolean {
  return /\d/.test(phone) && /^[\d +().-]+$/.test(phone)
}

/** The number for a `tel:` link: the digits, with a leading `+` kept. */
export function phoneHref(phone: string): string {
  return `tel:${phone.trim().startsWith('+') ? '+' : ''}${phone.replace(/\D/g, '')}`
}

/** The text to send for a note, or `null` when it is the saved note. An empty text removes the note. */
export function noteToSave(saved: string | null | undefined, draft: string): string | null {
  const next = draft.trim()
  return next === (saved ?? '').trim() ? null : next
}

export interface ProfileForm {
  name: string
  title: string
  pronouns: string
  phone: string
  timezone: string
  bio: string
}

type ProfileSource = Pick<AuthUserResponse, 'display_name' | 'title' | 'pronouns' | 'phone' | 'timezone' | 'bio'>

export function profileFormOf(user: ProfileSource | null | undefined): ProfileForm {
  return {
    name: user?.display_name ?? '',
    title: user?.title ?? '',
    pronouns: user?.pronouns ?? '',
    phone: user?.phone ?? '',
    timezone: user?.timezone ?? '',
    bio: user?.bio ?? '',
  }
}

/** The form as it is saved: trimmed text. */
function settled(form: ProfileForm): ProfileForm {
  return { name: form.name.trim(), title: form.title.trim(), pronouns: form.pronouns.trim(), phone: form.phone.trim(), timezone: form.timezone, bio: form.bio.trim() }
}

const OPTIONAL_PARTS = ['title', 'pronouns', 'phone', 'timezone', 'bio'] as const

/** The form differs from the saved profile (an emptied name too, though that cannot be saved). */
export function profileDirty(user: ProfileSource | null | undefined, form: ProfileForm): boolean {
  const saved = profileFormOf(user)
  const next = settled(form)
  return next.name !== saved.name || OPTIONAL_PARTS.some((part) => next[part] !== saved[part])
}

/**
 * The request for the changed parts only, or `null` when nothing changed or the name is empty. The name always goes
 * along (the endpoint requires it); an optional part that was emptied goes as `''`, which removes it.
 */
export function profileChanges(user: ProfileSource | null | undefined, form: ProfileForm): UpdateMeBody | null {
  const saved = profileFormOf(user)
  const next = settled(form)
  if (!next.name || !profileDirty(user, form)) return null
  const body: UpdateMeBody = { display_name: next.name }
  for (const part of OPTIONAL_PARTS) {
    if (next[part] !== saved[part]) body[part] = next[part]
  }
  return body
}
