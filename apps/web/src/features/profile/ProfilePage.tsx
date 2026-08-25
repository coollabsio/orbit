import { useRef, useState } from 'react'
import { Eye, EyeSlash, Key } from 'reicon-react'
import { EmptyState } from '../../components/ui/EmptyState'
import { Modal } from '../../components/ui/Modal'
import { updateUserProfile } from '../../mock/actions'
import { useAppState } from '../../mock/store'
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

function recoveryCodes() {
  return Array.from({ length: 8 }, () =>
    Array.from({ length: 2 }, () => Math.random().toString(36).slice(2, 7)).join('-'),
  )
}

/** Coolify `livewire/profile/index`: picture, details, password, two-factor. */
export function ProfilePage() {
  const state = useAppState()
  const me = state.users.find((u) => u.id === state.currentUserId)

  const fileRef = useRef<HTMLInputElement>(null)
  const [avatarUrl, setAvatarUrl] = useState<string | null>(null)
  const [avatarError, setAvatarError] = useState<string | null>(null)

  const [name, setName] = useState(me?.name ?? '')
  const [emailModalOpen, setEmailModalOpen] = useState(false)
  const [newEmail, setNewEmail] = useState('')

  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [passwordError, setPasswordError] = useState<string | null>(null)

  const [twoFactorEnabled, setTwoFactorEnabled] = useState(false)
  const [codes, setCodes] = useState<string[] | null>(null)

  const initial = (me?.name || me?.email || 'A').charAt(0).toUpperCase()
  const nameDirty = name.trim() !== (me?.name ?? '')

  const pickAvatar = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return
    if (!['image/jpeg', 'image/png', 'image/webp'].includes(file.type)) {
      setAvatarError('The image could not be processed in this browser.')
      return
    }
    setAvatarError(null)
    if (avatarUrl) URL.revokeObjectURL(avatarUrl)
    setAvatarUrl(URL.createObjectURL(file))
    e.target.value = ''
  }

  const removeAvatar = () => {
    if (avatarUrl) URL.revokeObjectURL(avatarUrl)
    setAvatarUrl(null)
  }

  const saveDetails = (e: React.FormEvent) => {
    e.preventDefault()
    if (!me || !name.trim()) return
    updateUserProfile(me.id, { name: name.trim() })
  }

  const submitEmail = (e: React.FormEvent) => {
    e.preventDefault()
    const trimmed = newEmail.trim()
    if (!me || !trimmed) return
    updateUserProfile(me.id, { email: trimmed })
    setNewEmail('')
    setEmailModalOpen(false)
  }

  const changePassword = (e: React.FormEvent) => {
    e.preventDefault()
    if (newPassword !== confirmPassword) {
      setPasswordError('The new password confirmation does not match.')
      return
    }
    setPasswordError(null)
    setCurrentPassword('')
    setNewPassword('')
    setConfirmPassword('')
  }

  return (
    <div className="page">
      <div className="pane" style={{ flex: 1 }}>
        <div className="pane-header">
          <span className="pane-title">Profile</span>
        </div>
        <div className="settings-scroll">
          <div className="profile-workspace">
            <SettingsCard title="Profile picture" description="Upload a JPG, PNG, or WebP image.">
              <div className="profile-picture">
                <div className="profile-picture-avatar">
                  {avatarUrl ? <img src={avatarUrl} alt={me?.name} /> : <span>{initial}</span>}
                </div>
                <div className="profile-picture-actions">
                  <div className="profile-picture-buttons">
                    <input
                      ref={fileRef}
                      type="file"
                      accept="image/jpeg,image/png,image/webp"
                      hidden
                      onChange={pickAvatar}
                    />
                    <button type="button" className="button" onClick={() => fileRef.current?.click()}>
                      Browse…
                    </button>
                    {avatarUrl ? (
                      <button type="button" className="button button-danger" onClick={removeAvatar}>
                        Remove
                      </button>
                    ) : null}
                  </div>
                  {avatarError ? <p className="profile-error">{avatarError}</p> : null}
                </div>
              </div>
            </SettingsCard>

            <form onSubmit={saveDetails}>
              <SettingsCard
                title="Profile details"
                description="Your display name and verified sign-in address."
                actions={
                  nameDirty ? (
                    <button type="submit" className="button" disabled={!name.trim()}>
                      Save
                    </button>
                  ) : null
                }
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
                  <div className="settings-field-row">
                    <div className="settings-field">
                      <label className="field-label" htmlFor="profile-email">
                        Email
                      </label>
                      <input id="profile-email" className="input" value={me?.email ?? ''} readOnly />
                    </div>
                    <button
                      type="button"
                      className="button"
                      disabled={emailModalOpen}
                      onClick={() => setEmailModalOpen(true)}
                    >
                      Change
                    </button>
                  </div>
                </div>
              </SettingsCard>
            </form>

            <form onSubmit={changePassword}>
              <SettingsCard
                title="Password"
                description="Changing your password signs out every active session."
                actions={
                  <button type="submit" className="button">
                    Change password
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
                  {passwordError ? (
                    <p className="profile-error col-span-2">{passwordError}</p>
                  ) : null}
                </div>
              </SettingsCard>
            </form>

            <SettingsCard
              title="Two-factor authentication"
              description="Add a time-based one-time password to protect your account."
              actions={
                !twoFactorEnabled ? (
                  <button
                    type="button"
                    className="button"
                    onClick={() => {
                      setTwoFactorEnabled(true)
                      setCodes(recoveryCodes())
                    }}
                  >
                    Configure 2FA
                  </button>
                ) : null
              }
            >
              {twoFactorEnabled ? (
                <div className="profile-2fa">
                  <div className="profile-2fa-actions">
                    <button type="button" className="button" onClick={() => setCodes(recoveryCodes())}>
                      Regenerate recovery codes
                    </button>
                    <button
                      type="button"
                      className="button button-danger"
                      onClick={() => {
                        setTwoFactorEnabled(false)
                        setCodes(null)
                      }}
                    >
                      Disable 2FA
                    </button>
                  </div>
                  {codes ? (
                    <div className="profile-recovery-codes">
                      {codes.map((code) => (
                        <div key={code}>{code}</div>
                      ))}
                    </div>
                  ) : null}
                </div>
              ) : (
                <EmptyState
                  size="sm"
                  icon={Key}
                  title="Two-factor authentication is off"
                  description="Configure an authenticator app to add another sign-in check."
                />
              )}
            </SettingsCard>
          </div>
        </div>
      </div>

      {emailModalOpen ? (
        <Modal
          title="Change email"
          description="A six-digit verification code will be sent to the new address."
          maxWidth={576}
          onClose={() => setEmailModalOpen(false)}
        >
          <form onSubmit={submitEmail} style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
            <div className="settings-field">
              <label className="field-label" htmlFor="new-email">
                New email address <span className="field-required">*</span>
              </label>
              <input
                id="new-email"
                type="email"
                className="input"
                required
                value={newEmail}
                onChange={(e) => setNewEmail(e.target.value)}
              />
            </div>
            <div className="modal-footer" style={{ marginTop: 0 }}>
              <button type="submit" className="button button-primary">
                Send code
              </button>
            </div>
          </form>
        </Modal>
      ) : null}
    </div>
  )
}
