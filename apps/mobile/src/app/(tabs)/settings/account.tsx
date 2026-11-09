import { ACCOUNT_NAV, SettingsMenu } from '../../../components/settings/Shell'

/** Account settings from the account menu: its pages as a menu, like the web's `/profile` on a phone. */
export default function AccountMenuScreen() {
  return <SettingsMenu title="Account settings" area="account" nav={ACCOUNT_NAV} />
}
