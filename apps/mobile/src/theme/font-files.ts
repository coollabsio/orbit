import { Geist_400Regular } from '@expo-google-fonts/geist/400Regular'
import { Geist_500Medium } from '@expo-google-fonts/geist/500Medium'
import { Geist_600SemiBold } from '@expo-google-fonts/geist/600SemiBold'
import { Geist_700Bold } from '@expo-google-fonts/geist/700Bold'
import { GeistMono_400Regular } from '@expo-google-fonts/geist-mono/400Regular'
import { GeistMono_500Medium } from '@expo-google-fonts/geist-mono/500Medium'
import { GeistMono_600SemiBold } from '@expo-google-fonts/geist-mono/600SemiBold'
import { GeistMono_700Bold } from '@expo-google-fonts/geist-mono/700Bold'
import { IBMPlexSans_400Regular } from '@expo-google-fonts/ibm-plex-sans/400Regular'
import { IBMPlexSans_500Medium } from '@expo-google-fonts/ibm-plex-sans/500Medium'
import { IBMPlexSans_600SemiBold } from '@expo-google-fonts/ibm-plex-sans/600SemiBold'
import { IBMPlexSans_700Bold } from '@expo-google-fonts/ibm-plex-sans/700Bold'
import { Inter_400Regular } from '@expo-google-fonts/inter/400Regular'
import { Inter_500Medium } from '@expo-google-fonts/inter/500Medium'
import { Inter_600SemiBold } from '@expo-google-fonts/inter/600SemiBold'
import { Inter_700Bold } from '@expo-google-fonts/inter/700Bold'
import { Literata_400Regular } from '@expo-google-fonts/literata/400Regular'
import { Literata_500Medium } from '@expo-google-fonts/literata/500Medium'
import { Literata_600SemiBold } from '@expo-google-fonts/literata/600SemiBold'
import { Literata_700Bold } from '@expo-google-fonts/literata/700Bold'
import { loadAsync } from 'expo-font'

import type { FontChoice } from './typography'

/** The files of each font that the root layout does not load (Noto Sans) and that the device does not have (System). */
const FONT_FILES: Partial<Record<FontChoice, Record<string, number>>> = {
  geist: { Geist_400Regular, Geist_500Medium, Geist_600SemiBold, Geist_700Bold },
  inter: { Inter_400Regular, Inter_500Medium, Inter_600SemiBold, Inter_700Bold },
  'ibm-plex-sans': { IBMPlexSans_400Regular, IBMPlexSans_500Medium, IBMPlexSans_600SemiBold, IBMPlexSans_700Bold },
  literata: { Literata_400Regular, Literata_500Medium, Literata_600SemiBold, Literata_700Bold },
  mono: { GeistMono_400Regular, GeistMono_500Medium, GeistMono_600SemiBold, GeistMono_700Bold },
}

/** Loads a font's files, so that text can use it. Loading a font twice does nothing. */
export async function loadFont(choice: FontChoice): Promise<void> {
  const files = FONT_FILES[choice]
  if (files) await loadAsync(files)
}
