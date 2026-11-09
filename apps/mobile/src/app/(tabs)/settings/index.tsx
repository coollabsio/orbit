import { SettingsMenu, WORKSPACE_NAV } from '../../../components/settings/Shell'

/** The dock's Settings: the workspace pages as a menu, like the web on a phone. */
export default function SettingsMenuScreen() {
  return <SettingsMenu title="Settings" area="workspace" nav={WORKSPACE_NAV} />
}
