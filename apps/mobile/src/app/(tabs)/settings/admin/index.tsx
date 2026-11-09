import { ADMIN_NAV, SettingsMenu } from '../../../../components/settings/Shell'

/** The Admin area: its pages as a menu, like the web's `/admin` on a phone. */
export default function AdminMenuScreen() {
  return <SettingsMenu title="Admin" area="admin" nav={ADMIN_NAV} />
}
