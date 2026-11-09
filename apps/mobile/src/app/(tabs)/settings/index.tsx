import { ACCOUNT_NAV, SettingsMenu, WORKSPACE_NAV } from '../../../components/settings/Shell'
import { useIsGuest } from '../../../orbit-context'

/** The dock's Settings: the workspace pages as a menu, like the web on a phone. A guest has no workspace settings, so it gets its account menu. */
export default function SettingsMenuScreen() {
  return useIsGuest() ? <SettingsMenu title="Account settings" area="account" nav={ACCOUNT_NAV} /> : <SettingsMenu title="Settings" area="workspace" nav={WORKSPACE_NAV} />
}
