# Release video and screenshots: guide for AI agents

Use this guide to make the product update video of an Orbit release, and optionally the screenshots in the GitHub
release notes. Do all steps in order. Each step ends with a check: do not continue until the check passes.

The reference is v0.3.0: `marketing/releases/v0.3.0/` (`video.json`, `notes.md`, `contact-sheet.jpg`). Look at the
contact sheet and read `video.json` before you start. This flow is a port of Coolify's release video kit
(`coolify/marketing/release-video`, the v4.7 video), which the maintainer likes.

## What you make

| Output | Where | In git |
| --- | --- | --- |
| Update video: 1920x1080 MP4, about 50 to 75 seconds, silent | `marketing/releases/<version>/orbit-<version>-update.mp4` | No |
| Video spec | `marketing/releases/<version>/video.json` | Yes |
| All slides in one image | `marketing/releases/<version>/contact-sheet.jpg` | Yes |
| Highlights text of the release notes (only if the user asks for screenshots in the release) | `marketing/releases/<version>/notes.md` | Yes |
| Screenshots and slides | `marketing/releases/<version>/build/` | No |

The video stays local. Do not upload it to the GitHub release, and do not upload screenshots, unless the user asks.

## Files in this folder

| File | What it does |
| --- | --- |
| `build-tag.sh` | Builds a release tag in `/root/orbit-release-<tag>/` and starts it with seed data on port 19080 |
| `seed-demo.sh` + `demo-database.json` | Adds demo content: the "Launch plan" database with all view types, an icon, pages in the trash |
| `scenes.mjs` | One function per screenshot. Add scenes for the new features of each release |
| `shoot.mjs` | Signs in as `test@example.com` / `password` and runs the scenes (dark theme, 1440x900 at 2x) |
| `slides.mjs` | Makes 1920x1080 slide PNGs from `video.json` and the screenshots |
| `render.mjs` | Joins the slides with 0.6-second fades into the MP4, and writes `contact-sheet.jpg` |
| `check-release.mjs` | Checks that every image in the GitHub release notes loads |

## Requirements

- `bun install` at the repo root (Playwright and `@fontsource-variable/noto-sans` come from the root `node_modules`).
- `ffmpeg`, `jq`, `cargo`, `bun` on the host.
- About 3 GB free on the main disk. Do not build in `/tmp`: it is a 16 GB RAM disk, and a cargo build fills it.

## Steps

### 1. Check the release

```bash
gh release view v0.4.0
gh run list --limit 5
```

- Read the release notes. Select 6 to 10 features that a user can see in the web app. Put up to 6 other items on
  the "Also in" list slide. Put one breaking change that users must know in the outro, if there is one.
- Check that the release build passed. A release without binaries and image is not finished: tell the user. (The
  v0.3.0 build failed because the `Dockerfile` did not copy a new workspace package in `packages/`.)

### 2. Build the tag with demo data

```bash
marketing/release-video/build-tag.sh v0.4.0
marketing/release-video/seed-demo.sh
```

- Use a clean build of the tag, not the dev server. Main and the dev server can have changes that are not in the
  release, and the dev server can run an older API.
- Pages must show real data, not empty states. Add demo data for the new features to `seed-demo.sh`. Use the
  API with the session token, as the script does. Page updates and deletes need `?expected_version=<version>`.
- Check: open `http://127.0.0.1:19080`, sign in as `test@example.com` / `password`, and look at the new features.

### 3. Add the scenes

- Add one function to `scenes.mjs` for each feature screen. Use the `view()` and `route()` helpers. A scene calls
  `shot(name)` one time; `name` is the scene's key.
- Remove the scenes of older releases that you do not use.
- The donation reminder shows on the first page after sign-in. The scenes close it.
- Billing shows only on Orbit Cloud. A self-hosted build cannot show it.
- Mobile screens need Expo or a phone. Put mobile features on the list slide.

### 4. Take the screenshots

```bash
node marketing/release-video/shoot.mjs marketing/releases/v0.4.0/build/shots http://127.0.0.1:19080
```

- Look at each PNG. A page can show an error, an empty state, a hover mark or a closed menu with no error message.
  Do not put a broken page in the video. If the app has a bug, fix it or tell the user.

### 5. Write the spec

Copy `marketing/releases/v0.3.0/video.json` to `marketing/releases/v0.4.0/video.json`. Change `version`, `notes`
and `slides`.

| Slide type | Fields |
| --- | --- |
| `title` | `subtitle`: one line with the main features |
| feature (no `type`) | `tag`, `title`, `shot` (scene name), `box` |
| `list` | `tag` (for example "Also in v0.4.0"), `items` (6 at most, `<b>` for the key words) |
| `outro` | `title` ("Update to v0.4.0 today"), `lines` |

Optional on any slide: `seconds` (default: title 4, feature 5, list 8, outro 5).
Optional on a feature slide:
- `box`: `[x, y, width, height]` in the screenshot's CSS pixels (1440x900). The slide zooms to the box (1.17x to
  1.7x) and darkens everything outside it.
- `hideLeft`: the crop hides everything left of this x. Default 260 (the app sidebar). Use 224 on Settings pages.
- `shiftDown`: moves the crop down in slide pixels. A negative value moves it up.

To find a box, read the screenshot. The image you see is scaled: compute CSS pixels from the PNG size
(2880x1800 PNG = 1440x900 CSS).

### 6. Render and review the slides

```bash
node marketing/release-video/slides.mjs marketing/releases/v0.4.0/video.json
```

Read each PNG in `build/slides/`. Fix `box`, `hideLeft` or `shiftDown` when:
- the box cuts text or a control, or misses part of the feature,
- the crop shows half a page title or half the sidebar at the edge,
- the box is so big that the text is too small to read (make the box smaller to zoom in).

Render again until all slides are good.

### 7. Render the video

```bash
node marketing/release-video/render.mjs marketing/releases/v0.4.0/video.json
ffprobe -v error -show_entries format=duration:stream=codec_type -of csv=p=0 marketing/releases/v0.4.0/orbit-v0.4.0-update.mp4
```

Check: the duration is 50 to 75 seconds and there is only a video stream. Read `contact-sheet.jpg` for a last check.

### 8. Optional: screenshots in the GitHub release notes

Only when the user asks. Copy `marketing/releases/v0.3.0/notes.md` to the new release folder, and write the
Highlights text for the new features.

```bash
gh release upload v0.4.0 marketing/releases/v0.4.0/build/shots/screenshot-*.png
gh release view v0.4.0 --json body -q .body > /tmp/old.md
sed 's#<tag>#v0.4.0#g' marketing/releases/v0.4.0/notes.md | cat - /tmp/old.md > /tmp/new.md
gh release edit v0.4.0 --notes-file /tmp/new.md
node marketing/release-video/check-release.mjs v0.4.0
```

Release asset URLs show as images in the notes because the repo is public.

### 9. Clean up and report

```bash
pkill -f '/root/orbit-release-v0.4.0/target/debug/orbit serve'
rm -rf /root/orbit-release-v0.4.0
```

- Commit only `video.json`, `contact-sheet.jpg` and `notes.md`. Never commit the MP4 or `build/` (`.gitignore`
  skips them). Keep `build/shots/` locally: with it, `slides.mjs` and `render.mjs` make the video again without a
  new build.
- Give the user the path of the MP4.

## Style rules (from the maintainer)

- No music. A synthesized soundtrack was rejected.
- One short title for each slide, no description lines. People cannot read long text in 5 seconds.
- The app's font, Noto Sans (`slides.mjs` embeds it). Fallback fonts make letters collide.
- Screenshots at 2x scale and dark theme (`shoot.mjs` does this). Low-resolution screenshots are blurry after zoom.
- Do not show half of the sidebar.
- Orbit colors: pink highlight box (`#f2458f`), dark pink gradient background.
- Do not feature small admin items (for example the donation reminder) as their own slide.
