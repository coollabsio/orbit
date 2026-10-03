import { useRef, useState } from 'react'
import { Eye, EyeSlash as EyeOff } from 'reicon-react'
import { ApiProblem } from '@/api/problem'
import { UnsavedBar } from '@/components/common/UnsavedBar'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group'
import { Button } from '@/components/ui/button'
import { useChangePassword, useCurrentUser, useSetAvatar, useUpdateProfile } from '@/features/auth/api'
import { userColor } from '@/features/workspaces/api'
import { UserAvatar } from '@/components/common/UserAvatar'
import { avatarImage } from '@/features/profile/avatarImage'
import { TimeZonePicker } from '@/features/profile/components/TimeZonePicker'
import { PROFILE_LIMITS, phoneValid, profileChanges, profileDirty, profileFormOf, type ProfileForm } from '@/features/profile/profileLib'
import { SettingsCard } from '@/components/common/SettingsCard'
import { FieldGrid, RequiredMark } from '@/features/settings/components/SettingsParts'


function PasswordInput({
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
  const [visible, setVisible] = useState(false)
  return (
    <Field>
      <FieldLabel htmlFor={id}>
        {label} <RequiredMark />
      </FieldLabel>
      <InputGroup>
        <InputGroupInput
          id={id}
          type={visible ? 'text' : 'password'}
          required
          autoComplete={autoComplete}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        />
        <InputGroupAddon align="inline-end">
          <InputGroupButton
            size="icon-xs"
            className="text-muted-foreground"
            aria-label="Toggle password visibility"
            onClick={() => setVisible((v) => !v)}
          >
            {visible ? <EyeOff /> : <Eye />}
          </InputGroupButton>
        </InputGroupAddon>
      </InputGroup>
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

/** Account settings: the profile other members see, and authenticated password change. */
export function ProfilePage() {
  const user = useCurrentUser()
  const me = user.data
  const updateProfile = useUpdateProfile()
  const changePasswordMutation = useChangePassword()

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
      { current_password: currentPassword, new_password: newPassword },
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
                    <Input id="profile-email" value={me?.email ?? ''} readOnly />
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
                title="Password"
                description="Other devices will be signed out."
                actions={
                  <Button type="submit" variant="outline" disabled={changePasswordMutation.isPending}>
                    {changePasswordMutation.isPending ? 'Changing…' : 'Change password'}
                  </Button>
                }
              >
                <FieldGrid>
                  <div className="min-[900px]:col-span-2">
                    <PasswordInput
                      id="current-password"
                      label="Current password"
                      autoComplete="current-password"
                      value={currentPassword}
                      onChange={setCurrentPassword}
                    />
                  </div>
                  <PasswordInput
                    id="new-password"
                    label="New password"
                    autoComplete="new-password"
                    value={newPassword}
                    onChange={setNewPassword}
                  />
                  <PasswordInput
                    id="confirm-password"
                    label="Confirm new password"
                    autoComplete="new-password"
                    value={confirmPassword}
                    onChange={setConfirmPassword}
                  />
                  {passwordAlert ? (
                    <p className="text-xs text-destructive min-[900px]:col-span-2" role="alert">
                      {passwordAlert}
                    </p>
                  ) : null}
                </FieldGrid>
              </SettingsCard>
            </form>
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
