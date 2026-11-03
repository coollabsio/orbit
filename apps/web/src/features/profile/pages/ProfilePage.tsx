import { useRef, useState } from 'react'
import { ApiProblem } from '@orbit/api/problem'
import { UnsavedBar } from '@/components/common/UnsavedBar'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { PasswordInput } from '@/components/ui/password-input'
import { Button } from '@/components/ui/button'
import { useAuthOptions, useChangePassword, useCurrentUser, useSetAvatar, useUpdateProfile } from '@/features/auth/api'
import { ConfirmWithProvider } from '@/features/auth/components/ConfirmWithProvider'
import { userColor } from '@/features/workspaces/api'
import { UserAvatar } from '@/components/common/UserAvatar'
import { avatarImage } from '@/features/profile/avatarImage'
import { TimeZonePicker } from '@/features/profile/components/TimeZonePicker'
import { DeleteAccountCard } from '@/features/profile/components/DeleteAccountCard'
import { EmailChangeDialog } from '@/features/profile/components/EmailChangeDialog'
import { PROFILE_LIMITS, phoneValid, profileChanges, profileDirty, profileFormOf, type ProfileForm } from '@/features/profile/profileLib'
import { SettingsCard } from '@/components/common/SettingsCard'
import { FieldGrid, RequiredMark } from '@/features/settings/components/SettingsParts'


function PasswordField({
  id,
  label,
  value,
  onChange,
  autoComplete,
}: {
  id: string
  label: string
  value: string
  onChange: (value: string) => void
  autoComplete: string
}) {
  return (
    <Field>
      <FieldLabel htmlFor={id}>
        {label} <RequiredMark />
      </FieldLabel>
      <PasswordInput id={id} required autoComplete={autoComplete} value={value} onChange={(e) => onChange(e.target.value)} />
    </Field>
  )
}

function errorDetail(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
}

/** Profile picture with upload and remove. The picture is cropped and scaled in the browser first. */
function AvatarField() {
  const me = useCurrentUser().data
  const setAvatar = useSetAvatar()
  const inputRef = useRef<HTMLInputElement>(null)
  const [readError, setReadError] = useState<string | null>(null)

  const pick = async (file: File | undefined) => {
    if (!file) return
    setReadError(null)
    setAvatar.reset()
    let image: Blob
    try {
      image = await avatarImage(file)
    } catch {
      setReadError('This image could not be read. Use a PNG, JPEG or WebP file.')
      return
    }
    setAvatar.mutate(image)
  }

  const error = readError ?? (setAvatar.isError ? errorDetail(setAvatar.error, 'The picture could not be saved.') : null)
  return (
    <div className="flex flex-wrap items-center gap-4">
      <UserAvatar
        user={me ? { name: me.display_name, color: userColor(me.id), avatarUrl: me.avatar_url } : null}
        size={64}
      />
      <div className="flex min-w-0 flex-col gap-2">
        <div className="flex flex-wrap gap-2">
          <Button type="button" variant="outline" disabled={setAvatar.isPending} onClick={() => inputRef.current?.click()}>
            {setAvatar.isPending ? 'Saving…' : me?.avatar_url ? 'Change picture' : 'Upload picture'}
          </Button>
          {me?.avatar_url ? (
            <Button type="button" variant="ghost" disabled={setAvatar.isPending} onClick={() => setAvatar.mutate(null)}>
              Remove
            </Button>
          ) : null}
        </div>
        {error ? (
          <p className="text-xs text-destructive" role="alert">{error}</p>
        ) : (
          <p className="text-xs text-muted-foreground">Cropped to a square. People in your workspaces see it.</p>
        )}
      </div>
      <input
        ref={inputRef}
        type="file"
        accept="image/png,image/jpeg,image/webp"
        className="hidden"
        aria-label="Profile picture"
        onChange={(event) => {
          void pick(event.target.files?.[0])
          event.target.value = ''
        }}
      />
    </div>
  )
}

/** The bio counter shows from this many characters before the limit. */
const BIO_COUNTER_FROM = 100

/** Account settings: the profile other members see, the email, the password, and deleting the account. */
export function ProfilePage() {
  const user = useCurrentUser()
  const me = user.data
  const updateProfile = useUpdateProfile()
  const changePasswordMutation = useChangePassword()
  // Accounts created with Google or GitHub set their first password without a current one.
  const hasPassword = me?.has_password !== false

  const formRef = useRef<HTMLFormElement>(null)
  const saved = profileFormOf(me)
  const savedKey = JSON.stringify(saved)
  const [form, setForm] = useState(saved)
  // the saved profile changed (loaded, saved, or edited in another tab): the form follows it
  const [shownKey, setShownKey] = useState(savedKey)
  if (shownKey !== savedKey) {
    setShownKey(savedKey)
    setForm(saved)
  }
  const set = (part: Partial<ProfileForm>) => setForm((current) => ({ ...current, ...part }))
  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [passwordError, setPasswordError] = useState<string | null>(null)
  const options = useAuthOptions()
  // The new address confirms the change with an emailed link.
  const emailEnabled = options.data?.email_enabled === true
  const [changingEmail, setChangingEmail] = useState(false)

  const dirty = profileDirty(me, form)
  const bioLeft = PROFILE_LIMITS.bio - form.bio.length
  const phoneInvalid = form.phone.trim() !== '' && !phoneValid(form.phone.trim())

  const saveDetails = (e: React.FormEvent) => {
    e.preventDefault()
    const changes = profileChanges(me, form)
    if (!changes || updateProfile.isPending || phoneInvalid) return
    updateProfile.mutate(changes)
  }

  const submitPassword = (e: React.FormEvent) => {
    e.preventDefault()
    if (newPassword !== confirmPassword) {
      setPasswordError('The new password confirmation does not match.')
      return
    }
    setPasswordError(null)
    changePasswordMutation.mutate(
      { current_password: hasPassword ? currentPassword : null, new_password: newPassword },
      {
        onSuccess: () => {
          setCurrentPassword('')
          setNewPassword('')
          setConfirmPassword('')
        },
      },
    )
  }

  const passwordAlert = passwordError
    ?? (changePasswordMutation.isError
      ? errorDetail(changePasswordMutation.error, 'Password could not be changed.')
      : null)
  const reauthenticate = changePasswordMutation.error instanceof ApiProblem && changePasswordMutation.error.code === 'reauthentication_required'

  // Shown in `AccountLayout`, which has the pane, the header and the scrolling column.
  return (
    <>
            <form ref={formRef} onSubmit={saveDetails}>
              <SettingsCard
                title="Profile details"
                description="Your picture, name and what the people in your workspaces see of you."
              >
                <AvatarField />
                <FieldGrid className="mt-4">
                  <Field>
                    <FieldLabel htmlFor="profile-name">
                      Name <RequiredMark />
                    </FieldLabel>
                    <Input
                      id="profile-name"
                      required
                      value={form.name}
                      onChange={(e) => set({ name: e.target.value })}
                    />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor="profile-email">
                      Email
                    </FieldLabel>
                    <div className="flex gap-2">
                      <Input id="profile-email" value={me?.email ?? ''} readOnly />
                      {emailEnabled ? <Button type="button" variant="outline" onClick={() => setChangingEmail(true)}>Change</Button> : null}
                    </div>
                    {options.data && !emailEnabled ? <FieldDescription>This Orbit cannot send email, so the email cannot be changed. Ask an administrator.</FieldDescription> : null}
                  </Field>
                  <Field>
                    <FieldLabel htmlFor="profile-title">Title</FieldLabel>
                    <Input id="profile-title" maxLength={PROFILE_LIMITS.title} placeholder="Product designer" value={form.title} onChange={(e) => set({ title: e.target.value })} />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor="profile-pronouns">Pronouns</FieldLabel>
                    <Input id="profile-pronouns" maxLength={PROFILE_LIMITS.pronouns} placeholder="she/her" value={form.pronouns} onChange={(e) => set({ pronouns: e.target.value })} />
                  </Field>
                  <Field data-invalid={phoneInvalid || undefined}>
                    <FieldLabel htmlFor="profile-phone">Phone number</FieldLabel>
                    <Input
                      id="profile-phone"
                      type="tel"
                      autoComplete="tel"
                      maxLength={PROFILE_LIMITS.phone}
                      placeholder="+1 555 010 0000"
                      aria-invalid={phoneInvalid || undefined}
                      value={form.phone}
                      onChange={(e) => set({ phone: e.target.value })}
                    />
                    <FieldDescription>
                      {phoneInvalid ? 'Use digits, spaces and + - ( ) . only.' : 'The people in your workspaces can see it.'}
                    </FieldDescription>
                  </Field>
                  <Field>
                    <FieldLabel htmlFor="profile-timezone">Time zone</FieldLabel>
                    <TimeZonePicker id="profile-timezone" value={form.timezone} onChange={(timezone) => set({ timezone })} />
                  </Field>
                  <Field className="min-[900px]:col-span-2">
                    <FieldLabel htmlFor="profile-bio">Bio</FieldLabel>
                    <Textarea id="profile-bio" maxLength={PROFILE_LIMITS.bio} rows={3} value={form.bio} onChange={(e) => set({ bio: e.target.value })} aria-describedby={bioLeft <= BIO_COUNTER_FROM ? 'profile-bio-count' : undefined} />
                    {bioLeft <= BIO_COUNTER_FROM ? (
                      <FieldDescription id="profile-bio-count" className="text-right text-xs tabular-nums">
                        {form.bio.length} / {PROFILE_LIMITS.bio}
                      </FieldDescription>
                    ) : null}
                  </Field>
                </FieldGrid>
                {updateProfile.isError ? (
                  <p className="text-xs text-destructive" role="alert">
                    {errorDetail(updateProfile.error, 'Your profile could not be saved.')}
                  </p>
                ) : null}
              </SettingsCard>
            </form>

            <form onSubmit={submitPassword}>
              <SettingsCard
                title={hasPassword ? 'Password' : 'Set a password'}
                description={hasPassword ? 'Other devices will be signed out.' : 'Sign in with your email and a password as well as your connected account.'}
                actions={
                  <Button type="submit" variant="outline" disabled={changePasswordMutation.isPending}>
                    {changePasswordMutation.isPending ? 'Saving…' : hasPassword ? 'Change password' : 'Set password'}
                  </Button>
                }
              >
                <FieldGrid>
                  {hasPassword ? (
                    <div className="min-[900px]:col-span-2">
                      <PasswordField
                        id="current-password"
                        label="Current password"
                        autoComplete="current-password"
                        value={currentPassword}
                        onChange={setCurrentPassword}
                      />
                    </div>
                  ) : null}
                  <PasswordField
                    id="new-password"
                    label="New password"
                    autoComplete="new-password"
                    value={newPassword}
                    onChange={setNewPassword}
                  />
                  <PasswordField
                    id="confirm-password"
                    label="Confirm new password"
                    autoComplete="new-password"
                    value={confirmPassword}
                    onChange={setConfirmPassword}
                  />
                  {reauthenticate ? (
                    <div className="min-[900px]:col-span-2"><ConfirmWithProvider returnTo="/profile" /></div>
                  ) : passwordAlert ? (
                    <p className="text-xs text-destructive min-[900px]:col-span-2" role="alert">
                      {passwordAlert}
                    </p>
                  ) : null}
                </FieldGrid>
              </SettingsCard>
            </form>
            <DeleteAccountCard />
            {changingEmail && me ? <EmailChangeDialog currentEmail={me.email} hasPassword={hasPassword} onClose={() => setChangingEmail(false)} /> : null}
        {dirty ? (
          <UnsavedBar
            onReset={() => {
              if (updateProfile.isPending) return
              setForm(saved)
              updateProfile.reset()
            }}
            onSave={() => formRef.current?.requestSubmit()}
            saving={updateProfile.isPending}
          />
        ) : null}
    </>
  )
}
