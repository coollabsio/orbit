// The Android build gets push notifications (FCM) only with the Firebase project's google-services.json. The file is
// not in git, and prebuild fails when app.json names a file that is missing, so it is added only when it is there.
const { existsSync } = require('node:fs')
const { join } = require('node:path')

module.exports = ({ config }) =>
  existsSync(join(__dirname, 'google-services.json'))
    ? { ...config, android: { ...config.android, googleServicesFile: './google-services.json' } }
    : config
