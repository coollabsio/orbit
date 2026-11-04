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

## Promo (44 seconds, with music)

`src/Promo.tsx` is a separate `Promo` composition (1320 frames): logo, "Tasks Docs Chat Mail AI / One app.", "Replaces Linear, Notion, Slack, Discord", one slide each for Tasks, Timeline, Docs, Chat, Mail and AI (a concept UI, because the assistant is not built yet; this slide is longer so it can be read), self-hosting facts, Orbit Cloud pricing and an `orbit.so` end card.

`scripts/make-beat.mjs` synthesizes the soundtrack (no samples) in three styles: `STYLE=synthwave` (default), `STYLE=futurebass` or `STYLE=bach` (solo piano: Bach, Prelude in C major BWV 846, public domain; one bar = 2 s = 60 frames). At 120 BPM and 30 fps one beat is 15 frames, so every scene starts on a beat. If you change scene lengths, change `LENGTH`, `DROP`, `ARP` and `END` in the script too.

```bash
bun run render:promo                                   # → out/orbit-promo-synthwave.mp4
STYLE=futurebass bun run render:promo                  # → out/orbit-promo-futurebass.mp4
STYLE=bach bun run render:promo                        # → out/orbit-promo-bach.mp4
STYLE=none bun run render:promo                        # → out/orbit-promo-silent.mp4 (no music)
MUSIC=out/interstellar.mp3 bun run render:promo        # → out/orbit-promo-interstellar.mp4 (MUSIC_START=341)
```

- "a few dozen MB of RAM": the release build used 45 MB at start and 78 MB after loading Tasks, Docs, Chat and Activity with demo data (2026-11-04). Measure again before you change the claim.
- To capture new screenshots, run a release server on a fresh SQLite database (`orbit seed`), then `scripts/seed-demo-tasks.ts`, `scripts/seed-demo-docs.ts` and `scripts/seed-demo-chat.ts` against it, start Vite with `scripts/vite.capture.config.ts` (`VITE_API_PROXY` = that server) and run `bun run capture`.
