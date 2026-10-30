# Orbit Mobile

The iOS and Android app for Orbit, in `apps/mobile` of the Orbit monorepo. Built with Expo and React Native.

The app signs in to an Orbit server and works in one workspace at a time. It follows the web app's phone layout: the same bottom dock (Home, Tasks, Docs, Chat, Mail as "coming soon", Settings) and the ☰ drawer (workspace switcher, Activity, the task views, Settings).

- Home: greeting, Due this week, Unread in Inbox, Recently updated docs.
- Tasks: project picker, filter, search, status groups; task detail with every property (status, priority, assignees, labels, due date, parent, source), relations, sub-issues, activity and comments with attachments; New task dialog.
- Docs: teamspaces, private pages and favorites; create, rename, icon, cover, comment, trash and restore. The page body is BlockNote in a DOM component (a web view) that co-edits the page live with the web, with a formatting bar above the keyboard.
- Chat: channels and DMs, new channel/DM, browse, unreads, threads, search, members, pins, files, reactions, emoji, attachments and message actions.
- Activity (from the drawer): notifications with All, Unread and Mentions.
- Settings: workspace General and Members, the account pages (Profile, Notifications, Security, Sessions), and Admin for instance admins.

Every action runs in the app; nothing opens the web.

## Code map

- `src/data/`: no React. `orbit.ts` has every API call over the generated SDK (`@orbit/api`), `keys.ts` every query key, and `tasks.ts`, `activity.ts`, `chat.ts`, `docs.ts`, `inbox.ts`, `profile.ts`, `format.ts`, `color.ts`, `markdown.ts`, `live.ts`, `socket.ts` the logic the screens and the live sockets use (`color.ts` and `format.ts` reproduce the web's color mixing and date wording). Unit tests sit next to them.
- `src/components/`: the dock, the drawer, the task row and the New task dialog. `src/ui/`: the web's primitives (header, buttons, inputs, avatar, icons).
- `src/session-context.tsx`: the stored session and one API client per session. A 401 signs out.
- `src/orbit-context.tsx`: `useOrbit()` gives the client, the user and the current workspace (the last one chosen, else the first).
- `src/components/docs/PageEditor.dom.tsx`: the page editor, an Expo DOM component (`'use dom'`) running BlockNote with the web's schema from `@orbit/editor` (`packages/editor`). It connects to the page's collab socket with a one-time ticket (`getTicket` native action), and the screen (`src/app/(tabs)/docs/[id].tsx`) drives it through props, native actions and `ref.run(command)`. `src/data/page-editor.ts` holds the commands, the toolbar state and the answers to close codes.
- `src/live.ts`: the workspace events socket (marks task, project and inbox queries stale) and the chat live socket (patches messages and read states). Both close in the background and reconnect in the foreground.

## How the app talks to Orbit

- The app sends no `Origin` header, so the server treats it as a native client.
- `POST /api/v1/auth/login` (and `/login/two-factor` when 2FA is on) returns `session_token` in the body. No cookie is set.
- The app keeps the token in the device keychain (`expo-secure-store`). It sends the token as `Authorization: Bearer <token>` on every request and on the WebSocket upgrade.
- The token is a normal Orbit session. It shows in Account → Sessions, and you can revoke it there.
- **Continue with Google / GitHub** shows on the sign-in screen for each provider the server's admin set up (`providers` in `GET /api/v1/auth/options`). The app starts the flow (`POST /api/v1/auth/providers/{provider}/start` with a PKCE challenge), opens the provider in the system browser's auth session (`expo-web-browser`), and gets back a one-time `handoff` on its redirect URL. It exchanges the handoff and the PKCE verifier for the session token (`POST /api/v1/auth/providers/exchange`). The token never goes in a URL. Two-factor sign-in still asks for the code after this.
- Security → Connected accounts connects and disconnects providers. An account without a password confirms authenticator changes with a fresh provider sign-in when the server answers `reauthentication_required`.

The server side lives in `apps/server` of the same repo.

## Develop

Requirements: Bun, the Orbit dev server (`just dev` at the repo root), and a phone with **Expo Go**, or an iOS simulator or Android emulator.

1. Install dependencies at the repo root. The web and mobile apps share one Bun workspace and one React version.

   ```bash
   bun install
   ```

2. Set the server the sign-in screen fills in.

   ```bash
   cp apps/mobile/.env.example apps/mobile/.env.local
   ```

   The phone must reach this URL. Use one of these:

   - **Tailscale** (recommended). Put the phone on the same tailnet and use the dev server's `https://<host>.ts.net:<port>` URL. The Vite dev server proxies `/api` and WebSockets to the Rust server.
   - **LAN.** Use `http://<computer-LAN-IP>:8888`. Vite must listen on `0.0.0.0`.

3. Sign in to Expo. iOS Expo Go only opens a project when Expo CLI and Expo Go use the same account. On a remote server, `bunx expo login` opens a browser that calls back to `localhost`, so use `bunx expo login --no-browser`, or put a **personal** access token in `.env.local` as `EXPO_TOKEN` (a robot token does not work). Sign in to the same account in Expo Go.

4. Start Metro.

   ```bash
   just mobile               # Metro on port 8082, advertised as ORBIT_MOBILE_HOST
   ```

   If the phone cannot reach the computer directly, run `bunx expo start --tunnel` in `apps/mobile`.

5. Open the app in Expo Go. The phone must be on the tailnet. Use one of these:

   - Tap the server under **Development servers** on the Expo Go Home tab (shows when both sides use the same Expo account).
   - iOS: type `exp://devserver.tail661ee3.ts.net:8082` in the Safari address bar and open it in Expo Go.
   - Android: tap **Enter URL manually** in Expo Go and enter the same URL.
   - Scan the QR code that Metro prints.

   `just mobile` sets `REACT_NATIVE_PACKAGER_HOSTNAME` from `ORBIT_MOBILE_HOST`, so Metro tells the phone to load the app from that host.

6. Optional: to test Google or GitHub sign-in in Expo Go, allow the app's Expo Go redirect on the server. A development or store build sends `orbit://auth/callback`, which every server accepts. Expo Go sends its own URL instead, for example `exp://devserver.tail661ee3.ts.net:8082/--/auth/callback` (the Metro host and port, then `/--/auth/callback`). Add that exact value to the server's environment and restart it:

   ```bash
   ORBIT_DEV_NATIVE_REDIRECTS=exp://devserver.tail661ee3.ts.net:8082/--/auth/callback
   ```

   The variable takes a comma-separated list, and production configurations ignore it. Without it, the start request fails with `invalid_redirect`.

7. Sign in with a seed account: `member@example.com` / `password` (create it with `just seed`).

You should see the Home tab. Settings shows your profile and the server, and signs you out.

## Checks

```bash
cd apps/mobile
bunx tsc --noEmit
bunx tsc --noEmit -p scripts
bun test
bun scripts/api-check.ts            # a running server (default http://127.0.0.1:18080, test@example.com / password); creates a task, a comment and a message, then deletes them
bun scripts/api-check-tasks.ts      # every task property, relations, sub-issues and comment attachments
bun scripts/api-check-chat.ts       # channels, reactions, threads, uploads, search
bun scripts/api-check-settings.ts   # profile, avatar, notifications, sessions, members, docs, admin reads
bunx expo-doctor
```

## Theme and icons

The colors, radii and icons come from the web app. Do not edit the generated files; regenerate them from the repo root after the web changes:

```bash
bun apps/mobile/scripts/generate-theme.ts   # apps/web/src/index.css → src/theme/tokens.gen.ts and theme.gen.css
bun apps/mobile/scripts/generate-icons.ts   # reicon-react Outline icons → src/ui/icons.gen.ts
```

To add an icon, add its reicon name to `ICONS` in `scripts/generate-icons.ts` and rerun the script. Use the name the web imports.

`just check` at the repo root also runs the mobile typecheck.

## Store builds

The sign-in screen fills in Orbit Cloud (`https://app.orbit.so`) unless `EXPO_PUBLIC_ORBIT_SERVER` is set, so a cloud user only types an email and a password. A self-hosted user replaces the server.

What is configured:

- `app.json`: the iOS bundle identifier and the Android package are `io.coollabs.orbit`. `ios.privacyManifests` declares no tracking, the data the app collects (email, name, photos or videos, and other user content, linked to the user, for app functionality only), and the required-reason APIs that React Native and the Expo modules use (UserDefaults, file timestamps, system boot time, disk space). The reasons are copied from each package's `PrivacyInfo.xcprivacy`. After you add a native module, check its manifest and add its reasons.
- `eas.json`: the build version numbers live on EAS (`appVersionSource: remote`). The `development` profile makes an internal development build, `preview` an internal build, and `production` a store build that increments the build number.
- The billing banner only shows the trial's days left or that the workspace is read-only. It has no link to billing. App Store guideline 3.1.1 and the Google Play payments policy forbid pointing to purchases outside the app.

What you still have to do:

1. Create the project on EAS: `bunx eas-cli init` in `apps/mobile`. It adds `extra.eas.projectId` to `app.json`.
2. Before you build the `development` profile, install the dev client: `bunx expo install expo-dev-client`.
3. Create the apps in App Store Connect and in the Google Play Console with the identifier `io.coollabs.orbit`.
4. Fill in `submit.production` in `eas.json`. iOS takes `appleId` (or `EXPO_APPLE_ID`), `ascAppId` (the App Store Connect app ID) and `appleTeamId`. Android takes `serviceAccountKeyPath` (a Google service account JSON key with access to the Play Console; keep it out of git, or upload it to EAS instead) and `track` (`internal` for the first upload). Google Play requires the first upload of an app to be done by hand in the Play Console.
5. Build and submit: `bunx eas-cli build --platform all --profile production`, then `bunx eas-cli submit --platform ios --profile production` (and `android`).
6. Fill in the App Store privacy labels and the Play data safety form so that they match `ios.privacyManifests`.

## Next steps

- Move to a development build (`bunx expo run:ios|android`, or EAS) when the app needs native modules that Expo Go does not include, for example APNs/FCM push.
- Browser-sheet sign-in for passkeys. Admin → Sign-in providers (client ID and secret) is web-only.
