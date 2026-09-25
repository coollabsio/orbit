# Orbit launch video

An 89-second, 1920×1080 product launch video (Tasks as a Linear alternative, Docs as a Notion alternative with Notion import, Mail and Chat as coming soon) made with [Remotion](https://www.remotion.dev) from real Orbit screenshots.

```bash
bun install
bun run studio    # preview and scrub in the browser
bun run render    # → out/orbit-launch.mp4
```

- Screenshots: `public/shots/*.png` (1600×1000 at 2×). `manifest.json` stores the zoom regions the scenes use.
  Mock data: `scripts/seed-demo-tasks.ts` and `scripts/seed-demo-docs.ts`. To capture again, start the web app on port 18889, then run `bun run capture`.
- Scenes: `src/scenes/` (Opening, Features, Docs, Upcoming, Closing). The order and durations are in `src/LaunchVideo.tsx`.
- `typescript` is pinned to 5.x: Remotion's bundler reads `tsconfig.json` through the TypeScript 5 API.
- Mail and Chat are not routed in the app yet. For their shots, run Vite with `scripts/vite.capture.config.ts`. It enables those routes in memory only and swaps in the mock seeds from `scripts/seed/`. The Notion import steps come from mocked API responses in `capture.mjs`.
- Soundtrack: `bun run render:music` renders the video silently, then adds `out/interstellar.mp3` from 4:56 as AAC at the source sample rate, with no processing (`MUSIC_START=<seconds>` to change it, `MUSIC=<file>` for another track). At 4:56, the 6:20 climax lands on the end card and the video ends as the track fades out.
