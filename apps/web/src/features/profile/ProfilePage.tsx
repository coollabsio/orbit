import { useEffect, useRef, useState } from 'react'
import { Eye, EyeOff } from 'lucide-react'
import { ApiProblem } from '../../api/problem'
import { UnsavedBar } from '../../components/ui/UnsavedBar'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { cn } from 'cn'
import { useChangePassword, useCurrentUser, useUpdateProfile } from '../auth/api'
import { SettingsCard } from '../settings/SettingsCard'

const FIELD_LABEL = 'mb-1.5 h-4 gap-1 text-[13px] leading-4 font-medium text-muted-foreground'
const GRID = 'grid grid-cols-1 gap-4 min-[900px]:grid-cols-2'
const FIELD = 'w-full min-w-0'
const REQ = 'inline-block font-semibold text-primary'

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
    <div className={FIELD}>
      <Label className={FIELD_LABEL} htmlFor={id}>
        {label} <span className={REQ}>*</span>
      </Label>
      <div className="relative">
        <Input
          id={id}
          type={visible ? 'text' : 'password'}
          className="pr-10"
          required
          autoComplete={autoComplete}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        />
        <button
          type="button"
          className="absolute inset-y-0 right-0 z-[1] flex items-center pr-2 text-muted-foreground transition-colors hover:text-foreground"
          aria-label="Toggle password visibility"
          onClick={() => setVisible((v) => !v)}
        >
          {visible ? <EyeOff className="size-4.5" /> : <Eye className="size-4.5" />}
        </button>
      </div>
    </div>
  )
}

function errorDetail(error: unknown, fallback: string) {
  return error instanceof ApiProblem ? error.detail : fallback
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
      <div className="relative flex h-full min-h-0 min-w-0 flex-1 flex-col bg-background">
        <div className="flex min-h-12 shrink-0 items-center gap-2 border-b border-border px-3 py-2 max-[899px]:border-b-0">
          <span className="truncate text-[13px] font-semibold text-foreground">Account settings</span>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          <div className="flex w-full min-w-0 flex-col gap-6 px-5 pt-5 pb-8 min-[900px]:px-10 min-[900px]:pt-7 min-[900px]:pb-10">
            <form ref={formRef} onSubmit={saveDetails}>
              <SettingsCard
                title="Profile details"
                description="Your display name and verified sign-in address."
              >
                <div className={GRID}>
                  <div className={FIELD}>
                    <Label className={FIELD_LABEL} htmlFor="profile-name">
                      Name <span className={REQ}>*</span>
                    </Label>
                    <Input
                      id="profile-name"
                      required
                      value={name}
                      onChange={(e) => setName(e.target.value)}
                    />
                  </div>
                  <div className={FIELD}>
                    <Label className={FIELD_LABEL} htmlFor="profile-email">
                      Email
                    </Label>
                    <Input id="profile-email" value={me?.email ?? ''} readOnly />
                  </div>
                </div>
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
                <div className={GRID}>
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
                    <p className={cn('text-xs text-destructive', 'min-[900px]:col-span-2')} role="alert">
                      {passwordAlert}
                    </p>
                  ) : null}
                </div>
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
      </div>
    </div>
  )
}
