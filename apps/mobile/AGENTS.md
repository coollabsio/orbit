# Orbit Mobile agent instructions

Read `README.md` first. It explains how the app signs in to an Orbit server (bearer session token, no `Origin` header) and how to run it against the dev server.

- Keep business rules on the server. The app is a thin client of the Orbit HTTP API.
- Store the session token only with `expo-secure-store`. Never put it in a URL or in logs.
- Add dependencies with `bunx expo install <package>` in `apps/mobile`. The repo root is one Bun workspace with the hoisted linker: web and mobile must use the same `react` version (Expo decides it), or one app loads two copies of React.
- Style with Uniwind `className` and the components in `src/ui`, using the classes the web uses for the same element. Keep `style` only for what a class cannot express: animated values, values computed at runtime (insets, measured sizes, colors from data, `mix()` results) and computed shadows. Props that are not styles (an `Icon`'s `color`, `placeholderTextColor`) read `useTheme().colors`. Never hard-code a color: the tokens are generated from `apps/web/src/index.css` (see README, Theme and icons).
- Uniwind does not merge conflicting classes (there is no tailwind-merge), and the order inside `className` never decides. A `src/ui` component whose defaults a call site overrides keeps them in `@layer components` in `src/global.css` (`text-body`, `pane-header`), so any utility wins. `group-*` variants do nothing in the free version: use `active:` or Pressable's `pressed`.
  - This includes the `dark:` variant: `border-black/9 dark:border-sidebar-tint/13` on one element keeps the black border in dark mode. Choose the classes for each scheme in JS (`useTheme().scheme`), or set the value in `style`.
  - Fraction widths (`w-1/2`, `w-1/3`) do nothing on native. Write percentages: `w-[50%]`.
  - A `style` key whose value is `undefined` replaces the class value (React Native merges every key). Write `...(pressed ? { backgroundColor: … } : {})`, never `backgroundColor: pressed ? … : undefined`, for a property that a class also sets.
- The custom theme and the New UI colors reach a part of the screen through `ColorScope` (`src/theme/ColorScope.tsx`), not through one-off props. `useTheme()` reads the nearest scope.

This is an Expo/React Native mobile application. Prioritize mobile-first patterns, performance, and cross-platform compatibility.

## Expo has changed — do not trust your training data

Expo ships breaking changes every SDK release. APIs you remember are likely renamed, moved, or removed. Before writing any code that touches an Expo, EAS, or React Native API:

1. Read the major version of the `expo` package in `package.json`.
2. Fetch the matching versioned docs: `https://docs.expo.dev/versions/v<major>.0.0/`
3. For anything else, fetch https://docs.expo.dev/llms.txt — an index of all Expo docs with corrections to common LLM misconceptions. Follow its links to the specific page you need; never answer from memory.

## Commands

Use `bunx` instead of `npx` if the project uses bun (`bun.lock` present).

```bash
npx expo install <package>  # ALWAYS use instead of npm/yarn/pnpm/bun add — resolves SDK-compatible versions
npx expo start              # start the dev server
npx expo lint               # lint
npx tsc --noEmit            # typecheck
npx expo-doctor             # diagnose dependency and config issues
npx expo install --fix      # fix incompatible package versions
```

Run lint and typecheck before declaring any task done.

## Navigation & Routing

- Use **Expo Router** for all navigation. Routes live in `src/app/` — every file there is a screen, `_layout.tsx` files define navigators. Keep non-route code (components, hooks, utils) outside `src/app/`.
- Import `Link`, `router`, and `useLocalSearchParams` from `expo-router`.
- Docs: https://docs.expo.dev/router/introduction.md

## Release builds (no EAS)

The project does not use EAS. The GitHub workflow `.github/workflows/testflight.yml` (manual run) builds the iOS app on a
macOS runner with `expo prebuild` and `xcodebuild`, signs it with the certificate and profile in GitHub secrets, and
uploads it to TestFlight with the App Store Connect API key. The certificate and the profile expire each year: renew
them at developer.apple.com and update the `IOS_*` secrets. Do not add `eas.json`, `eas-cli` or an EAS project link.
App icons and the splash image are in `assets/` and configured in `app.json`.

## Rules

- If `ios/` and `android/` directories do not exist, they are generated (Continuous Native Generation). Never create or edit them by hand — configure native behavior in `app.json` and config plugins.
- Expo Go only includes its bundled native modules. After adding a library with native code, the app needs a development build: `npx expo run:ios|android` locally.
- Prefer recommended Expo modules over third-party libraries, and check your available skills before adding dependencies. Docs: https://docs.expo.dev/versions/latest/index.md
