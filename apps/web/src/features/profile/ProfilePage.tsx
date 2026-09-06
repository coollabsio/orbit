import { useEffect, useRef, useState } from 'react'
import { Eye, EyeSlash } from 'reicon-react'
import { ApiProblem } from '../../api/problem'
import { UnsavedBar } from '../../components/ui/UnsavedBar'
import { useChangePassword, useCurrentUser, useUpdateProfile } from '../auth/api'
import { SettingsCard } from '../settings/SettingsCard'
import '../shared/cards.css'
import '../settings/settings.css'
import './profile.css'

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
    <div className="settings-field">
      <label className="field-label" htmlFor={id}>
        {label} <span className="field-required">*</span>
      </label>
      <div className="input-group">
        <input
          id={id}
          type={visible ? 'text' : 'password'}
          className="input"
          required
          autoComplete={autoComplete}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        />
        <button
          type="button"
          className="input-group-toggle"
          aria-label="Toggle password visibility"
          onClick={() => setVisible((v) => !v)}
        >
          {visible ? <EyeSlash size={18} /> : <Eye size={18} />}
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
    <div className="page">
      <div className="pane" style={{ flex: 1, position: 'relative' }}>
        <div className="pane-header">
          <span className="pane-title">Account settings</span>
        </div>
        <div className="settings-scroll">
          <div className="profile-workspace">
            <form ref={formRef} onSubmit={saveDetails}>
              <SettingsCard
                title="Profile details"
                description="Your display name and verified sign-in address."
              >
                <div className="settings-grid">
                  <div className="settings-field">
                    <label className="field-label" htmlFor="profile-name">
                      Name <span className="field-required">*</span>
                    </label>
                    <input
                      id="profile-name"
                      className="input"
                      required
                      value={name}
                      onChange={(e) => setName(e.target.value)}
                    />
                  </div>
                  <div className="settings-field">
                    <label className="field-label" htmlFor="profile-email">
                      Email
                    </label>
                    <input id="profile-email" className="input" value={me?.email ?? ''} readOnly />
                  </div>
                </div>
                {updateProfile.isError ? (
                  <p className="profile-error" role="alert">
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
                  <button type="submit" className="button" disabled={changePasswordMutation.isPending}>
                    {changePasswordMutation.isPending ? 'Changing…' : 'Change password'}
                  </button>
                }
              >
                <div className="settings-grid">
                  <div className="col-span-2">
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
                    <p className="profile-error col-span-2" role="alert">
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
