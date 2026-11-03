import { SafeAreaView as NativeSafeAreaView } from 'react-native-safe-area-context'
import { withUniwind } from 'uniwind'

/** `react-native-safe-area-context`'s view with `className`. */
export const SafeAreaView = withUniwind(NativeSafeAreaView)
