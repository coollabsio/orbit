import { expect, test } from 'bun:test'

import { typographyStyle } from './typography'

test('the chosen font replaces each Noto Sans face, and code keeps its own font', () => {
  expect(typographyStyle({ fontFamily: 'NotoSans_600SemiBold', fontSize: 15 }, 'inter', 1)).toEqual({ fontFamily: 'Inter_600SemiBold' })
  expect(typographyStyle({ fontFamily: 'NotoSans_700Bold' }, 'system', 1)).toEqual({ fontFamily: 'System', fontWeight: '700' })
  expect(typographyStyle({ fontFamily: 'Menlo', fontSize: 13 }, 'mono', 1)).toEqual({})
})

test('the text size scales font size and line height', () => {
  expect(typographyStyle({ fontFamily: 'NotoSans_400Regular', fontSize: 16, lineHeight: 20 }, 'noto-sans', 1.25)).toEqual({ fontSize: 20, lineHeight: 25 })
  expect(typographyStyle({ fontFamily: 'NotoSans_400Regular', fontSize: 16 }, 'geist', 0.875)).toEqual({ fontSize: 14, fontFamily: 'Geist_400Regular' })
  // the web preview resolves classes to CSS strings
  expect(typographyStyle({ fontSize: '14px' as unknown as number, lineHeight: '20px' as unknown as number }, 'noto-sans', 1.25)).toEqual({ fontSize: 17.5, lineHeight: 25 })
})
