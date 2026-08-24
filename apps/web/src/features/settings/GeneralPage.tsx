import { Listbox } from '../../components/ui/Listbox'
import { useTheme, type Theme } from '../../lib/themeContext'
import { SettingsCard } from './SettingsCard'

export function GeneralPage() {
  const { theme, setTheme } = useTheme()

  return (
    <>
      <SettingsCard title="Appearance" description="Theme for this browser.">
        <div className="settings-grid">
          <div className="settings-field">
            <label className="field-label" htmlFor="appearance-theme">
              Theme
            </label>
            <Listbox<Theme>
              id="appearance-theme"
              value={theme}
              options={[
                { value: 'light', label: 'Light' },
                { value: 'dark', label: 'Dark' },
              ]}
              onChange={setTheme}
            />
          </div>
        </div>
      </SettingsCard>

      <SettingsCard title="About" description="Version and backend status of this Orbit instance.">
        <div className="settings-grid">
          <div className="settings-field">
            <label className="field-label" htmlFor="about-version">
              Version
            </label>
            <input id="about-version" className="input" value="0.1.0 (mock)" readOnly />
          </div>
          <div className="settings-field">
            <label className="field-label" htmlFor="about-backend">
              Backend
            </label>
            <input
              id="about-backend"
              className="input"
              value="Not connected — using mock data"
              readOnly
            />
          </div>
          <div className="settings-field">
            <label className="field-label" htmlFor="about-storage">
              Storage
            </label>
            <input id="about-storage" className="input" value="Local browser session" readOnly />
          </div>
        </div>
      </SettingsCard>
    </>
  )
}
