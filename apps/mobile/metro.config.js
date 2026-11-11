const { getDefaultConfig } = require('expo/metro-config')
const { withUniwindConfig } = require('uniwind/metro')

// Pierre Diffs (components/pulls/PullDiff.dom.tsx) loads its languages with import(). When Expo exports DOM
// components with bundle splitting, it puts their shared code in an async "__common" chunk and then cannot find it
// ("Asset not found: _expo/static/js/web/__common-….js"). One bundle for each DOM component avoids that. Every Expo
// command (start, export, the native build's export:embed) loads this file, so the setting holds for all of them.
process.env.EXPO_NO_BUNDLE_SPLITTING ??= '1'

const config = getDefaultConfig(__dirname)

// Uniwind must be the outermost wrapper.
module.exports = withUniwindConfig(config, {
  cssEntryFile: './src/global.css',
  dtsFile: './src/uniwind-types.d.ts',
})
