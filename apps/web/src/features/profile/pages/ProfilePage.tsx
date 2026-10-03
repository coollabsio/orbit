import { useEffect, useRef, useState } from 'react'
import { Eye, EyeSlash as EyeOff } from 'reicon-react'
import { ApiProblem } from '@/api/problem'
import { UnsavedBar } from '@/components/common/UnsavedBar'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '@/components/ui/input-group'
import { Button } from '@/components/ui/button'
import { useChangePassword, useCurrentUser, useSetAvatar, useUpdateProfile } from '@/features/auth/api'
import { userColor } from '@/features/workspaces/api'
import { UserAvatar } from '@/components/common/UserAvatar'
import { avatarImage } from '@/features/profile/avatarImage'
import { SettingsCard } from '@/components/common/SettingsCard'
import { Pane, PaneHeader, PaneTitle } from '@/components/common/Pane'
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

/** Account settings: display name and authenticated password change. */
export function ProfilePage() {
  const user = useCurrentUser()
  const me = user.data
  const updateProfile = useUpdateProfile()
  const changePasswordMutation = useChangePassword()

  const formRef = useRef<HTMLFormElement>(null)
  const [name, setName] = useState(me?.display_name ?? '')
  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [passwordError, setPasswordError] = useState<string | null>(null)

  useEffect(() => {
    if (me?.display_name != null) setName(me.display_name)
  }, [me?.display_name])

  const nameDirty = name.trim() !== (me?.display_name ?? '')

  const saveDetails = (e: React.FormEvent) => {
    e.preventDefault()
    const displayName = name.trim()
    if (!displayName || !nameDirty || updateProfile.isPending) return
    updateProfile.mutate({ display_name: displayName })
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

  return (
    <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      <Pane className="relative">
        <PaneHeader>
          <PaneTitle>Account settings</PaneTitle>
        </PaneHeader>
        <div className="min-h-0 flex-1 overflow-y-auto">
          <div className="flex w-full min-w-0 flex-col gap-6 px-5 pt-5 pb-8 min-[900px]:px-10 min-[900px]:pt-7 min-[900px]:pb-10">
            <form ref={formRef} onSubmit={saveDetails}>
              <SettingsCard
                title="Profile details"
                description="Your picture, display name and verified sign-in address."
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
                      value={name}
                      onChange={(e) => setName(e.target.value)}
                    />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor="profile-email">
                      Email
                    </FieldLabel>
                    <Input id="profile-email" value={me?.email ?? ''} readOnly />
                  </Field>
                </FieldGrid>
                {updateProfile.isError ? (
                  <p className="text-xs text-destructive" role="alert">
                    {errorDetail(updateProfile.error, 'Display name could not be saved.')}
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
          </div>
        </div>
        {nameDirty ? (
          <UnsavedBar
            onReset={() => {
              if (updateProfile.isPending) return
              setName(me?.display_name ?? '')
              updateProfile.reset()
            }}
            onSave={() => formRef.current?.requestSubmit()}
            saving={updateProfile.isPending}
          />
        ) : null}
      </Pane>
    </div>
  )
}
